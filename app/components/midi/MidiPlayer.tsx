// MidiPlayer.tsx — live piano roll + synced playback. Copied from HOT-Step-CPP
// (scragnog/HOT-Step-CPP) ui/src/components/midi-studio/MidiPlayer.tsx; the notes
// come from the studio's service instead of an event stream.
//
// Plays the ORIGINAL track (audio element) and the transcribed MIDI (WebAudio
// synth) in sync, with an equal-power crossfade slider between them — hear
// either or both. In live mode it consumes the job's SSE event stream, so
// notes appear on the roll (and become playable) while transcription is still
// running; for finished jobs it loads notes.json and offers the same player.

import React, { useCallback, useEffect, useRef, useState } from 'react';
import { Pause, Play, Radio } from 'lucide-react';
import { useI18n } from '../../context/I18nContext';
import { MidiSynth, type PlayNote } from './midiSynth';

/** A note as the studio's service reports it, in seconds. */
export interface HeardNote {
  pitch: number;
  start: number;
  end: number;
  instrument: string;
}

const PPS = 40;          // px per second
const ROLL_H = 230;
const PITCH_MIN = 21, PITCH_MAX = 108;

function familyColor(family: string, alpha = 1): string {
  if (/drum/i.test(family)) return `hsla(0, 70%, 55%, ${alpha})`;
  let h = 0;
  for (let i = 0; i < family.length; i++) h = (h * 31 + family.charCodeAt(i)) % 360;
  return `hsla(${h}, 70%, 55%, ${alpha})`;
}

interface Props {
  notes: HeardNote[];
  sourceAudioUrl?: string;
  live: boolean;
  /** Pieces of the audio transcribed so far, while live. */
  chunks?: { done: number; total: number } | null;
}

