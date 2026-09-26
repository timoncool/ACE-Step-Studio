// Copies what the player sounds like, after the equalizer, in blocks of 2048
// stereo frames, so a visualiser in another window can hear it too.
class PcmTap extends AudioWorkletProcessor {
  constructor() {
    super();
    this.size = 2048;
    this.left = new Float32Array(this.size);
    this.right = new Float32Array(this.size);
    this.filled = 0;
  }

  process(inputs) {
    const input = inputs[0];
    if (!input || input.length === 0) return true;
    const left = input[0];
    const right = input[1] || input[0];
    let offset = 0;
    while (offset < left.length) {
      const take = Math.min(left.length - offset, this.size - this.filled);
      this.left.set(left.subarray(offset, offset + take), this.filled);
      this.right.set(right.subarray(offset, offset + take), this.filled);
      this.filled += take;
      offset += take;
      if (this.filled === this.size) {
        this.port.postMessage({ left: this.left, right: this.right }, [this.left.buffer, this.right.buffer]);
        this.left = new Float32Array(this.size);
        this.right = new Float32Array(this.size);
        this.filled = 0;
      }
    }
    return true;
  }
}

registerProcessor('pcm-tap', PcmTap);
