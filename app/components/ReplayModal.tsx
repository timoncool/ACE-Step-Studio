import React, { useState } from 'react';
import { AlertTriangle, Loader2, Repeat, X } from 'lucide-react';
import { Song } from '../types';
import { useI18n } from '../context/I18nContext';
import { SCHEDULERS, SOLVERS } from '../services/aceEngine';

/**
 * Deterministic re-render.
 *
 * A track keeps the audio codes its language model wrote. Re-submitting them
 * skips the planning entirely: the composition stays while the DiT renders it
 * again, so this is the way to change step count, guidance or output format
 * without generating a different song.
 */

interface ReplayModalProps {
  song: Song;
  onClose: () => void;
  onQueued: (jobId: string) => void;
}

const CONTROL =
  'w-full rounded-lg border border-zinc-200 bg-white px-3 py-2 text-sm text-zinc-900 outline-none focus:border-pink-500 dark:border-white/10 dark:bg-black/20 dark:text-white';

export const ReplayModal: React.FC<ReplayModalProps> = ({ song, onClose, onQueued }) => {
  const { t } = useI18n();
  const tt = t as unknown as (key: string) => string;
  const settings = (song.generationParams ?? {}) as Record<string, unknown>;
  const numberOr = (key: string, fallback: number) =>
    typeof settings[key] === 'number' ? (settings[key] as number) : fallback;
  const textOr = (key: string, fallback: string) => (typeof settings[key] === 'string' && settings[key] ? (settings[key] as string) : fallback);

  const [steps, setSteps] = useState<number>(numberOr('inference_steps', 8));
  const [cfg, setCfg] = useState<number>(numberOr('guidance_scale', 1));
  const [solver, setSolver] = useState(textOr('solver', 'euler'));
  const [scheduler, setScheduler] = useState(textOr('scheduler', 'linear'));
  const [seed, setSeed] = useState<string>(typeof settings.seed === 'number' ? String(settings.seed) : '');
  const [format, setFormat] = useState<'mp3' | 'wav16' | 'wav24' | 'wav32' | 'flac'>(
    typeof settings.output_format === 'string' ? (settings.output_format as 'mp3') : 'mp3',
  );
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      const parsedSeed = seed.trim() === '' ? undefined : Number(seed);
      if (parsedSeed !== undefined && (!Number.isInteger(parsedSeed) || parsedSeed < 0)) {
        throw new Error('Seed must be a non-negative integer.');
      }
      const response = await fetch('/v1/music/replay', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          song_id: song.id,
          steps,
          seed: parsedSeed,
          output_format: format,
          changes: { guidance_scale: cfg, solver, scheduler },
        }),
      });
      const body = await response.json().catch(() => null);
      if (!response.ok) throw new Error(body?.error || `Re-render failed (${response.status})`);
      onQueued(body.id);
      onClose();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'Re-render failed.');
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="fixed inset-0 z-[70] flex items-center justify-center bg-black/60 p-4" onClick={onClose}>
      <div className="w-full max-w-md overflow-hidden rounded-2xl bg-white shadow-2xl dark:bg-zinc-900" onClick={event => event.stopPropagation()}>
        <div className="flex items-center justify-between border-b border-zinc-200 px-5 py-4 dark:border-white/10">
          <h3 className="flex items-center gap-2 text-base font-bold text-zinc-900 dark:text-white">
            <Repeat size={17} className="text-pink-500" /> {t('replayTitle')}
          </h3>
          <button type="button" onClick={onClose} className="text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-200">
            <X size={18} />
          </button>
        </div>

        <div className="space-y-3 p-5">
          <p className="truncate text-sm font-semibold text-zinc-900 dark:text-white">{song.title}</p>
          <p className="text-xs leading-5 text-zinc-500 dark:text-zinc-400">{t('replayHint')}</p>

          <div className="grid grid-cols-2 gap-3">
            <label className="block text-xs font-medium text-zinc-600 dark:text-zinc-300">
              <span className="mb-1.5 block">{t('ditSteps')}</span>
              <input type="number" min={1} max={200} value={steps} onChange={event => setSteps(Number(event.target.value) || 1)} className={CONTROL} />
            </label>
            <label className="block text-xs font-medium text-zinc-600 dark:text-zinc-300">
              <span className="mb-1.5 block">CFG</span>
              <input type="number" min={1} max={15} step={0.1} value={cfg} onChange={event => setCfg(Number(event.target.value) || 1)} className={CONTROL} />
            </label>
            <label className="block text-xs font-medium text-zinc-600 dark:text-zinc-300">
              <span className="mb-1.5 block">{tt('aceSolver')}</span>
              <select value={solver} onChange={event => setSolver(event.target.value)} className={CONTROL}>
                {SOLVERS.map(value => <option key={value} value={value}>{tt(`aceSolver_${value}`)}</option>)}
              </select>
            </label>
            <label className="block text-xs font-medium text-zinc-600 dark:text-zinc-300">
              <span className="mb-1.5 block">{tt('aceScheduler')}</span>
              <select value={scheduler} onChange={event => setScheduler(event.target.value)} className={CONTROL}>
                {SCHEDULERS.map(value => <option key={value} value={value}>{tt(`aceScheduler_${value}`)}</option>)}
              </select>
            </label>
            <label className="block text-xs font-medium text-zinc-600 dark:text-zinc-300">
              <span className="mb-1.5 block">{t('ditSeed')}</span>
              <input inputMode="numeric" value={seed} onChange={event => setSeed(event.target.value)} className={CONTROL} />
            </label>
            <label className="block text-xs font-medium text-zinc-600 dark:text-zinc-300">
              <span className="mb-1.5 block">{t('outputFormat')}</span>
              <select value={format} onChange={event => setFormat(event.target.value as typeof format)} className={CONTROL}>
                <option value="mp3">MP3</option>
                <option value="wav16">WAV 16-bit</option>
                <option value="wav24">WAV 24-bit</option>
                <option value="wav32">WAV 32-bit float</option>
                <option value="flac">FLAC</option>
              </select>
            </label>
          </div>

          {error && (
            <p role="alert" className="flex items-center gap-2 rounded-lg bg-rose-500/10 px-3 py-2 text-xs text-rose-700 dark:text-rose-300">
              <AlertTriangle size={14} /> {error}
            </p>
          )}
        </div>

        <div className="flex justify-end gap-2 border-t border-zinc-200 px-5 py-4 dark:border-white/10">
          <button type="button" onClick={onClose} className="rounded-lg px-4 py-2 text-sm font-medium text-zinc-500 hover:text-zinc-800 dark:hover:text-zinc-200">
            {t('cancel')}
          </button>
          <button
            type="button"
            onClick={() => void submit()}
            disabled={busy}
            className="inline-flex items-center gap-2 rounded-lg bg-gradient-to-r from-orange-500 to-pink-600 px-4 py-2 text-sm font-bold text-white disabled:opacity-50"
          >
            {busy ? <Loader2 size={14} className="animate-spin" /> : <Repeat size={14} />} {t('replayStart')}
          </button>
        </div>
      </div>
    </div>
  );
};
