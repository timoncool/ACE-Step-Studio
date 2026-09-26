/**
 * What the visualiser shows and where, shared by the studio window, the
 * visualiser's own window and the agent: one state kept alike in every window
 * over a BroadcastChannel. The view that draws reports the preset it shows;
 * commands (next, previous) go to whichever window draws.
 */

export type VisualizerEngine = 'milkdrop' | 'spectrum';
export type VisualizerPlace = 'closed' | 'panel' | 'window';

export interface VisualizerState {
  place: VisualizerPlace;
  engine: VisualizerEngine;
  /** The MilkDrop preset shown or asked for, by name. */
  preset: string | null;
  /** The spectrum look, by id. */
  look: string;
  /** MilkDrop stays on the preset. */
  locked: boolean;
  /** MilkDrop moves on at random rather than in order. */
  random: boolean;
  showName: boolean;
  /** How long MilkDrop stays on a preset. */
  cycleSeconds: number;
  fullscreen: boolean;
}

export type VisualizerCommand = 'next' | 'previous';

type Message = { kind: 'state'; state: VisualizerState } | { kind: 'command'; command: VisualizerCommand } | { kind: 'ask' };

const STORE = 'studio:visualizer';
const CHANNEL = 'studio:visualizer-state';

const DEFAULT: VisualizerState = {
  place: 'closed',
  engine: 'milkdrop',
  preset: null,
  look: 'winamp',
  locked: false,
  random: true,
  showName: true,
  cycleSeconds: 15,
  fullscreen: false,
};

function load(): VisualizerState {
  try {
    const saved = JSON.parse(localStorage.getItem(STORE) ?? 'null') as Partial<VisualizerState> | null;
    // where it is shown and fullscreen belong to this run
    return { ...DEFAULT, ...(saved ?? {}), place: 'closed', fullscreen: false };
  } catch {
    return DEFAULT;
  }
}

let state = load();
const listeners = new Set<(state: VisualizerState) => void>();
const commandListeners = new Set<(command: VisualizerCommand) => void>();
const channel = typeof BroadcastChannel === 'undefined' ? null : new BroadcastChannel(CHANNEL);

function publish(next: VisualizerState, share: boolean): void {
  state = next;
  try {
    localStorage.setItem(STORE, JSON.stringify(state));
  } catch {
    // kept for this run only
  }
  if (share) channel?.postMessage({ kind: 'state', state } satisfies Message);
  listeners.forEach((listener) => listener(state));
}

if (channel) {
  channel.onmessage = (event: MessageEvent<Message>) => {
    const message = event.data;
    if (message.kind === 'state') publish(message.state, false);
    else if (message.kind === 'command') commandListeners.forEach((listener) => listener(message.command));
    else if (message.kind === 'ask') channel.postMessage({ kind: 'state', state } satisfies Message);
  };
  // a window opened later learns the state from the ones already open
  channel.postMessage({ kind: 'ask' } satisfies Message);
}

export function visualizer(): VisualizerState {
  return state;
}

export function setVisualizer(change: Partial<VisualizerState>): void {
  publish({ ...state, ...change }, true);
}

export function onVisualizer(listener: (state: VisualizerState) => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** Next or previous, done by whichever window draws. */
export function visualizerCommand(command: VisualizerCommand): void {
  commandListeners.forEach((listener) => listener(command));
  channel?.postMessage({ kind: 'command', command } satisfies Message);
}

export function onVisualizerCommand(listener: (command: VisualizerCommand) => void): () => void {
  commandListeners.add(listener);
  return () => commandListeners.delete(listener);
}
