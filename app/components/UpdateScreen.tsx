import React, { useCallback, useEffect, useRef, useState } from 'react';
import { ArrowRight, Check, Download, Loader2, X } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { SELF_READ } from '../services/studioQueries';
import type { TranslationKey } from '../i18n/translations';

type Row = { id: string; part: string; ids: string[]; name: string; from: string | null; to: string; bytes: number; download: number; reason_code: string; reason: string };
type Gone = { what: string; name: string; paths: string[]; size: number; reason_code: string; reason: string };
export type UpdatePlan = { version: string; updated: Row[]; added: Row[]; retired: Gone[]; download: number; freed: number; show: boolean };
type UpdateJob = { status: 'running' | 'done' | 'failed'; error: string | null; updated: Row[]; added: Row[] };
type UpdateAnswer = { plan: UpdatePlan; job: UpdateJob | null };

/** The window's words for the reasons of the release's table of changes, by their codes. */
const WHY: Record<string, TranslationKey> = {
  'crisperwhisper': 'updateWhyCrisperwhisper',
  'transcribe-avx2': 'updateWhyTranscribeAvx2',
  'llama': 'updateWhyLlama',
  'hotstep': 'updateWhyHotstep',
  'cublas-wheel': 'updateWhyCublasWheel',
  'faster-whisper-gone': 'updateWhyFasterWhisperGone',
  'parakeet-onnx-gone': 'updateWhyParakeetOnnxGone',
  'ort-cpu': 'updateWhyOrtCpu',
  'ort-gpu-gone': 'updateWhyOrtGpuGone',
  'demucs-audiocpp': 'updateWhyDemucsAudiocpp',
  'midi-audiocpp': 'updateWhyMidiAudiocpp',
  'newer': 'updateWhyNewer',
  'unused': 'updateWhyUnused',
};

const bytes = (value: number) => (value < 1024 ** 3 ? `${Math.round(value / 1024 ** 2)} MB` : `${(value / 1024 ** 3).toFixed(1)} GB`);

async function answer(response: Response): Promise<UpdateAnswer> {
  const body = await response.json().catch(() => null);
  if (!response.ok) throw new Error(body?.error ?? `HTTP ${response.status}`);
  return body as UpdateAnswer;
}

export const readUpdatePlan = () => fetch('/setup/update', { headers: SELF_READ }).then(answer);

/** The update screen's plan: three groups, the totals and one button that downloads it all and then removes what is
 * no longer needed. `onDone` follows a finished update; `onLater` is given where the screen can be put off. */
