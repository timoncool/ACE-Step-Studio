import { afterEach, describe, expect, it, vi } from 'vitest';
import { MidiSynth } from './midiSynth';

class FakeParam {
  value = 0;
  setValueAtTime() { return this; }
  linearRampToValueAtTime() { return this; }
  setTargetAtTime() { return this; }
  cancelScheduledValues() { return this; }
}

class FakeContext {
  currentTime = 0;
  sampleRate = 8000;
  destination = {};
  async resume() {}
  close() {}
  createDynamicsCompressor() { return { threshold: new FakeParam(), ratio: new FakeParam(), connect() {} }; }
  createGain() { return { gain: new FakeParam(), connect() {}, disconnect() {} }; }
  createBuffer(_channels: number, length: number) { return { getChannelData: () => new Float32Array(length) }; }
}

describe('MidiSynth.attachAudio', () => {
  afterEach(() => vi.unstubAllGlobals());

  it('routes every audio element it is given through the crossfade, once per element', () => {
    const made: { el: unknown; connected: number; disconnected: number }[] = [];
    class WithMedia extends FakeContext {
      createMediaElementSource(el: unknown) {
        const node = { el, connected: 0, disconnected: 0, connect() { node.connected++; }, disconnect() { node.disconnected++; } };
        made.push(node);
        return node;
      }
    }
    vi.stubGlobal('AudioContext', WithMedia);
    const synth = new MidiSynth();
    const first = document.createElement('audio');
    const second = document.createElement('audio');
    synth.attachAudio(first);
    synth.attachAudio(second);
    expect(made.map(node => node.el)).toEqual([first, second]);
    expect(made[1].connected).toBe(1);
    expect(made[0].disconnected).toBe(1);
    // back to the first element: its own node again, never a second one for it
    synth.attachAudio(first);
    expect(made).toHaveLength(2);
    expect(made[0].connected).toBe(2);
  });
});
