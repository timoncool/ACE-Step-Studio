import React, { useCallback, useEffect, useRef, useState } from 'react';
import { Check, Copy, Download, FileAudio, Loader2, Music, Play, RefreshCw, Scissors, Search, SlidersHorizontal, Wand2 } from 'lucide-react';
import { mapNativeLibrarySong } from '../services/nativeLibrary';
import { MidiTool } from './midi/MidiTool';
import { useI18n } from '../context/I18nContext';
import { DevicePicker } from './DevicePicker';
import { transcribeWithNativeOpenRouter } from '../services/nativeOpenRouter';
import { apiUrl } from '../services/apiBase';
import { openExternal } from '../services/externalLinks';
import { StemPlayer } from './StemPlayer';

/**
 * Studio tools.
 *
 * Work done to a finished track rather than to a new one: splitting a song into
 * its parts, and turning a reference recording into text you can paste into a
 * lyric sheet. Both run through the studio's own service - the separator on the
 * ONNX Runtime it already ships, the transcription through whichever recogniser
 * the user connected.
 *
 * The page used to be diagnostics. Those moved to Settings, next to the flags
 * that produce them, which left this page for the tools it is named after.
 */

interface LibrarySong {
  id: string;
  title: string;
  audio_path?: string | null;
}

interface SeparationStatus {
  model: { label: string; bytes: number; installed: boolean; note: string };
  runtime_installed: boolean;
  ready: boolean;
  stems: string[];
  download: { downloaded_bytes: number; total_bytes: number; done: boolean } | null;
  cuda_runtime_installed: boolean;
  card_missing_bytes: number;
  settings: { stems: string[]; overlap: number; runtime: 'auto' | 'cuda' | 'cpu' };
  run: { song_id: string; progress: number; done: boolean; error: string | null; stems: string[] } | null;
}

/** Fast, the model's own reference setting, and slow-but-smoothest. */
const QUALITIES = [
  { overlap: 0.1, key: 'separationQualityFast' },
  { overlap: 0.25, key: 'separationQualityBalanced' },
  { overlap: 0.5, key: 'separationQualityBest' },
] as const;

interface CatalogModel {
  id: string;
  name: string;
  capabilities: string[];
}

const CARD = 'rounded-2xl border border-zinc-200 bg-zinc-50 p-4 dark:border-white/10 dark:bg-white/[0.03]';
const CONTROL =
  'w-full rounded-lg border border-zinc-200 bg-white px-3 py-2 text-sm text-zinc-900 outline-none focus:border-pink-500 disabled:opacity-50 dark:border-white/10 dark:bg-black/30 dark:text-white';

const megabytes = (bytes: number) => `${(bytes / 1024 / 1024).toFixed(0)} MB`;

