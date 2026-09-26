// Plays the blocks another window's tap sends, so this window's visualisers
// have a source. Kept at most a quarter of a second behind: older blocks are
// dropped rather than let the picture fall behind the sound.
class PcmPlayer extends AudioWorkletProcessor {
  constructor() {
    super();
    this.queue = [];
    this.current = null;
    this.offset = 0;
    this.port.onmessage = (event) => {
      this.queue.push(event.data);
      const limit = Math.ceil((sampleRate / 4) / event.data.left.length);
      while (this.queue.length > limit) this.queue.shift();
    };
  }

  process(_inputs, outputs) {
    const output = outputs[0];
    const left = output[0];
    const right = output[1] || output[0];
    let written = 0;
    while (written < left.length) {
      if (!this.current) {
        this.current = this.queue.shift() || null;
        this.offset = 0;
        if (!this.current) {
          left.fill(0, written);
          right.fill(0, written);
          return true;
        }
      }
      const take = Math.min(left.length - written, this.current.left.length - this.offset);
      left.set(this.current.left.subarray(this.offset, this.offset + take), written);
      right.set(this.current.right.subarray(this.offset, this.offset + take), written);
      written += take;
      this.offset += take;
      if (this.offset >= this.current.left.length) this.current = null;
    }
    return true;
  }
}

registerProcessor('pcm-player', PcmPlayer);
