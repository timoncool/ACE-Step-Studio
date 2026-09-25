import React, { useCallback, useEffect, useRef, useState } from 'react';
import { Download, Loader2, Piano, Play, Square, Trash2 } from 'lucide-react';
import { useI18n } from '../../context/I18nContext';
import { apiUrl } from '../../services/apiBase';
import { MidiPlayer, type HeardNote } from './MidiPlayer';

/**
 * Audio to MIDI on the tools page, for the track chosen there: a song, a
 * stem, a processed take. The transcriber and its weights are optional; the
 * first transcription fetches them, or the user does beforehand.
 */

interface MidiSize {
  id: 'small' | 'medium' | 'large';
  params: string;
  bytes: number;
  installed: boolean;
  missing_bytes: number;
}

interface MidiRun {
  song_id: string | null;
  size: string;
  stage: 'preparing' | 'downloading' | 'reading' | 'transcribing' | 'done';
  chunks_done: number;
  chunks_total: number;
  notes: number;
  done: boolean;
  error: string | null;
  file: string | null;
  progress: number;
}

interface MidiStatus {
  tool_installed: boolean;
  sizes: MidiSize[];
  default_size: MidiSize['id'];
  download: { downloaded_bytes: number; total_bytes: number; done: boolean; error: string | null } | null;
  run: MidiRun | null;
  license: string;
}

interface SongMidi {
  file: string;
  size: string | null;
  instruments: string[];
  notes: HeardNote[];
}

const gigabytes = (bytes: number) => (bytes >= 1e9 ? `${(bytes / 1e9).toFixed(1)} GB` : `${Math.max(1, Math.round(bytes / 1e6))} MB`);

