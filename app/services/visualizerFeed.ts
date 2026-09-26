import { ensureAudioGraph, onAudioGraph, type AudioGraph } from './audioGraph';

/**
 * The sound a visualiser in its own window hears. The studio window taps the
 * player after the equalizer and sends the samples over a BroadcastChannel;
 * the visualiser window plays them into a silent graph of its own, which its
 * visualisers read exactly as they read the player in the studio window.
 * Windows of one app share an origin, which is all the channel needs.
 */

const CHANNEL = 'studio:visualizer-feed';

type Message = { kind: 'hello' } | { kind: 'bye' } | { kind: 'pcm'; left: Float32Array; right: Float32Array };

/** Started by the studio window: sends while a visualiser window listens. */
export function serveVisualizerFeed(): () => void {
  const channel = new BroadcastChannel(CHANNEL);
  let tap: AudioWorkletNode | null = null;
  let sink: GainNode | null = null;
  let listening = false;
  let graph: AudioGraph | null = null;

  let starting: Promise<void> | null = null;
  const start = () => {
    starting ??= begin().finally(() => {
      starting = null;
    });
    return starting;
  };
  const begin = async () => {
    if (!graph || tap) return;
    await graph.context.audioWorklet.addModule('/worklets/pcm-tap.js');
    tap = new AudioWorkletNode(graph.context, 'pcm-tap', { numberOfInputs: 1, numberOfOutputs: 1, outputChannelCount: [2] });
    // a node nothing pulls on is never processed: it goes on, silently, to the speakers
    sink = graph.context.createGain();
    sink.gain.value = 0;
    graph.output.connect(tap);
    tap.connect(sink).connect(graph.context.destination);
    tap.port.onmessage = (event) => channel.postMessage({ kind: 'pcm', left: event.data.left, right: event.data.right } satisfies Message);
  };
  const stop = () => {
    tap?.disconnect();
    sink?.disconnect();
    if (graph && tap) graph.output.disconnect(tap);
    tap = null;
    sink = null;
  };

  const unsubscribe = onAudioGraph((ready) => {
    graph = ready;
    if (listening) void start();
  });
  channel.onmessage = (event: MessageEvent<Message>) => {
    if (event.data.kind === 'hello') {
      listening = true;
      // the window asking is heard through the graph, made now if it was not
      ensureAudioGraph();
      void start();
    } else if (event.data.kind === 'bye') {
      listening = false;
      stop();
    }
  };
  return () => {
    unsubscribe();
    stop();
    channel.close();
  };
}

export interface ReceivedFeed {
  context: AudioContext;
  /** Where the studio's sound comes out in this window. */
  source: AudioNode;
  close: () => void;
}

/** Opened by the visualiser window. */
export async function receiveVisualizerFeed(): Promise<ReceivedFeed> {
  const context = new AudioContext();
  await context.audioWorklet.addModule('/worklets/pcm-player.js');
  const player = new AudioWorkletNode(context, 'pcm-player', { numberOfInputs: 0, numberOfOutputs: 1, outputChannelCount: [2] });
  const mute = context.createGain();
  mute.gain.value = 0;
  player.connect(mute).connect(context.destination);
  const channel = new BroadcastChannel(CHANNEL);
  let heard = 0;
  channel.onmessage = (event: MessageEvent<Message>) => {
    if (event.data.kind !== 'pcm') return;
    heard = Date.now();
    player.port.postMessage({ left: event.data.left, right: event.data.right });
  };
  // said again while nothing arrives: the studio window may have come up, or
  // made its graph, after this one asked
  const hello = () => channel.postMessage({ kind: 'hello' } satisfies Message);
  hello();
  const asking = window.setInterval(() => {
    if (Date.now() - heard > 1500) hello();
  }, 1000);
  const bye = () => channel.postMessage({ kind: 'bye' } satisfies Message);
  window.addEventListener('beforeunload', bye);
  return {
    context,
    source: player,
    close: () => {
      window.clearInterval(asking);
      bye();
      window.removeEventListener('beforeunload', bye);
      channel.close();
      void context.close();
    },
  };
}
