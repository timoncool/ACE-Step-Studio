import { bandFilters, EQ_BANDS, EQ_RANGE_DB, responseDb } from './equalizerCurve';

export { EQ_BANDS, EQ_RANGE_DB };

/**
 * What the studio's player sounds through: its audio element feeds one Web
 * Audio graph - preamp, ten peaking filters on Winamp's bands, mono and
 * balance - and on to the speakers; the visualisers read where it ends. The
 * filters are the browser's own BiquadFilterNode; nothing here decodes or
 * resamples.
 *
 * An element can be fed into only one graph, once, and after that it is heard
 * only through the graph, so this is made the first time the user presses
 * play (a gesture the audio context needs) and then kept.
 */

export interface EqualizerState {
  enabled: boolean;
  preamp: number;
  gains: number[];
  /** The preset the curve came from, until a band is moved. */
  preset: string | null;
  /** -1 is all left, 1 all right. Balance and mono apply with the equalizer off too. */
  balance: number;
  mono: boolean;
}

export interface AudioGraph {
  context: AudioContext;
  preamp: GainNode;
  filters: BiquadFilterNode[];
  /** Two channels, or one for mono. */
  mix: GainNode;
  left: GainNode;
  right: GainNode;
  /** Where the sound leaves the equalizer; visualisers and the tap read it. */
  output: GainNode;
}

const STORE = 'studio:equalizer';
const DEFAULT: EqualizerState = { enabled: false, preamp: 0, gains: EQ_BANDS.map(() => 0), preset: null, balance: 0, mono: false };

function load(): EqualizerState {
  try {
    const saved = JSON.parse(localStorage.getItem(STORE) ?? 'null') as Partial<EqualizerState> | null;
    if (!saved) return DEFAULT;
    const gains = Array.isArray(saved.gains) && saved.gains.length === EQ_BANDS.length ? saved.gains.map(Number) : DEFAULT.gains;
    return {
      enabled: Boolean(saved.enabled),
      preamp: Number(saved.preamp) || 0,
      gains,
      preset: saved.preset ?? null,
      balance: Math.max(-1, Math.min(1, Number(saved.balance) || 0)),
      mono: Boolean(saved.mono),
    };
  } catch {
    return DEFAULT;
  }
}

let state: EqualizerState = load();
let graph: AudioGraph | null = null;
let element: HTMLMediaElement | null = null;
const listeners = new Set<(state: EqualizerState) => void>();
const graphListeners = new Set<(graph: AudioGraph) => void>();

const decibels = (value: number) => Math.max(-EQ_RANGE_DB, Math.min(EQ_RANGE_DB, value));

function apply(): void {
  if (!graph) return;
  const now = graph.context.currentTime;
  graph.preamp.gain.setTargetAtTime(state.enabled ? Math.pow(10, decibels(state.preamp) / 20) : 1, now, 0.015);
  const bands = bandFilters(state.enabled ? state.gains.map(decibels) : EQ_BANDS.map(() => 0), graph.context.sampleRate);
  graph.filters.forEach((filter, index) => filter.gain.setTargetAtTime(bands[index].gain, now, 0.015));
  // the quieter side is turned down, the other stays
  graph.left.gain.setTargetAtTime(Math.min(1, 1 - state.balance), now, 0.015);
  graph.right.gain.setTargetAtTime(Math.min(1, 1 + state.balance), now, 0.015);
  graph.mix.channelCount = state.mono ? 1 : 2;
}

/** Feeds the player's element through the graph; the same graph on every call. */
export function attachAudioGraph(audio: HTMLMediaElement): AudioGraph {
  if (graph && element === audio) return graph;
  if (graph) throw new Error('the audio graph belongs to another element');
  const context = new AudioContext();
  const source = context.createMediaElementSource(audio);
  const preamp = context.createGain();
  const filters = bandFilters(EQ_BANDS.map(() => 0), context.sampleRate).map((band) => {
    const filter = context.createBiquadFilter();
    filter.type = 'peaking';
    filter.frequency.value = band.frequency;
    filter.Q.value = band.q;
    return filter;
  });
  // mono is a one-channel mix, (L + R) / 2, spread back over both sides
  const mix = context.createGain();
  mix.channelCountMode = 'explicit';
  mix.channelInterpretation = 'speakers';
  const spread = context.createGain();
  spread.channelCount = 2;
  spread.channelCountMode = 'explicit';
  spread.channelInterpretation = 'speakers';
  const split = context.createChannelSplitter(2);
  const merge = context.createChannelMerger(2);
  const left = context.createGain();
  const right = context.createGain();
  const output = context.createGain();
  source.connect(preamp);
  filters.reduce<AudioNode>((previous, filter) => {
    previous.connect(filter);
    return filter;
  }, preamp).connect(mix);
  mix.connect(spread).connect(split);
  split.connect(left, 0).connect(merge, 0, 0);
  split.connect(right, 1).connect(merge, 0, 1);
  merge.connect(output);
  output.connect(context.destination);
  graph = { context, preamp, filters, mix, left, right, output };
  element = audio;
  apply();
  graphListeners.forEach((listener) => listener(graph as AudioGraph));
  return graph;
}

let player: HTMLMediaElement | null = null;

/** The studio's player element; the graph is made from it when first needed. */
export function registerPlayer(audio: HTMLMediaElement): void {
  player = audio;
  if (needsGraph()) ensureAudioGraph();
}

/** The equalizer, balance or mono change the sound; plain playback does not need the graph. */
function needsGraph(): boolean {
  return state.enabled || state.balance !== 0 || state.mono;
}

/** The graph, made now if the player is there; woken if the browser holds it. */
export function ensureAudioGraph(): AudioGraph | null {
  if (!graph && player) attachAudioGraph(player);
  void resumeAudioGraph();
  return graph;
}

/** Wakes the context: the browser suspends one made before a gesture. */
export async function resumeAudioGraph(): Promise<void> {
  if (graph && graph.context.state === 'suspended') await graph.context.resume();
}

export function audioGraph(): AudioGraph | null {
  return graph;
}

/** Called with the graph as soon as there is one. */
export function onAudioGraph(listener: (graph: AudioGraph) => void): () => void {
  graphListeners.add(listener);
  if (graph) listener(graph);
  return () => graphListeners.delete(listener);
}

export function equalizer(): EqualizerState {
  return state;
}

export function setEqualizer(change: Partial<EqualizerState>): void {
  state = { ...state, ...change };
  try {
    localStorage.setItem(STORE, JSON.stringify(state));
  } catch {
    // the equalizer still works for this session
  }
  if (needsGraph()) ensureAudioGraph();
  apply();
  listeners.forEach((listener) => listener(state));
}

export function onEqualizer(listener: (state: EqualizerState) => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** The response the filters and the preamp play, in decibels, at the given frequencies: what the curve shows. */
export function equalizerResponse(frequencies: ArrayLike<number>, eq: EqualizerState): number[] {
  if (!eq.enabled) return Array.from(frequencies, () => 0);
  return responseDb(frequencies, eq.gains.map(decibels), decibels(eq.preamp), graph?.context.sampleRate ?? 48000);
}
