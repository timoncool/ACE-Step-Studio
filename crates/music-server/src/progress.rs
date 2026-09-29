//! Generation progress read from the engine log.
//!
//! acestep.cpp reports a job as `running` and nothing finer; its log counts the
//! language model's code steps every 50 tokens against the `max_tokens` it
//! announced, every DiT step, and the VAE decode. The language model takes the
//! first 40% of the bar, the DiT the next 55%, the VAE the rest.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// The language model planning the song and writing its audio codes.
    Planning,
    /// The DiT rendering the latents step by step.
    Rendering,
    /// The VAE turning the latents into audio.
    Decoding,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Progress {
    pub stage: Stage,
    /// Overall fraction of the job, 0 to 1.
    pub fraction: f64,
    /// The stage's own counter, e.g. `4/8`.
    pub detail: String,
    /// The code budget the language model announced, which its steps count against.
    #[serde(skip)]
    budget: f64,
}

const LM_SHARE: f64 = 0.4;
const DIT_SHARE: f64 = 0.55;

fn planning(fraction: f64, detail: String, budget: f64) -> Progress {
    Progress { stage: Stage::Planning, fraction, detail, budget }
}

/// `[LM-Phase2] Step 500, 1 active, ...`: the step count before the comma.
fn code_step(line: &str) -> Option<f64> {
    line.strip_prefix("[LM-Phase2] Step ")?.split(',').next()?.trim().parse().ok()
}

/// `[LM-Phase2] max_tokens: 1000, CFG: ...`: the budget before the comma.
fn code_budget(line: &str) -> Option<f64> {
    line.strip_prefix("[LM-Phase2] max_tokens:")?.split(',').next()?.trim().parse().ok()
}

/// `[DiT] Step 4/8 t=0.500`: the step and the step count.
fn dit_step(line: &str) -> Option<(f64, f64)> {
    let (done, rest) = line.strip_prefix("[DiT] Step ")?.split_once('/')?;
    let total: f64 = rest.split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()?;
    let done: f64 = done.trim().parse().ok()?;
    (total > 0.0).then_some((done, total))
}

/// The progress after one more line of the engine log: a counter moves it, the
/// end of a job clears it, any other line leaves it as it was.
pub fn step(current: Option<Progress>, line: &str) -> Option<Progress> {
    let line = line.trim();
    if (line.starts_with("[Server] Job") && line.contains(" done"))
        || line.starts_with("[Server] Cancel")
        || line.contains("Cancelled")
        || line.contains("FATAL")
    {
        None
    } else if line.starts_with("[VAE] Decoded") || line.starts_with("[VAE] Tiled decode") {
        Some(Progress { stage: Stage::Decoding, fraction: LM_SHARE + DIT_SHARE, detail: String::new(), budget: 0.0 })
    } else if let Some((done, total)) = dit_step(line) {
        Some(Progress {
            stage: Stage::Rendering,
            fraction: LM_SHARE + DIT_SHARE * (done / total).clamp(0.0, 1.0),
            detail: format!("{done}/{total}"),
            budget: 0.0,
        })
    } else if let Some(budget) = code_budget(line) {
        Some(planning(0.0, String::new(), budget))
    } else if let Some(done) = code_step(line) {
        let budget = current.as_ref().map_or(0.0, |progress| progress.budget);
        let fraction = if budget > 0.0 { LM_SHARE * (done / budget).min(1.0) } else { 0.0 };
        Some(planning(fraction, if budget > 0.0 { format!("{done}/{budget}") } else { format!("{done}") }, budget))
    } else if line.starts_with("[LM-Phase1]") || line.starts_with("[LM-Generate]") {
        Some(planning(0.0, String::new(), 0.0))
    } else {
        current
    }
}

/// The progress of the job the engine is running now, or `None` when the last
/// thing the log says is that it finished, failed or was cancelled.
#[cfg(test)]
pub fn from_log(lines: &[String]) -> Option<Progress> {
    lines.iter().fold(None, |current, line| step(current, line))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(str::to_owned).collect()
    }

    #[test]
    fn counts_code_steps_against_the_announced_budget() {
        let progress = from_log(&lines("[LM-Phase2] max_tokens: 1000, CFG: 2.00, N=1, prompts=1\n[LM-Phase2] Step 500, 1 active, 500 total codes, 80.0 tok/s")).unwrap();
        assert_eq!(progress.stage, Stage::Planning);
        assert!((progress.fraction - 0.2).abs() < 1e-9);
    }

    #[test]
    fn puts_the_dit_after_the_language_model() {
        let progress = from_log(&lines("[LM-Phase2] Decode 900ms\n[DiT] Step 4/8 t=0.500")).unwrap();
        assert_eq!(progress.stage, Stage::Rendering);
        assert!((progress.fraction - 0.675).abs() < 1e-9);
    }

    #[test]
    fn the_decode_follows_the_last_dit_step() {
        let log = lines("[DiT] Step 8/8 t=0.100\n[VAE] Tiled decode: 4 tiles (chunk=256, overlap=64, stride=192)");
        assert_eq!(from_log(&log).unwrap().stage, Stage::Decoding);
        assert!(from_log(&lines("[Server] ready")).is_none());
    }

    #[test]
    fn a_finished_or_cancelled_job_is_not_progress() {
        assert!(from_log(&lines("[DiT] Step 8/8 t=0.100\n[Server] Job 7f91 done (1 tracks)")).is_none());
        assert!(from_log(&lines("[LM-Phase2] Step 100, 1 active\n[LM-Phase2] Cancelled at step 120")).is_none());
        assert!(from_log(&lines("[DiT] Step 3/8 t=0.600\n[Server] Cancel requested for job 7f91")).is_none());
    }
}
