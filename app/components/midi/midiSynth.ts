// Copied from HOT-Step-CPP (scragnog/HOT-Step-CPP) ui/src/components/midi-studio/midiSynth.ts.
// midiSynth.ts — WebAudio playback engine for MIDI Studio
//
// Plays transcribed notes with simple per-family synth voices, in sync with
// the original audio track (an <audio> element is the master clock), with an
// equal-power crossfade between the two. Notes can be appended while playing
// (live transcription streaming): a lookahead scheduler picks them up.

export interface PlayNote {
  pitch: number;      // MIDI 0-127
  start: number;      // seconds
  duration: number;   // seconds
  family: string;     // instrument family key ('drums' special-cased)
}

const LOOKAHEAD_S = 0.35;   // schedule window
const TICK_MS = 120;        // scheduler tick
const VOICE_GAIN = 0.10;

interface FamilyVoice {
  type: OscillatorType;
  attack: number;
  release: number;
  sustain: number;    // sustain level fraction of peak
  detune?: number;    // slight second-osc detune (cents) for width
}

function voiceForFamily(family: string): FamilyVoice {
  const f = family.toLowerCase();
  if (/bass|tuba|contrabass/.test(f)) return { type: 'sine', attack: 0.01, release: 0.12, sustain: 0.8 };
  if (/piano|chromatic|harp|timpani/.test(f)) return { type: 'triangle', attack: 0.005, release: 0.25, sustain: 0.35 };
  if (/guitar/.test(f)) return { type: 'sawtooth', attack: 0.005, release: 0.2, sustain: 0.4 };
  if (/organ/.test(f)) return { type: 'square', attack: 0.02, release: 0.08, sustain: 0.9 };
  if (/string|voice|pad|ensemble|orchestra/.test(f)) return { type: 'sawtooth', attack: 0.08, release: 0.25, sustain: 0.85, detune: 8 };
  if (/brass|trumpet|trombone|horn|sax|reed|oboe|bassoon|clarinet|english/.test(f)) return { type: 'sawtooth', attack: 0.04, release: 0.15, sustain: 0.8 };
  if (/flute|pipe/.test(f)) return { type: 'sine', attack: 0.05, release: 0.15, sustain: 0.85 };
  if (/synth_lead|lead/.test(f)) return { type: 'square', attack: 0.01, release: 0.12, sustain: 0.7 };
  return { type: 'triangle', attack: 0.01, release: 0.15, sustain: 0.6 };
}

function midiToHz(pitch: number): number {
  return 440 * Math.pow(2, (pitch - 69) / 12);
}

export class MidiSynth {
  private ctx: AudioContext;
  private midiGain: GainNode;
  private origGain: GainNode;
  private noiseBuf: AudioBuffer;
  private audioEl: HTMLAudioElement | null = null;
  private mediaSrc: MediaElementAudioSourceNode | null = null;

  private notes: PlayNote[] = [];       // kept sorted by start
  private schedIdx = 0;                 // next note to consider scheduling
  private timer: number | null = null;
  private active: Set<{ stop: (t: number) => void }> = new Set();
  private crossfade = 0.5;

  // per-instrument-family sub-mix (mute/solo support)
  private familyGains = new Map<string, GainNode>();
  private familyAudible = new Map<string, boolean>();

  constructor() {
    this.ctx = new AudioContext();
    const comp = this.ctx.createDynamicsCompressor();
    comp.threshold.value = -18;
    comp.ratio.value = 6;
    comp.connect(this.ctx.destination);
    this.midiGain = this.ctx.createGain();
    this.midiGain.connect(comp);
    this.origGain = this.ctx.createGain();
    this.origGain.connect(comp);
    this.setCrossfade(0.5);

    // shared white-noise buffer for drum voices
    this.noiseBuf = this.ctx.createBuffer(1, this.ctx.sampleRate / 2, this.ctx.sampleRate);
    const d = this.noiseBuf.getChannelData(0);
    for (let i = 0; i < d.length; i++) d[i] = Math.random() * 2 - 1;
  }

  /** Wire the original-audio element into the crossfade graph (once). */
  attachAudio(el: HTMLAudioElement): void {
    if (this.audioEl === el) return;
    this.audioEl = el;
    if (!this.mediaSrc) {
      this.mediaSrc = this.ctx.createMediaElementSource(el);
      this.mediaSrc.connect(this.origGain);
    }
  }

  /** Per-family output bus — lets the UI mute/solo instrument tracks. */
  private gainForFamily(family: string): GainNode {
    let g = this.familyGains.get(family);
    if (!g) {
      g = this.ctx.createGain();
      g.gain.value = this.familyAudible.get(family) === false ? 0 : 1;
      g.connect(this.midiGain);
      this.familyGains.set(family, g);
    }
    return g;
  }

  /** Set a family's audibility (the UI computes mute/solo into a boolean). */
  setFamilyAudible(family: string, audible: boolean): void {
    this.familyAudible.set(family, audible);
    const g = this.familyGains.get(family);
    if (g) g.gain.setTargetAtTime(audible ? 1 : 0, this.ctx.currentTime, 0.02);
  }

  /** 0 = original only, 1 = MIDI only; equal-power blend in between. */
  setCrossfade(v: number): void {
    this.crossfade = Math.min(1, Math.max(0, v));
    const t = this.ctx.currentTime;
    this.origGain.gain.setTargetAtTime(Math.cos(this.crossfade * Math.PI / 2), t, 0.02);
    this.midiGain.gain.setTargetAtTime(Math.sin(this.crossfade * Math.PI / 2), t, 0.02);
  }
  getCrossfade(): number { return this.crossfade; }