export function StudioToolsPanel({ initialSongId }: { initialSongId?: string | null } = {}): React.ReactElement {
  const { t } = useI18n();

  const [songs, setSongs] = useState<LibrarySong[]>([]);
  const [songId, setSongId] = useState('');
  const [status, setStatus] = useState<SeparationStatus | null>(null);
  const [stems, setStems] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [asrModels, setAsrModels] = useState<CatalogModel[]>([]);
  const [asrModel, setAsrModel] = useState('');
  const [asrFile, setAsrFile] = useState<File | null>(null);
  const [asrLanguage, setAsrLanguage] = useState('');
  const [asrBusy, setAsrBusy] = useState(false);
  const [asrText, setAsrText] = useState('');
  const [copied, setCopied] = useState(false);
  const [query, setQuery] = useState('');
  const filePicker = useRef<HTMLInputElement | null>(null);

  const loadSongs = useCallback(async () => {
    const body = await fetch('/v1/library/songs').then(response => response.json());
    const list: LibrarySong[] = Array.isArray(body) ? body : body.songs ?? [];
    const playable = list.filter(song => song.audio_path);
    setSongs(playable);
    setSongId(current => current || playable[0]?.id || '');
  }, []);

  // A track handed over by a menu wins over whatever was chosen here before.
  useEffect(() => {
    if (initialSongId) setSongId(initialSongId);
  }, [initialSongId]);

  useEffect(() => {
    void loadSongs().catch(() => undefined);
    void fetch('/v1/openrouter/catalog')
      .then(response => response.json())
      .then((body: { models?: CatalogModel[] }) => {
        const recognisers = (body.models ?? []).filter(model => model.capabilities.includes('speech_to_text'));
        setAsrModels(recognisers);
        setAsrModel(current => current || recognisers[0]?.id || '');
      })
      .catch(() => undefined);
  }, [loadSongs]);

  // The separation state, and which stems the chosen track already has.
  const refresh = useCallback(async () => {
    const separation: SeparationStatus = await fetch('/v1/separation/status').then(response => response.json());
    setStatus(separation);
    if (songId) {
      const body = await fetch(`/v1/library/songs/${encodeURIComponent(songId)}/stems`).then(response => response.json());
      setStems(body.stems ?? []);
    }
  }, [songId]);

  useEffect(() => {
    void refresh().catch(() => undefined);
    const timer = window.setInterval(() => void refresh().catch(() => undefined), 1000);
    return () => window.clearInterval(timer);
  }, [refresh]);

  const saveSettings = async (next: { stems: string[]; overlap: number; runtime: 'auto' | 'cuda' | 'cpu' }) => {
    setStatus(current => (current ? { ...current, settings: next } : current));
    await fetch('/v1/separation/settings', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(next),
    }).catch(() => undefined);
  };

  const install = async () => {
    setBusy(true);
    setError(null);
    await fetch('/v1/separation/install', { method: 'POST' }).catch(() => undefined);
    setBusy(false);
  };

  const separate = async () => {
    if (!songId) return;
    setBusy(true);
    setError(null);
    const response = await fetch(`/v1/library/songs/${encodeURIComponent(songId)}/stems`, { method: 'POST' });
    if (!response.ok) {
      const body = await response.json().catch(() => null);
      setError(body?.error || `Separation could not start (${response.status})`);
    }
    setBusy(false);
  };

  const transcribe = async () => {
    if (!asrFile || !asrModel) return;
    setAsrBusy(true);
    setError(null);
    setAsrText('');
    try {
      setAsrText(await transcribeWithNativeOpenRouter(asrModel, asrFile, asrLanguage.trim() || undefined));
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setAsrBusy(false);
    }
  };

  const copyText = async () => {
    try {
      await navigator.clipboard.writeText(asrText);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      // Clipboard permission is not worth an error banner.
    }
  };

  // The list the user actually sees: filtered, and short enough to scan.
  // A library of five thousand tracks is not a list to scroll: the search
  // narrows it, and only the first handful of matches are drawn.
  const needle = query.trim().toLowerCase();
  const matches = needle ? songs.filter(song => song.title.toLowerCase().includes(needle)) : songs;
  const visible = matches.slice(0, 12);
  const hidden = matches.length - visible.length;
  const settings = status?.settings ?? { stems: status?.stems ?? [], overlap: 0.25, runtime: 'auto' as const };
  const run = status?.run?.song_id === songId ? status?.run : null;
  const running = Boolean(run && !run.done);
  const percent = Math.round((run?.progress ?? 0) * 100);
  const download = status?.download;

  return (
    <div className="flex-1 overflow-y-auto bg-white px-5 py-6 dark:bg-suno md:px-8">
      <div className="mx-auto max-w-4xl space-y-6">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div>
            <p className="text-xs font-semibold uppercase tracking-[0.18em] text-pink-500">{t('studioTools')}</p>
            <h1 className="mt-1 text-2xl font-bold text-zinc-950 dark:text-white">{t('toolsHeading')}</h1>
            <p className="mt-2 max-w-3xl text-sm text-zinc-600 dark:text-zinc-400">{t('toolsIntro')}</p>
          </div>
          <button
            onClick={() => void loadSongs()}
            className="inline-flex items-center gap-2 rounded-lg border border-zinc-200 px-3 py-2 text-sm text-zinc-700 hover:border-pink-400 hover:text-pink-600 dark:border-white/10 dark:text-zinc-200"
          >
            <RefreshCw size={15} /> {t('refresh')}
          </button>
        </div>

        {/* Stems. */}
        <section className={CARD}>
          <div className="flex items-center gap-2 text-sm font-semibold text-zinc-900 dark:text-white">
            <Scissors size={17} className="text-pink-500" /> {t('stemsTitle')}
          </div>
          <p className="mt-2 text-sm text-zinc-600 dark:text-zinc-300">{t('stemsHint')}</p>

          {/* A track is picked from a list, not from a native dropdown: a
              generated title can be a whole paragraph, and an <option> shows
              all of it, at any width it likes. */}
          <div className="mt-3">
            <div className="relative">
              <Search size={14} className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-zinc-400" />
              <input
                value={query}
                onChange={event => setQuery(event.target.value)}
                placeholder={t('searchYourSongs')}
                className={`${CONTROL} pl-9`}
              />
            </div>
            <div className="mt-2 max-h-56 overflow-y-auto rounded-lg border border-zinc-200 dark:border-white/10">
              {visible.length === 0 && (
                <p className="px-3 py-3 text-xs text-zinc-500">{t('noSongsYet')}</p>
              )}
              {visible.map(song => (
                <button
                  key={song.id}
                  type="button"
                  onClick={() => setSongId(song.id)}
                  className={`flex w-full items-center gap-2 border-b border-zinc-100 px-3 py-2 text-left last:border-b-0 dark:border-white/5 ${
                    song.id === songId ? 'bg-pink-500/10' : 'hover:bg-zinc-100 dark:hover:bg-white/5'
                  }`}
                >
                  <Music size={13} className={song.id === songId ? 'shrink-0 text-pink-500' : 'shrink-0 text-zinc-400'} />
                  <span className="min-w-0 flex-1 truncate text-sm text-zinc-800 dark:text-zinc-200">{song.title}</span>
                  {song.id === songId && <Check size={14} className="shrink-0 text-pink-500" />}
                </button>
              ))}
              {hidden > 0 && (
                <p className="px-3 py-2 text-[11px] text-zinc-500">{t('searchNarrowsMore')} · {hidden}</p>
              )}
            </div>
            {/* What to keep, and how carefully to join the segments. Both are
                remembered, so the next track starts where this one left off. */}
            <div className="mt-4">
              <p className="text-[11px] font-bold uppercase tracking-wide text-zinc-500">{t('separationWhichStems')}</p>
              <div className="mt-2 flex flex-wrap gap-2">
                {(status?.stems ?? []).map(stem => {
                  const chosen = settings.stems.includes(stem);
                  return (
                    <button
                      key={stem}
                      type="button"
                      onClick={() => {
                        const next = chosen ? settings.stems.filter(name => name !== stem) : [...settings.stems, stem];
                        void saveSettings({ ...settings, stems: next });
                      }}
                      className={`inline-flex items-center gap-2 rounded-full border px-3 py-1.5 text-xs font-semibold ${
                        chosen
                          ? 'border-pink-400 bg-pink-500/10 text-zinc-900 dark:text-white'
                          : 'border-zinc-200 text-zinc-500 dark:border-white/10'
                      }`}
                    >
                      <span className={`grid h-3.5 w-3.5 place-items-center rounded ${chosen ? 'bg-pink-500 text-white' : 'bg-zinc-200 dark:bg-white/10'}`}>
                        {chosen && <Check size={9} strokeWidth={3} />}
                      </span>
                      {t(`stem_${stem}` as never) || stem}
                    </button>
                  );
                })}
              </div>
            </div>

            {/* Card or processor. The card needs its own build of the runtime,
                which is listed with the other optional downloads. */}
            <div className="mt-3">
              <p className="text-[11px] font-bold uppercase tracking-wide text-zinc-500">{t('separationRuntime')}</p>
              <div className="mt-2">
                <DevicePicker
                  value={settings.runtime}
                  onChange={choice => void saveSettings({ ...settings, runtime: choice })}
                  cudaAvailable={Boolean(status?.cuda_runtime_installed)}
                />
              </div>
              {/* Where a capability is installed is one place: the models
                  panel. This tool used to install the card runtime itself, with
                  its own button and its own progress, and disagree with it. */}
              {!status?.cuda_runtime_installed && (
                <p className="mt-2 text-[11px] leading-4 text-amber-600 dark:text-amber-300">
                  {t('separationInstallGpu')}
                  {status?.card_missing_bytes ? ` · ${megabytes(status.card_missing_bytes)}` : ''}
                </p>
              )}
            </div>

            <div className="mt-3">
              <p className="text-[11px] font-bold uppercase tracking-wide text-zinc-500">{t('separationQuality')}</p>
              <div className="mt-2 flex gap-2">
                {QUALITIES.map(quality => (
                  <button
                    key={quality.overlap}
                    type="button"
                    onClick={() => void saveSettings({ ...settings, overlap: quality.overlap })}
                    className={`flex-1 rounded-lg border px-3 py-2 text-xs font-semibold ${
                      Math.abs(settings.overlap - quality.overlap) < 0.01
                        ? 'border-pink-400 bg-pink-500/10 text-zinc-900 dark:text-white'
                        : 'border-zinc-200 text-zinc-500 dark:border-white/10'
                    }`}
                  >
                    {t(quality.key)}
                  </button>
                ))}
              </div>
              <p className="mt-2 text-xs leading-5 text-zinc-500 dark:text-zinc-400">{t('separationQualityHint')}</p>
            </div>

            <button
              type="button"
              onClick={() => void separate()}
              disabled={!status?.ready || !songId || settings.stems.length === 0 || running || busy}
              className="mt-3 inline-flex items-center justify-center gap-2 rounded-lg bg-gradient-to-r from-orange-500 to-pink-600 px-4 py-2 text-sm font-bold text-white disabled:cursor-not-allowed disabled:opacity-50"
            >
              {running ? <Loader2 size={14} className="animate-spin" /> : <Play size={14} />}
              {stems.length > 0 ? t('stemsAgain') : t('stemsStart')}
            </button>
          </div>

          {status && !status.model.installed && (
            <div className="mt-3 rounded-lg border border-zinc-200 p-3 dark:border-white/10">
              <p className="text-xs leading-5 text-zinc-600 dark:text-zinc-300">
                {status.model.label} · {megabytes(status.model.bytes)} — {status.model.note}
              </p>
              {download && !download.done ? (
                <div className="mt-2">
                  <div className="h-1.5 overflow-hidden rounded-full bg-zinc-200 dark:bg-black/30">
                    <div
                      className="h-full bg-gradient-to-r from-orange-500 to-pink-500 transition-[width]"
                      style={{ width: `${Math.round((download.downloaded_bytes / Math.max(1, download.total_bytes)) * 100)}%` }}
                    />
                  </div>
                  <p className="mt-1 text-[11px] tabular-nums text-zinc-500">
                    {megabytes(download.downloaded_bytes)} / {megabytes(download.total_bytes)}
                  </p>
                </div>
              ) : (
                <button
                  type="button"
                  onClick={() => void install()}
                  disabled={busy}
                  className="mt-2 inline-flex items-center gap-2 rounded-lg bg-zinc-900 px-3 py-1.5 text-xs font-bold text-white disabled:opacity-50 dark:bg-white dark:text-zinc-900"
                >
                  <Download size={13} /> {t('stemsInstallModel')}
                </button>
              )}
            </div>
          )}

          {running && (
            <div className="mt-3">
              <div className="flex items-center justify-between text-xs font-medium text-zinc-700 dark:text-zinc-200">
                <span className="flex items-center gap-2"><Loader2 size={14} className="animate-spin text-pink-500" />{t('stemsRunning')}</span>
                <span className="tabular-nums text-zinc-500">{percent}%</span>
              </div>
              <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-zinc-200 dark:bg-black/30">
                <div className="h-full bg-gradient-to-r from-orange-500 to-pink-500 transition-[width]" style={{ width: `${percent}%` }} />
              </div>
            </div>
          )}

          {stems.length > 0 && (
            <div className="mt-3 space-y-2">
              {stems.map(stem => {
                const url = apiUrl(`/v1/library/songs/${encodeURIComponent(songId)}/stems/${stem}`);
                return (
                  <div key={stem} className="flex items-center gap-2">
                    <div className="min-w-0 flex-1">
                      <StemPlayer src={url} label={t(`stem_${stem}` as never) || stem} />
                    </div>
                    <a
                      href={url}
                      download={`${songs.find(song => song.id === songId)?.title ?? 'track'} - ${stem}.wav`}
                      className="shrink-0 text-zinc-400 hover:text-pink-500"
                      title={t('download')}
                    >
                      <Download size={15} />
                    </a>
                  </div>
                );
              })}
            </div>
          )}

          {run?.error && (
            <p role="alert" className="mt-3 select-text rounded-lg bg-rose-500/10 px-3 py-2 text-xs text-rose-700 [overflow-wrap:anywhere] dark:text-rose-300">{run.error}</p>
          )}
        </section>

        {/* Audio to MIDI, on the track chosen above. */}
        <MidiTool songId={songId} songTitle={songs.find(song => song.id === songId)?.title ?? ''} card={CARD} />

        {/* The audio editor, on the track chosen above or on any stem it produced. */}
        <section className={CARD}>
          <div className="flex items-center gap-2 text-sm font-semibold text-zinc-900 dark:text-white">
            <SlidersHorizontal size={17} className="text-pink-500" /> {t('audioEditor')}
          </div>
          <p className="mt-2 text-sm text-zinc-600 dark:text-zinc-300">{t('audioEditorIntro')}</p>
          <div className="mt-3 flex flex-wrap gap-2">
            <button
              type="button"
              onClick={() => {
                if (!songId) return;
                const url = apiUrl(`/v1/library/media/${encodeURIComponent(songId)}`);
                void openExternal(apiUrl(`/editor/index.html?audioUrl=${encodeURIComponent(url)}`));
              }}
              disabled={!songId}
              className="inline-flex items-center gap-2 rounded-lg border border-zinc-300 px-3 py-2 text-xs font-semibold text-zinc-700 hover:border-pink-400 hover:text-pink-600 disabled:opacity-50 dark:border-white/15 dark:text-zinc-200"
            >
              <SlidersHorizontal size={13} /> {t('openTrackInEditor')}
            </button>
            {stems.map(stem => (
              <button
                key={stem}
                type="button"
                onClick={() => {
                  const url = apiUrl(`/v1/library/songs/${encodeURIComponent(songId)}/stems/${stem}`);
                  void openExternal(apiUrl(`/editor/index.html?audioUrl=${encodeURIComponent(url)}`));
                }}
                className="inline-flex items-center gap-2 rounded-lg border border-zinc-200 px-3 py-2 text-xs font-medium text-zinc-600 hover:border-pink-400 hover:text-pink-600 dark:border-white/10 dark:text-zinc-300"
              >
                {t(`stem_${stem}` as never) || stem}
              </button>
            ))}
          </div>
        </section>

        {/* Processing: the same window the track menu opens, on the track chosen above. */}
        <section className={CARD}>
          <div className="flex items-center gap-2 text-sm font-semibold text-zinc-900 dark:text-white">
            <Wand2 size={17} className="text-pink-500" /> {t('processTitle')}
          </div>
          <p className="mt-2 text-sm text-zinc-600 dark:text-zinc-300">{t('processHint')}</p>
          <button
            type="button"
            onClick={() => {
              if (!songId) return;
              void fetch(`/v1/library/songs/${encodeURIComponent(songId)}`)
                .then(response => (response.ok ? response.json() : Promise.reject(new Error(`HTTP ${response.status}`))))
                .then(body => window.dispatchEvent(new CustomEvent('studio:process-song', { detail: mapNativeLibrarySong(body) })))
                .catch(problem => setError(problem instanceof Error ? problem.message : String(problem)));
            }}
            disabled={!songId}
            className="mt-3 inline-flex items-center gap-2 rounded-lg border border-zinc-300 px-3 py-2 text-xs font-semibold text-zinc-700 hover:border-pink-400 hover:text-pink-600 disabled:opacity-50 dark:border-white/15 dark:text-zinc-200"
          >
            <Wand2 size={13} /> {t('processMenu')}
          </button>
        </section>

        {/* Transcription. */}
        <section className={CARD}>
          <div className="flex items-center gap-2 text-sm font-semibold text-zinc-900 dark:text-white">
            <FileAudio size={17} className="text-pink-500" /> {t('transcribeAudio')}
          </div>
          <p className="mt-2 text-sm text-zinc-600 dark:text-zinc-300">{t('transcribeIntro')}</p>

          <div className="mt-3 grid gap-2 md:grid-cols-3">
            <select value={asrModel} onChange={event => setAsrModel(event.target.value)} className={CONTROL}>
              {asrModels.length === 0 && <option value="">{t('noSpeechModel')}</option>}
              {asrModels.map(model => <option key={model.id} value={model.id}>{model.name}</option>)}
            </select>
            <button
              type="button"
              onClick={() => filePicker.current?.click()}
              className={`${CONTROL} text-left ${asrFile ? '' : 'text-zinc-400'}`}
            >
              {asrFile?.name ?? t('chooseAudioFile')}
            </button>
            <input
              value={asrLanguage}
              onChange={event => setAsrLanguage(event.target.value)}
              placeholder={t('languageOptional')}
              className={CONTROL}
            />
          </div>
          <input
            ref={filePicker}
            type="file"
            accept="audio/*"
            className="hidden"
            onChange={event => { setAsrFile(event.target.files?.[0] ?? null); event.target.value = ''; }}
          />

          <button
            type="button"
            onClick={() => void transcribe()}
            disabled={!asrFile || !asrModel || asrBusy}
            className="mt-3 inline-flex items-center gap-2 rounded-lg bg-gradient-to-r from-orange-500 to-pink-600 px-4 py-2 text-sm font-bold text-white disabled:cursor-not-allowed disabled:opacity-50"
          >
            {asrBusy ? <Loader2 size={14} className="animate-spin" /> : <FileAudio size={14} />} {t('transcribeAudio')}
          </button>

          {asrText && (
            <div className="mt-3">
              <textarea readOnly value={asrText} rows={8} className={`${CONTROL} resize-y font-mono text-[13px] leading-5`} />
              <button
                type="button"
                onClick={() => void copyText()}
                className="mt-2 inline-flex items-center gap-2 rounded-lg border border-zinc-300 px-3 py-1.5 text-xs font-semibold text-zinc-700 hover:border-pink-400 hover:text-pink-600 dark:border-white/15 dark:text-zinc-200"
              >
                {copied ? <Check size={13} /> : <Copy size={13} />} {copied ? t('copied') : t('copy')}
              </button>
            </div>
          )}
        </section>

        {error && (
          <p role="alert" className="rounded-lg bg-rose-500/10 px-3 py-2 text-sm text-rose-700 dark:text-rose-300">{error}</p>
        )}
      </div>
    </div>
  );
}
