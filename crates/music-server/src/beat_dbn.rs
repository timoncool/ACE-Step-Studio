//! madmom's DBN downbeat tracker, as beat_this_cpp (mosynthkey, MIT) ports it
//! in Source/DBNPostprocessor.cpp, and Beat This! uses it to turn its beat
//! and downbeat activations into a steady beat sequence: a bar-position state
//! space for 3 and 4 beats per bar, exponential tempo transitions, a
//! three-class observation model and a checkpointed Viterbi.

const EPSILON: f64 = 1e-5;

pub struct Config {
    pub beats_per_bar: Vec<usize>,
    pub min_bpm: f64,
    pub max_bpm: f64,
    pub fps: f64,
    pub transition_lambda: f64,
    pub observation_lambda: f64,
    pub threshold: f64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            beats_per_bar: vec![3, 4],
            min_bpm: 55.0,
            max_bpm: 215.0,
            fps: 50.0,
            transition_lambda: 100.0,
            observation_lambda: 16.0,
            threshold: 0.05,
        }
    }
}

struct StateSpace {
    num_states: usize,
    positions: Vec<f64>,
    intervals_of: Vec<usize>,
    first: Vec<Vec<usize>>,
    last: Vec<Vec<usize>>,
    num_intervals: usize,
}

impl StateSpace {
    fn build(num_beats: usize, min_interval: f64, max_interval: f64) -> Self {
        let intervals: Vec<usize> = (min_interval.round() as usize..=max_interval.round() as usize).collect();
        let beat_states: usize = intervals.iter().sum();
        let (mut beat_first, mut beat_last, mut beat_pos, mut beat_iv) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let mut index = 0;
        for &interval in &intervals {
            beat_first.push(index);
            beat_last.push(index + interval - 1);
            for step in 0..interval {
                beat_pos.push(step as f64 / interval as f64);
                beat_iv.push(interval);
            }
            index += interval;
        }
        let num_states = num_beats * beat_states;
        let (mut positions, mut intervals_of) = (vec![0.0; num_states], vec![0; num_states]);
        let (mut first, mut last) = (Vec::new(), Vec::new());
        for beat in 0..num_beats {
            let offset = beat * beat_states;
            for state in 0..beat_states {
                positions[offset + state] = beat_pos[state] + beat as f64;
                intervals_of[offset + state] = beat_iv[state];
            }
            first.push(beat_first.iter().map(|state| state + offset).collect());
            last.push(beat_last.iter().map(|state| state + offset).collect());
        }
        StateSpace { num_states, positions, intervals_of, first, last, num_intervals: intervals.len() }
    }
}

/// Transitions in compressed rows: for every state, the states it can come
/// from and the log probability of each.
struct Transitions {
    pointers: Vec<usize>,
    sources: Vec<usize>,
    log_probs: Vec<f64>,
}

fn exponential_transition(from: &[usize], to: &[usize], lambda: f64) -> Vec<Vec<f64>> {
    from.iter()
        .map(|&source| {
            let mut row: Vec<f64> = to
                .iter()
                .map(|&target| {
                    let probability = (-lambda * (target as f64 / source as f64 - 1.0).abs()).exp();
                    if probability <= f64::EPSILON { 0.0 } else { probability }
                })
                .collect();
            let sum: f64 = row.iter().sum();
            if sum > 0.0 {
                row.iter_mut().for_each(|value| *value /= sum);
            }
            row
        })
        .collect()
}