export const MidiPlayer: React.FC<Props> = ({ notes, sourceAudioUrl, live, chunks }) => {
  const { t } = useI18n();
  const say = (key: string, values: Record<string, number> = {}) =>
    Object.entries(values).reduce((text, [name, value]) => text.replace(`{${name}}`, String(value)), (t as unknown as (key: string) => string)(key));

  // NOTE: parent must mount this component with a key unique per (job, mode)
  // — notes accumulate for the component's lifetime, no in-place reset.
  const notesRef = useRef<PlayNote[]>([]);
  const [noteCount, setNoteCount] = useState(0);      // mirrors notesRef length -> redraw
  const [redraw, setRedraw] = useState(0);            // bump from event handlers
  const [isPlaying, setIsPlaying] = useState(false);
  const [crossfade, setCrossfade] = useState(50);
  const [duration, setDuration] = useState(0);
  const [curTime, setCurTime] = useState(0);
  // what is not transcribed yet: the transcriber works in five-second pieces
  const frontier = live && chunks ? chunks.done * 5.0 : null;
  const [families, setFamilies] = useState<string[]>([]);
  const [muted, setMuted] = useState<Set<string>>(new Set());
  const [soloed, setSoloed] = useState<Set<string>>(new Set());

  const audioRef = useRef<HTMLAudioElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const synthRef = useRef<MidiSynth | null>(null);
  const lastManualScroll = useRef(0);
  const rafRef = useRef(0);

  // family audible = no solos active ? not muted : soloed
  const applyMixer = useCallback((synth: MidiSynth, fams: string[], mutedS: Set<string>, soloS: Set<string>) => {
    for (const f of fams) {
      synth.setFamilyAudible(f, soloS.size > 0 ? soloS.has(f) : !mutedS.has(f));
    }
  }, []);

  const getSynth = useCallback((): MidiSynth => {
    if (!synthRef.current) {
      const synth = new MidiSynth();
      // seed with everything that streamed in before first play — the synth
      // is created lazily on the first user gesture, so notes accumulated in
      // notesRef must be handed over here (this was the "MIDI side silent" bug)
      synth.setAllNotes(notesRef.current);
      synthRef.current = synth;
    }
    return synthRef.current;
  }, []);

  // keep the synth's family buses in sync with mute/solo state
  useEffect(() => {
    if (synthRef.current) applyMixer(synthRef.current, families, muted, soloed);
  }, [families, muted, soloed, applyMixer]);

  // ── data source: the notes the service has heard, replaced whole each time
  // (a note still sounding when a piece ends is closed by the next one)
  useEffect(() => {
    const all: PlayNote[] = notes.map(note => ({ pitch: note.pitch, start: note.start, duration: Math.max(0.03, note.end - note.start), family: note.instrument }));
    notesRef.current = all;
    synthRef.current?.setAllNotes(all);
    setFamilies(prev => {
      const seen = new Set(prev);
      let changed = false;
      for (const note of all) if (!seen.has(note.family)) { seen.add(note.family); changed = true; }
      return changed ? [...seen] : prev;
    });
    setNoteCount(all.length);
    setDuration(d => all.reduce((longest, note) => Math.max(longest, note.start + note.duration), d));
  }, [notes]);

  // ── audio element wiring (original track = master clock) ──
  useEffect(() => {
    const el = audioRef.current;
    if (!el || !sourceAudioUrl) return;
    const onMeta = () => setDuration(d => Math.max(d, el.duration || 0));
    const onEnd = () => setIsPlaying(false);
    el.addEventListener('loadedmetadata', onMeta);
    el.addEventListener('ended', onEnd);
    return () => { el.removeEventListener('loadedmetadata', onMeta); el.removeEventListener('ended', onEnd); };
  }, [sourceAudioUrl]);

  useEffect(() => () => { synthRef.current?.dispose(); synthRef.current = null; }, []);

  const togglePlay = useCallback(async () => {
    const el = audioRef.current;
    if (!el || !sourceAudioUrl) return;
    const synth = getSynth();
    synth.attachAudio(el);
    synth.setCrossfade(crossfade / 100);
    applyMixer(synth, families, muted, soloed);
    if (synth.playing) {
      synth.pause();
      setIsPlaying(false);
    } else {
      await synth.play();
      setIsPlaying(true);
    }
  }, [sourceAudioUrl, crossfade, getSynth, applyMixer, families, muted, soloed]);

  const toggleMute = useCallback((f: string) => {
    setMuted(prev => {
      const s = new Set(prev);
      if (s.has(f)) s.delete(f); else s.add(f);
      return s;
    });
  }, []);
  const toggleSolo = useCallback((f: string) => {
    setSoloed(prev => {
      const s = new Set(prev);
      if (s.has(f)) s.delete(f); else s.add(f);
      return s;
    });
  }, []);

  const handleCrossfade = useCallback((v: number) => {
    setCrossfade(v);
    synthRef.current?.setCrossfade(v / 100);
  }, []);

  const seekTo = useCallback((time: number) => {
    const el = audioRef.current;
    if (!el) return;
    if (synthRef.current) synthRef.current.seek(time);
    else el.currentTime = time;
    setCurTime(time);
    setRedraw(r => r + 1);
  }, []);

  // ── canvas drawing + playhead follow ──
  const draw = useCallback(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const w = canvas.width, h = canvas.height;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    const dark = document.documentElement.classList.contains('dark');
    ctx.clearRect(0, 0, w, h);

    const rowH = h / (PITCH_MAX - PITCH_MIN + 1);
    // 10 s gridlines
    ctx.strokeStyle = dark ? 'rgba(255,255,255,0.07)' : 'rgba(0,0,0,0.07)';
    ctx.fillStyle = dark ? 'rgba(255,255,255,0.35)' : 'rgba(0,0,0,0.35)';
    ctx.font = '9px sans-serif';
    for (let s = 0; s * PPS < w; s += 10) {
      const x = s * PPS;
      ctx.beginPath(); ctx.moveTo(x, 0); ctx.lineTo(x, h); ctx.stroke();
      ctx.fillText(`${s}s`, x + 3, 10);
    }

    for (const n of notesRef.current) {
      const p = Math.min(PITCH_MAX, Math.max(PITCH_MIN, n.pitch));
      ctx.fillStyle = familyColor(n.family, 0.85);
      ctx.fillRect(n.start * PPS, (PITCH_MAX - p) * rowH, Math.max(1.5, n.duration * PPS), Math.max(1.5, rowH - 0.5));
    }

    // un-transcribed region (live)
    if (frontier !== null && duration > 0) {
      const x = frontier * PPS;
      ctx.fillStyle = dark ? 'rgba(255,255,255,0.05)' : 'rgba(0,0,0,0.05)';
      ctx.fillRect(x, 0, w - x, h);
    }

    // playhead
    const t = synthRef.current?.currentTime ?? curTime;
    ctx.strokeStyle = 'rgba(236,72,153,0.9)';
    ctx.lineWidth = 1.5;
    ctx.beginPath(); ctx.moveTo(t * PPS, 0); ctx.lineTo(t * PPS, h); ctx.stroke();
  }, [frontier, duration, curTime]);

  // resize canvas to content and redraw on data changes
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const wantW = Math.max(600, Math.ceil(Math.max(duration, frontier ?? 0, 10) * PPS) + 40);
    if (canvas.width !== wantW) { canvas.width = wantW; canvas.height = ROLL_H; }
    draw();
  }, [noteCount, redraw, duration, frontier, draw]);

  // animation loop while playing: playhead + auto-follow
  useEffect(() => {
    if (!isPlaying) { draw(); return; }
    const loop = () => {
      const tNow = synthRef.current?.currentTime ?? 0;
      setCurTime(tNow);
      draw();
      const sc = scrollRef.current;
      if (sc && Date.now() - lastManualScroll.current > 2500) {
        const target = tNow * PPS - sc.clientWidth * 0.4;
        sc.scrollLeft = Math.max(0, target);
      }
      rafRef.current = requestAnimationFrame(loop);
    };
    rafRef.current = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(rafRef.current);
  }, [isPlaying, draw]);

  const fmt = (s: number) => `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, '0')}`;

  return (
    <div className="mt-3">
      {sourceAudioUrl && <audio ref={audioRef} src={sourceAudioUrl} preload="metadata" />}

      {/* transport */}
      <div className="flex items-center gap-3 flex-wrap mb-2">
        <button
          onClick={togglePlay}
          disabled={!sourceAudioUrl}
          className="w-9 h-9 rounded-full bg-pink-600 hover:bg-pink-500 text-white flex items-center justify-center disabled:opacity-40 transition-colors"
          title={isPlaying ? say('midiPause') : say('midiPlay')}
        >
          {isPlaying ? <Pause size={16} /> : <Play size={16} className="ml-0.5" />}
        </button>
        <span className="text-[11px] tabular-nums text-zinc-500 dark:text-zinc-400 w-[86px]">
          {fmt(curTime)} / {fmt(duration)}
        </span>

        {/* crossfade: original <-> MIDI */}
        <div className="flex items-center gap-2">
          <span className={`text-[11px] ${crossfade < 50 ? 'font-semibold text-zinc-800 dark:text-zinc-200' : 'text-zinc-400 dark:text-zinc-500'}`}>
            {say('midiOriginal')}
          </span>
          <input
            type="range" min={0} max={100} value={crossfade}
            onChange={e => handleCrossfade(Number(e.target.value))}
            className="w-36 accent-pink-500"
            title={say('midiCrossfade')}
          />
          <span className={`text-[11px] ${crossfade > 50 ? 'font-semibold text-zinc-800 dark:text-zinc-200' : 'text-zinc-400 dark:text-zinc-500'}`}>
            MIDI
          </span>
        </div>

        {live && chunks && (
          <span className="flex items-center gap-1.5 text-[11px] text-pink-500 dark:text-pink-400">
            <Radio size={11} className="animate-pulse" />
            {say('midiLiveChunk', { done: chunks.done, total: chunks.total })}
          </span>
        )}
        <span className="text-[11px] text-zinc-400 dark:text-zinc-500">
          {say('midiNoteCount', { count: noteCount })}
        </span>
      </div>

      {/* legend + per-track mute/solo (affects the MIDI side only) */}
      {families.length > 0 && (
        <div className="flex flex-wrap gap-x-2 gap-y-1 mb-1.5">
          {families.map(f => {
            const isMuted = muted.has(f);
            const isSolo = soloed.has(f);
            const audible = soloed.size > 0 ? isSolo : !isMuted;
            return (
              <span
                key={f}
                className={`flex items-center gap-1.5 text-[11px] rounded-md px-1.5 py-0.5 border transition-colors ${
                  isSolo
                    ? 'border-pink-500/60 bg-pink-500/10 text-zinc-800 dark:text-zinc-200'
                    : 'border-transparent text-zinc-600 dark:text-zinc-400'
                } ${audible ? '' : 'opacity-45'}`}
              >
                <span className="w-2.5 h-2.5 rounded-sm inline-block flex-shrink-0" style={{ backgroundColor: familyColor(f) }} />
                <span className={isMuted && soloed.size === 0 ? 'line-through' : ''}>{f.replace(/_/g, ' ')}</span>
                <button
                  onClick={() => toggleMute(f)}
                  title={say('midiMute')}
                  className={`w-4 h-4 rounded text-[9px] font-bold leading-none flex items-center justify-center transition-colors ${
                    isMuted ? 'bg-red-500 text-white' : 'bg-zinc-200 dark:bg-white/10 text-zinc-500 hover:bg-zinc-300 dark:hover:bg-white/20'
                  }`}
                >M</button>
                <button
                  onClick={() => toggleSolo(f)}
                  title={say('midiSolo')}
                  className={`w-4 h-4 rounded text-[9px] font-bold leading-none flex items-center justify-center transition-colors ${
                    isSolo ? 'bg-amber-500 text-white' : 'bg-zinc-200 dark:bg-white/10 text-zinc-500 hover:bg-zinc-300 dark:hover:bg-white/20'
                  }`}
                >S</button>
              </span>
            );
          })}
        </div>
      )}

      {/* piano roll (click to seek) */}
      <div
        ref={scrollRef}
        onScroll={() => { lastManualScroll.current = Date.now(); }}
        className="overflow-x-auto rounded-lg border border-zinc-200 dark:border-white/5 bg-zinc-50 dark:bg-black/30"
      >
        <canvas
          ref={canvasRef}
          height={ROLL_H}
          className="block cursor-pointer"
          style={{ height: ROLL_H }}
          onClick={e => {
            const rect = (e.target as HTMLCanvasElement).getBoundingClientRect();
            seekTo((e.clientX - rect.left) / PPS);
          }}
        />
      </div>
    </div>
  );
};

export default MidiPlayer;