export const MidiTool: React.FC<{ songId: string; songTitle: string; card: string }> = ({ songId, songTitle, card }) => {
  const { t } = useI18n();
  const tt = t as unknown as (key: string) => string;
  const [status, setStatus] = useState<MidiStatus | null>(null);
  const [size, setSize] = useState<MidiSize['id'] | null>(null);
  const [songMidi, setSongMidi] = useState<SongMidi | null>(null);
  const [liveNotes, setLiveNotes] = useState<HeardNote[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const cardRef = useRef<HTMLElement | null>(null);

  const run = status?.run?.song_id === songId ? status.run : null;
  const running = Boolean(run && !run.done);
  const chosen = size ?? status?.default_size ?? 'medium';
  const chosenSize = status?.sizes.find(entry => entry.id === chosen);

  const loadSongMidi = useCallback(async () => {
    if (!songId) return setSongMidi(null);
    const response = await fetch(`/v1/library/songs/${encodeURIComponent(songId)}/midi`);
    setSongMidi(response.ok ? await response.json() : null);
  }, [songId]);

  useEffect(() => {
    void loadSongMidi().catch(() => setSongMidi(null));
  }, [loadSongMidi]);

  // the state, and while a run of this track is at work, the notes it has heard
  useEffect(() => {
    let alive = true;
    let wasRunning = false;
    const read = async () => {
      const next: MidiStatus = await fetch('/v1/midi').then(response => response.json());
      if (!alive) return;
      setStatus(next);
      const mine = next.run?.song_id === songId ? next.run : null;
      if (mine && !mine.done && mine.stage === 'transcribing') {
        const body = await fetch('/v1/midi/notes').then(response => response.json());
        if (alive) setLiveNotes(body.notes ?? []);
      }
      if (wasRunning && mine?.done) void loadSongMidi();
      wasRunning = Boolean(mine && !mine.done);
    };
    void read().catch(() => undefined);
    const timer = window.setInterval(() => void read().catch(() => undefined), 1000);
    return () => {
      alive = false;
      window.clearInterval(timer);
    };
  }, [songId, loadSongMidi]);

  // the track menu's "To MIDI" lands here
  useEffect(() => {
    const focus = () => cardRef.current?.scrollIntoView({ behavior: 'smooth', block: 'start' });
    window.addEventListener('studio:focus-midi', focus);
    return () => window.removeEventListener('studio:focus-midi', focus);
  }, []);

  const post = async (path: string, body: object) => {
    setBusy(true);
    setError(null);
    try {
      const response = await fetch(path, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
      if (!response.ok) {
        const problem = await response.json().catch(() => null);
        throw new Error(problem?.error || `HTTP ${response.status}`);
      }
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setBusy(false);
    }
  };

  const transcribe = () => {
    setLiveNotes([]);
    void post('/v1/midi/transcribe', { song_id: songId, size: chosen });
  };

  const remove = async () => {
    const response = await fetch(`/v1/library/songs/${encodeURIComponent(songId)}/midi`, { method: 'DELETE' });
    if (response.ok) setSongMidi(null);
  };

  const download = status?.download && !status.download.done ? status.download : null;
  const percent = run?.stage === 'downloading' && download
    ? Math.round((download.downloaded_bytes / Math.max(1, download.total_bytes)) * 100)
    : Math.round((run?.progress ?? 0) * 100);
  const audioUrl = songId ? apiUrl(`/v1/library/media/${encodeURIComponent(songId)}`) : undefined;
  const missing = chosenSize && (!status?.tool_installed || !chosenSize.installed) ? chosenSize.missing_bytes : 0;

  return (
    <section ref={cardRef} className={card}>
      <div className="flex items-center gap-2 text-sm font-semibold text-zinc-900 dark:text-white">
        <Piano size={17} className="text-pink-500" /> {t('midiTitle')}
      </div>
      <p className="mt-2 text-sm text-zinc-600 dark:text-zinc-300">{t('midiHint')}</p>

      <div className="mt-3">
        <p className="text-[11px] font-bold uppercase tracking-wide text-zinc-500">{t('midiModel')}</p>
        <div className="mt-2 flex gap-2">
          {(status?.sizes ?? []).map(entry => (
            <button
              key={entry.id}
              type="button"
              onClick={() => setSize(entry.id)}
              className={`flex-1 rounded-lg border px-3 py-2 text-left text-xs ${
                entry.id === chosen ? 'border-pink-400 bg-pink-500/10 text-zinc-900 dark:text-white' : 'border-zinc-200 text-zinc-500 dark:border-white/10'
              }`}
            >
              <span className="block font-semibold">{tt(`midiSize_${entry.id}`)} · {entry.params}</span>
              <span className="block text-[11px] text-zinc-500">{entry.installed ? t('midiInstalled') : gigabytes(entry.bytes)}</span>
            </button>
          ))}
        </div>
        {missing > 0 && !running && (
          <p className="mt-2 text-[11px] leading-4 text-zinc-500">
            {t('midiFirstUse').replace('{size}', gigabytes(missing))}{' '}
            <button type="button" onClick={() => void post('/v1/midi/install', { size: chosen })} disabled={busy || Boolean(download)} className="font-semibold text-pink-600 hover:underline disabled:opacity-50 dark:text-pink-400">
              {t('midiInstallNow')}
            </button>
          </p>
        )}
        {download && !running && (
          <p className="mt-2 text-[11px] tabular-nums text-zinc-500">
            {t('midiDownloading')} {gigabytes(download.downloaded_bytes)} / {gigabytes(download.total_bytes)}
          </p>
        )}
      </div>

      <div className="mt-3 flex flex-wrap gap-2">
        <button
          type="button"
          onClick={transcribe}
          disabled={!songId || running || busy}
          className="inline-flex items-center gap-2 rounded-lg bg-gradient-to-r from-orange-500 to-pink-600 px-4 py-2 text-sm font-bold text-white disabled:cursor-not-allowed disabled:opacity-50"
        >
          {running ? <Loader2 size={14} className="animate-spin" /> : <Play size={14} />}
          {songMidi ? t('midiAgain') : t('midiStart')}
        </button>
        {running && (
          <button type="button" onClick={() => void post('/v1/midi/cancel', {})} className="inline-flex items-center gap-2 rounded-lg border border-zinc-300 px-3 py-2 text-xs font-semibold text-zinc-700 hover:border-pink-400 dark:border-white/15 dark:text-zinc-200">
            <Square size={12} /> {t('midiStop')}
          </button>
        )}
        {songMidi && !running && (
          <>
            <a
              href={apiUrl(`/v1/library/songs/${encodeURIComponent(songId)}/midi/file`)}
              download={`${songTitle || 'track'}.mid`}
              className="inline-flex items-center gap-2 rounded-lg border border-zinc-300 px-3 py-2 text-xs font-semibold text-zinc-700 hover:border-pink-400 hover:text-pink-600 dark:border-white/15 dark:text-zinc-200"
            >
              <Download size={13} /> {t('midiSave')}
            </a>
            <button type="button" onClick={() => void remove()} className="inline-flex items-center gap-2 rounded-lg border border-zinc-200 px-3 py-2 text-xs text-zinc-500 hover:border-rose-400 hover:text-rose-600 dark:border-white/10">
              <Trash2 size={13} /> {t('midiDelete')}
            </button>
          </>
        )}
      </div>

      {running && run && (
        <div className="mt-3">
          <div className="flex items-center justify-between text-xs font-medium text-zinc-700 dark:text-zinc-200">
            <span className="flex items-center gap-2"><Loader2 size={14} className="animate-spin text-pink-500" />{tt(`midiStage_${run.stage}`)}</span>
            <span className="tabular-nums text-zinc-500">{percent}%</span>
          </div>
          <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-zinc-200 dark:bg-black/30">
            <div className="h-full bg-gradient-to-r from-orange-500 to-pink-500 transition-[width]" style={{ width: `${percent}%` }} />
          </div>
        </div>
      )}

      {running && run?.stage === 'transcribing' && (
        <MidiPlayer key={`live-${songId}`} notes={liveNotes} sourceAudioUrl={audioUrl} live chunks={{ done: run.chunks_done, total: run.chunks_total }} />
      )}
      {!running && songMidi && (
        <>
          <p className="mt-3 select-text text-[11px] text-zinc-500 [overflow-wrap:anywhere]">
            {songMidi.size ? `${tt(`midiSize_${songMidi.size}`)} · ` : ''}{songMidi.instruments.length} {t('midiInstruments')} · {songMidi.file}
          </p>
          <MidiPlayer key={`done-${songId}-${songMidi.notes.length}`} notes={songMidi.notes} sourceAudioUrl={audioUrl} live={false} />
        </>
      )}

      {(run?.error || error) && (
        <p role="alert" className="mt-3 select-text rounded-lg bg-rose-500/10 px-3 py-2 text-xs text-rose-700 [overflow-wrap:anywhere] dark:text-rose-300">{run?.error || error}</p>
      )}
      <p className="mt-3 text-[11px] leading-4 text-zinc-400">{t('midiLicense')}</p>
    </section>
  );
};