impl Transitions {
    fn build(space: &StateSpace, lambda: f64) -> Self {
        let num_beats = space.first.len();
        let mut is_first = vec![false; space.num_states];
        for beat in &space.first {
            for &state in beat {
                is_first[state] = true;
            }
        }
        let mut entries: Vec<(usize, usize, f64)> = Vec::new();
        for state in 0..space.num_states {
            if !is_first[state] {
                entries.push((state, state - 1, 1.0));
            }
        }
        for beat in 0..num_beats {
            let to_states = &space.first[beat];
            let from_states = &space.last[(beat + num_beats - 1) % num_beats];
            let from: Vec<usize> = from_states.iter().map(|&state| space.intervals_of[state]).collect();
            let to: Vec<usize> = to_states.iter().map(|&state| space.intervals_of[state]).collect();
            let probabilities = exponential_transition(&from, &to, lambda);
            for i in 0..space.num_intervals {
                for j in 0..space.num_intervals {
                    if probabilities[i][j] > 0.0 {
                        entries.push((to_states[j], from_states[i], probabilities[i][j]));
                    }
                }
            }
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        let mut pointers = vec![0usize; space.num_states + 1];
        for entry in &entries {
            pointers[entry.0 + 1] += 1;
        }
        for state in 0..space.num_states {
            pointers[state + 1] += pointers[state];
        }
        Transitions {
            pointers,
            sources: entries.iter().map(|entry| entry.1).collect(),
            log_probs: entries.iter().map(|entry| entry.2.ln()).collect(),
        }
    }
}

struct Model {
    space: StateSpace,
    transitions: Transitions,
    /// Per state: 0 no beat, 1 beat, 2 downbeat.
    observation: Vec<usize>,
}

impl Model {
    fn build(num_beats: usize, config: &Config) -> Self {
        let min_interval = 60.0 * config.fps / config.max_bpm;
        let max_interval = 60.0 * config.fps / config.min_bpm;
        let space = StateSpace::build(num_beats, min_interval, max_interval);
        let transitions = Transitions::build(&space, config.transition_lambda);
        let border = 1.0 / config.observation_lambda;
        let observation = space
            .positions
            .iter()
            .map(|&position| {
                if position < border {
                    2
                } else if position.rem_euclid(1.0) < border {
                    1
                } else {
                    0
                }
            })
            .collect();
        Model { space, transitions, observation }
    }