  /** Append notes (live streaming). Keeps the schedule order consistent. */
  addNotes(notes: PlayNote[]): void {
    if (!notes.length) return;
    this.notes.push(...notes);
    // note_end events can arrive slightly out of start order — sort from the
    // unscheduled tail only, so already-played history is untouched
    const tail = this.notes.slice(this.schedIdx).sort((a, b) => a.start - b.start);
    this.notes = this.notes.slice(0, this.schedIdx).concat(tail);
  }

  setAllNotes(notes: PlayNote[]): void {
    this.notes = [...notes].sort((a, b) => a.start - b.start);
    this.resync();
  }

  get currentTime(): number { return this.audioEl?.currentTime ?? 0; }
  get playing(): boolean { return !!this.audioEl && !this.audioEl.paused; }

  async play(): Promise<void> {
    if (!this.audioEl) return;
    await this.ctx.resume();
    this.resync();
    await this.audioEl.play();
    if (this.timer === null) {
      this.timer = window.setInterval(() => this.tick(), TICK_MS);
    }
  }

  pause(): void {
    this.audioEl?.pause();
    this.stopScheduled();
    if (this.timer !== null) { window.clearInterval(this.timer); this.timer = null; }
  }

  seek(t: number): void {
    if (!this.audioEl) return;
    this.audioEl.currentTime = Math.max(0, t);
    this.stopScheduled();
    this.resync();
  }

  dispose(): void {
    this.pause();
    try { this.ctx.close(); } catch { /* already closed */ }
  }

  // ── internals ──────────────────────────────────────────────────────────

  private resync(): void {
    const t = this.currentTime;
    this.schedIdx = 0;
    // binary search would be nicer; linear is fine at 10k notes on seek only
    while (this.schedIdx < this.notes.length && this.notes[this.schedIdx].start < t - 0.05) this.schedIdx++;
  }

  private stopScheduled(): void {
    const t = this.ctx.currentTime;
    for (const v of this.active) v.stop(t);
    this.active.clear();
  }

  private tick(): void {
    if (!this.audioEl || this.audioEl.paused) return;
    const trackTime = this.audioEl.currentTime;
    const horizon = trackTime + LOOKAHEAD_S;
    // map track seconds -> ctx seconds (recomputed every tick: absorbs drift)
    const ctxBase = this.ctx.currentTime - trackTime;

    while (this.schedIdx < this.notes.length && this.notes[this.schedIdx].start < horizon) {
      const n = this.notes[this.schedIdx++];
      if (n.start < trackTime - 0.05) continue;  // stale (seek/underrun)
      const when = ctxBase + n.start;
      if (n.family === 'drums' || n.family === 'Drums') this.playDrum(n, when);
      else this.playTone(n, when);
    }
  }

  private playTone(n: PlayNote, when: number): void {
    const v = voiceForFamily(n.family);
    const dur = Math.max(0.04, Math.min(n.duration, 12));
    const g = this.ctx.createGain();
    g.connect(this.gainForFamily(n.family));
    g.gain.setValueAtTime(0, when);
    g.gain.linearRampToValueAtTime(VOICE_GAIN, when + v.attack);
    g.gain.setTargetAtTime(VOICE_GAIN * v.sustain, when + v.attack, 0.08);
    const end = when + dur;
    g.gain.setTargetAtTime(0, end, v.release / 3);

    const oscs: OscillatorNode[] = [];
    const mk = (detune: number) => {
      const o = this.ctx.createOscillator();
      o.type = v.type;
      o.frequency.value = midiToHz(n.pitch);
      o.detune.value = detune;
      o.connect(g);
      o.start(when);
      o.stop(end + v.release * 4);
      oscs.push(o);
    };
    mk(0);
    if (v.detune) mk(v.detune);

    const voice = { stop: (t: number) => { try { g.gain.cancelScheduledValues(t); g.gain.setTargetAtTime(0, t, 0.01); oscs.forEach(o => o.stop(t + 0.05)); } catch { /* ended */ } } };
    this.active.add(voice);
    oscs[0].onended = () => this.active.delete(voice);
  }

  private playDrum(n: PlayNote, when: number): void {
    const p = n.pitch;
    const g = this.ctx.createGain();
    g.connect(this.gainForFamily(n.family));

    if (p === 35 || p === 36) {
      // kick: sine pitch-drop thump
      const o = this.ctx.createOscillator();
      o.type = 'sine';
      o.frequency.setValueAtTime(120, when);
      o.frequency.exponentialRampToValueAtTime(45, when + 0.09);
      g.gain.setValueAtTime(VOICE_GAIN * 2.2, when);
      g.gain.setTargetAtTime(0, when + 0.02, 0.05);
      o.connect(g);
      o.start(when);
      o.stop(when + 0.3);
      return;
    }
    // snare / hats / percussion: filtered noise burst
    const src = this.ctx.createBufferSource();
    src.buffer = this.noiseBuf;
    const filt = this.ctx.createBiquadFilter();
    const isHat = p === 42 || p === 44 || p === 46;
    const isSnare = p === 38 || p === 40;
    filt.type = isHat ? 'highpass' : 'bandpass';
    filt.frequency.value = isHat ? 7000 : isSnare ? 2200 : 900 + (p % 12) * 220;
    filt.Q.value = isHat ? 0.8 : 1.2;
    const len = isHat ? 0.05 : isSnare ? 0.14 : 0.1;
    g.gain.setValueAtTime(VOICE_GAIN * (isHat ? 1.0 : 1.7), when);
    g.gain.setTargetAtTime(0, when + 0.005, len / 3);
    src.connect(filt);
    filt.connect(g);
    src.start(when);
    src.stop(when + len + 0.1);
  }
}