export const UpdatePlanView: React.FC<{ onDone?: () => void; onLater?: () => void }> = ({ onDone, onLater }) => {
  const { t } = useI18n();
  const [state, setState] = useState<UpdateAnswer | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  const load = useCallback(() => readUpdatePlan().then(setState, (problem: unknown) => setError(problem instanceof Error ? problem.message : String(problem))), []);
  useEffect(() => { void load(); }, [load]);

  const running = state?.job?.status === 'running';
  // a parent that renders every second hands a new onDone each time; the poll must not restart with it
  const onDoneRef = useRef(onDone);
  onDoneRef.current = onDone;
  useEffect(() => {
    if (!running) return;
    const timer = window.setInterval(() => {
      void readUpdatePlan().then((next) => {
        setState(next);
        if (next.job?.status === 'done') onDoneRef.current?.();
      }, (problem: unknown) => setError(problem instanceof Error ? problem.message : String(problem)));
    }, 1000);
    return () => window.clearInterval(timer);
  }, [running]);

  const apply = async () => {
    setError(null);
    setStarting(true);
    try {
      const next = await fetch('/setup/update/apply', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: '{}' }).then(answer);
      setState(next);
      if (next.job?.status === 'done') onDone?.();
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setStarting(false);
    }
  };
  const later = () => {
    void fetch('/setup/update/seen', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: '{}' })
      .then((response) => (response.ok ? onLater?.() : response.json().then((body) => setError(body?.error ?? `HTTP ${response.status}`))))
      .catch((problem: unknown) => setError(problem instanceof Error ? problem.message : String(problem)));
  };

  const plan = state?.plan ?? null;
  const job = state?.job ?? null;
  const reason = (code: string, english: string) => (WHY[code] ? t(WHY[code]) : english);
  const progress = (row: Row): number | null => (running && row.bytes > 0 ? ((row.bytes - row.download) / row.bytes) * 100 : null);
  // while the download runs, its rows stay on screen as they were when it began
  const updated = running && job ? job.updated : plan?.updated ?? [];
  const added = running && job ? job.added : plan?.added ?? [];
  const empty = plan != null && !running && plan.updated.length + plan.added.length + plan.retired.length === 0;
  const busy = running || starting;

  const group = (label: string, rows: React.ReactNode[]) => rows.length > 0 && (
    <div className="rounded-xl border border-zinc-200 bg-white px-4 py-3 dark:border-white/10 dark:bg-suno-card">
      <p className="mb-2 text-[11px] font-bold uppercase tracking-wide text-zinc-500 dark:text-zinc-400">{label}</p>
      <div className="space-y-2.5">{rows}</div>
    </div>
  );
  const line = (key: string, name: string, version: React.ReactNode, size: string, why: string, done: number | null) => (
    <div key={key}>
      <div className="flex items-center gap-2">
        <span className="truncate text-sm font-semibold text-zinc-900 dark:text-white">{name}</span>
        {version}
        <span className="ml-auto shrink-0 text-xs tabular-nums text-zinc-500">{size}</span>
      </div>
      <p className="text-xs leading-5 text-zinc-500 dark:text-zinc-400">{why}</p>
      {done != null && (
        <div className="mt-1 h-1 overflow-hidden rounded-full bg-zinc-200 dark:bg-white/10"><div className="h-full bg-pink-500" style={{ width: `${done}%` }} /></div>
      )}
    </div>
  );
  const version = (from: string | null, to: string) => (
    <span className="inline-flex min-w-0 items-center gap-1 truncate font-mono text-[10.5px] text-zinc-500">
      {from && <>{from}<ArrowRight size={11} className="shrink-0" /></>}{to}
    </span>
  );

  return (
    <div className="space-y-3">
      {error && (
        <div className="flex gap-2 rounded-xl border border-rose-300 bg-rose-50 px-4 py-3 text-sm text-rose-700 dark:border-rose-500/30 dark:bg-rose-500/10 dark:text-rose-200">
          <X size={16} className="mt-0.5 shrink-0" />{error}
        </div>
      )}
      {job?.status === 'failed' && (
        <div className="flex gap-2 rounded-xl border border-rose-300 bg-rose-50 px-4 py-3 text-sm text-rose-700 dark:border-rose-500/30 dark:bg-rose-500/10 dark:text-rose-200">
          <X size={16} className="mt-0.5 shrink-0" />{t('updateFailed').replace('{error}', job.error ?? '')}
        </div>
      )}
      {job?.status === 'done' && empty && (
        <div className="flex gap-2 rounded-xl border border-emerald-500/30 bg-emerald-500/5 px-4 py-3 text-sm text-emerald-700 dark:text-emerald-300">
          <Check size={16} className="mt-0.5 shrink-0" />{t('updateDone')}
        </div>
      )}
      {!plan && !error && <Loader2 size={18} className="animate-spin text-zinc-400" />}
      {empty && job?.status !== 'done' && <p className="text-sm text-zinc-500 dark:text-zinc-400">{t('updateNothing')}</p>}
      {group(t('updateUpdated'), updated.map((row) => line(`${row.part}-${row.id}`, row.name, version(row.from, row.to), bytes(row.download), reason(row.reason_code, row.reason), progress(row))))}
      {group(t('updateAdded'), added.map((row) => line(`${row.part}-${row.id}`, row.name, version(null, row.to), bytes(row.download), reason(row.reason_code, row.reason), progress(row))))}
      {plan && group(t('updateRetired'), plan.retired.map((gone) => line(gone.what, gone.name, null, gone.size > 0 ? `−${bytes(gone.size)}` : '', reason(gone.reason_code, gone.reason), null)))}
      {plan && !empty && (
        <div className="flex flex-wrap items-center gap-3 pt-1">
          <button
            type="button"
            onClick={() => void apply()}
            disabled={busy}
            className="inline-flex items-center gap-2 rounded-lg bg-pink-600 px-4 py-2.5 text-sm font-semibold text-white hover:bg-pink-500 disabled:opacity-40"
          >
            {busy ? <Loader2 size={16} className="animate-spin" /> : plan.download > 0 ? <Download size={16} /> : <Check size={16} />}
            {t('updateApply')}
          </button>
          <span className="text-xs tabular-nums text-zinc-500">
            {[plan.download > 0 ? t('updateDownload').replace('{size}', bytes(plan.download)) : '', plan.freed > 0 ? t('updateFreed').replace('{size}', bytes(plan.freed)) : ''].filter(Boolean).join(' · ')}
          </span>
          {onLater && !busy && (
            <button type="button" onClick={later} className="ml-auto rounded-lg border border-zinc-300 px-3 py-2.5 text-sm text-zinc-600 hover:border-pink-400 hover:text-pink-600 dark:border-white/15 dark:text-zinc-300">
              {t('updateLater')}
            </button>
          )}
        </div>
      )}
    </div>
  );
};

/** The screen at the first start of a release whose plan is not empty, built like the setup screen. */
export const UpdateScreen: React.FC<{ onLeave: () => void }> = ({ onLeave }) => {
  const { t } = useI18n();
  return (
    <div className="flex h-full w-full justify-center overflow-y-auto bg-white px-5 py-10 dark:bg-suno">
      <div className="w-full max-w-2xl">
        <div className="flex items-center gap-2 text-xs font-medium uppercase tracking-[0.18em] text-pink-500">
          <span className="h-2 w-2 rounded-full bg-pink-500 shadow-[0_0_10px_rgba(236,72,153,0.75)]" />
          {t('updateKicker')}
        </div>
        <h1 className="mt-4 text-3xl font-extrabold tracking-tight text-zinc-900 dark:text-white">{t('updateTitle')}</h1>
        <p className="mb-5 mt-3 max-w-xl text-sm leading-6 text-zinc-500 dark:text-zinc-400">{t('updateSubtitle')}</p>
        <UpdatePlanView onDone={onLeave} onLater={onLeave} />
      </div>
    </div>
  );
};