    /// The best state path and its log probability.
    fn viterbi(&self, densities: &[[f64; 3]]) -> (Vec<usize>, f64) {
        let steps = densities.len();
        let states = self.space.num_states;
        let initial = -(states as f64).ln();
        let step = |previous: &[f64], current: &mut [f64], t: usize, mut back: Option<&mut [usize]>| {
            for state in 0..states {
                let density = densities[t][self.observation[state]];
                let (mut best, mut from) = (f64::NEG_INFINITY, 0usize);
                for at in self.transitions.pointers[state]..self.transitions.pointers[state + 1] {
                    let source = self.transitions.sources[at];
                    let value = previous[source] + self.transitions.log_probs[at] + density;
                    if value > best {
                        best = value;
                        from = source;
                    }
                }
                current[state] = best;
                if let Some(back) = back.as_deref_mut() {
                    back[state] = from;
                }
            }
        };

        // Forward pass, keeping the scores every sqrt(T) steps
        let segment = ((steps as f64).sqrt() as usize).max(1);
        let segments = steps.div_ceil(segment);
        let mut checkpoints: Vec<Vec<f64>> = Vec::with_capacity(segments);
        let mut previous = vec![initial; states];
        let mut current = vec![0.0; states];
        checkpoints.push(previous.clone());
        for t in 0..steps {
            step(&previous, &mut current, t, None);
            std::mem::swap(&mut previous, &mut current);
            if (t + 1) % segment == 0 && (t + 1) / segment < segments {
                checkpoints.push(previous.clone());
            }
        }
        let (best_state, best) = previous
            .iter()
            .enumerate()
            .fold((0, f64::NEG_INFINITY), |acc, (state, &value)| if value > acc.1 { (state, value) } else { acc });
        if best.is_infinite() {
            return (Vec::new(), best);
        }

        // Backward pass, recomputing each segment's back pointers from its checkpoint
        let mut path = vec![0usize; steps];
        let mut state = best_state;
        for index in (0..segments).rev() {
            let start = index * segment;
            let end = steps.min(start + segment);
            let mut back = vec![0usize; (end - start) * states];
            let mut previous = checkpoints[index].clone();
            let mut current = vec![0.0; states];
            for t in start..end {
                let row = &mut back[(t - start) * states..(t - start + 1) * states];
                step(&previous, &mut current, t, Some(row));
                std::mem::swap(&mut previous, &mut current);
            }
            for t in (start..end).rev() {
                path[t] = state;
                state = back[(t - start) * states + state];
            }
        }
        (path, best)
    }
}

/// Beat This! logits to the DBN's activations: sigmoids, the beat share with
/// the downbeat taken out.
fn activations(beat: &[f32], downbeat: &[f32]) -> Vec<[f64; 2]> {
    beat.iter()
        .zip(downbeat)
        .map(|(&b, &d)| {
            let sigmoid = |logit: f32| 1.0 / (1.0 + (-(logit as f64)).exp());
            let beat = sigmoid(b) * (1.0 - EPSILON) + EPSILON / 2.0;
            let down = sigmoid(d) * (1.0 - EPSILON) + EPSILON / 2.0;
            [(beat - down).max(EPSILON / 2.0), down]
        })
        .collect()
}

/// Beat times in seconds from Beat This! beat and downbeat logits.
pub fn beat_times(beat: &[f32], downbeat: &[f32], config: &Config) -> Vec<f64> {
    let act = activations(beat, downbeat);
    let Some(first) = act.iter().position(|a| a[0] >= config.threshold || a[1] >= config.threshold) else { return Vec::new() };
    let last = act.iter().rposition(|a| a[0] >= config.threshold || a[1] >= config.threshold).expect("a first implies a last") + 1;
    let act = &act[first..last];

    let lambda = config.observation_lambda;
    let densities: Vec<[f64; 3]> = act.iter().map(|a| [((1.0 - (a[0] + a[1])) / (lambda - 1.0)).ln(), a[0].ln(), a[1].ln()]).collect();
    let (model, path) = config
        .beats_per_bar
        .iter()
        .map(|&beats| {
            let model = Model::build(beats, config);
            let (path, score) = model.viterbi(&densities);
            (model, path, score)
        })
        .fold(None::<(Model, Vec<usize>, f64)>, |best, next| match best {
            Some(best) if best.2 >= next.2 => Some(best),
            _ => Some(next),
        })
        .map(|(model, path, _)| (model, path))
        .expect("at least one bar length");
    if path.is_empty() {
        return Vec::new();
    }

    // The frame of the strongest activation inside every run of beat states
    let on_beat: Vec<bool> = path.iter().map(|&state| model.observation[state] >= 1).collect();
    let mut edges = Vec::new();
    if on_beat[0] {
        edges.push(0);
    }
    for index in 1..on_beat.len() {
        if on_beat[index] != on_beat[index - 1] {
            edges.push(index);
        }
    }
    if *on_beat.last().expect("a path") {
        edges.push(on_beat.len());
    }
    edges
        .chunks(2)
        .filter(|pair| pair.len() == 2)
        .map(|pair| {
            let peak = (pair[0]..pair[1])
                .max_by(|&a, &b| act[a][0].max(act[a][1]).total_cmp(&act[b][0].max(act[b][1])))
                .expect("a non-empty run");
            (peak + first) as f64 / config.fps
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clean_pulse_comes_back_as_its_beats() {
        // 120 BPM at 50 fps: a beat every 25 frames, a downbeat every fourth
        let frames = 1000;
        let mut beat = vec![-8.0f32; frames];
        let mut down = vec![-8.0f32; frames];
        for (count, frame) in (10..frames).step_by(25).enumerate() {
            beat[frame] = 6.0;
            if count % 4 == 0 {
                down[frame] = 6.0;
            }
        }
        let times = beat_times(&beat, &down, &Config::default());
        assert!(times.len() >= 38, "{}", times.len());
        let spacing = (times[times.len() - 1] - times[0]) / (times.len() - 1) as f64;
        assert!((spacing - 0.5).abs() < 0.01, "{spacing}");
    }
}
