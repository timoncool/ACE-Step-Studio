import { ArrayBufferTarget, Muxer } from 'mp4-muxer';

/**
 * Hardware H.264 encoding for the video studio.
 *
 * "Hardware" is not necessarily the graphics card: it is whatever
 * fixed-function encoder the machine has - NVENC on an NVIDIA GPU, Quick Sync
 * inside an Intel processor, VCE on AMD. The browser picks one; this only asks.
 *
 * The studio used to capture every frame as a base64 JPEG and hand the lot to
 * ffmpeg compiled to WebAssembly, which encodes on one CPU core - minutes for
 * a two-minute track. WebCodecs hands the same frames to the GPU encoder the
 * machine already has and muxes the result directly, so the encode costs
 * roughly what the drawing costs.
 *
 * It is a capability, not an assumption: if the browser has no VideoEncoder,
 * or refuses the configuration, the caller falls back to the WebAssembly path
 * and the export still works.
 */

/** Which encoder the export ended up on, for the caller to report. */
export type EncoderKind = 'hardware' | 'software';

export interface HardwareEncoder {
  /** Where the work is actually happening. */
  readonly kind: EncoderKind;
  /** Encodes one drawn frame, waiting if the encoder is behind. */
  encode(canvas: HTMLCanvasElement, frameIndex: number): Promise<void>;
  /** Finishes the file and returns silent MP4 bytes. */
  finish(): Promise<Uint8Array>;
  close(): void;
}

interface EncoderOptions {
  width: number;
  height: number;
  fps: number;
  /** Roughly 0.1 bits per pixel per frame at 1080p30 looks clean enough. */
  bitrate?: number;
}

/** A key frame every two seconds keeps seeking usable without bloating size. */
const KEYFRAME_SECONDS = 2;

/**
 * How many frames may sit in the encoder's queue.
 *
 * Frames handed to `encode` hold graphics memory until the encoder gets to
 * them; feeding a render loop faster than the encoder drains grows that queue
 * until the process runs out of video memory. The loop waits instead.
 */
const QUEUE_LIMIT = 12;

/**
 * Tried in order. Hardware encoders differ in the profiles they accept, and a
 * configuration this machine cannot run is better found here than three
 * minutes into an export.
 */
const CODECS = ['avc1.640028', 'avc1.4d0028', 'avc1.42e01e'];

export async function createHardwareEncoder(options: EncoderOptions): Promise<HardwareEncoder | null> {
  const VideoEncoderClass = (globalThis as { VideoEncoder?: typeof VideoEncoder }).VideoEncoder;
  if (!VideoEncoderClass || typeof VideoFrame === 'undefined') return null;

  const { width, height, fps } = options;
  // Dimensions must be even for 4:2:0 chroma; the canvas is not always.
  if (width % 2 !== 0 || height % 2 !== 0) return null;

  const bitrate = options.bitrate ?? Math.round(width * height * fps * 0.1);
  // Three steps down, in order: a fixed-function encoder wherever it lives,
  // graphics card or processor; then the browser's own software encoder; and
  // when neither configures, the caller's WebAssembly path.
  let config: VideoEncoderConfig | null = null;
  let kind: EncoderKind = 'hardware';
  for (const acceleration of ['prefer-hardware', 'no-preference'] as const) {
    for (const codec of CODECS) {
      const candidate: VideoEncoderConfig = {
        codec,
        width,
        height,
        framerate: fps,
        bitrate,
        hardwareAcceleration: acceleration,
        avc: { format: 'avc' },
      };
      const support = await VideoEncoderClass.isConfigSupported(candidate).catch(() => null);
      if (support?.supported) {
        config = support.config ?? candidate;
        kind = acceleration === 'prefer-hardware' ? 'hardware' : 'software';
        break;
      }
    }
    if (config) break;
  }
  if (!config) return null;

  const target = new ArrayBufferTarget();
  const muxer = new Muxer({
    target,
    video: { codec: 'avc', width, height, frameRate: fps },
    fastStart: 'in-memory',
  });

  let failure: unknown = null;
  const encoder = new VideoEncoderClass({
    output: (chunk, meta) => muxer.addVideoChunk(chunk, meta),
    error: (error) => { failure = error; },
  });
  encoder.configure(config);

  const keyFrameEvery = Math.max(1, Math.round(fps * KEYFRAME_SECONDS));

  return {
    kind,
    async encode(canvas, frameIndex) {
      if (failure) throw failure;
      // Wait rather than pile frames up in graphics memory.
      while (encoder.encodeQueueSize > QUEUE_LIMIT) {
        await new Promise((resolve) => setTimeout(resolve, 8));
        if (failure) throw failure;
      }
      // A VideoFrame holds GPU memory until closed; leaking them stalls the
      // encoder within a few seconds of footage.
      const frame = new VideoFrame(canvas, {
        timestamp: Math.round((frameIndex * 1_000_000) / fps),
        duration: Math.round(1_000_000 / fps),
      });
      try {
        encoder.encode(frame, { keyFrame: frameIndex % keyFrameEvery === 0 });
      } finally {
        frame.close();
      }
    },
    async finish() {
      await encoder.flush();
      if (failure) throw failure;
      muxer.finalize();
      encoder.close();
      return new Uint8Array(target.buffer);
    },
    close() {
      try { encoder.close(); } catch { /* already closed */ }
    },
  };
}

/** True when this machine can encode video without the WebAssembly fallback. */
export async function hardwareEncodingAvailable(width: number, height: number, fps: number): Promise<boolean> {
  const encoder = await createHardwareEncoder({ width, height, fps });
  if (!encoder) return false;
  encoder.close();
  return true;
}
