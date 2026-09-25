import React, { useEffect, useState } from 'react';
import { Check, ChevronDown, Loader2, ScrollText } from 'lucide-react';
import { useI18n } from '../context/I18nContext';

/**
 * The engine coming up.
 *
 * This is not the first run and must not look like it: the models are on disk,
 * nothing is being downloaded, and there is nothing for the user to decide.
 * All that is happening is a process starting - a few seconds - so the screen
 * says which step it is on, moves while it moves, and shows the engine's own
 * log for anyone who wants to know what it is doing.
 */
export const EngineStarting: React.FC<{ onReady?: () => void }> = ({ onReady }) => {
  const { t } = useI18n();
  const [lines, setLines] = useState<string[]>([]);
  const [open, setOpen] = useState(true);
  const [seconds, setSeconds] = useState(0);
  const [failed, setFailed] = useState<string | null>(null);
  const [foreign, setForeign] = useState<string | null>(null);
  // The engine binary is linked against CUDA libraries that are downloaded
  // rather than shipped - half a gigabyte of them. While they arrive the
  // engine cannot start, and this screen has to say so with a number.
  const [runtime, setRuntime] = useState<{ downloading: boolean; downloaded: number; total: number; error?: string | null } | null>(null);

  useEffect(() => {
    const started = Date.now();
    const tick = window.setInterval(() => setSeconds(Math.round((Date.now() - started) / 1000)), 1000);
    return () => window.clearInterval(tick);
  }, []);

  useEffect(() => {
    const read = async () => {
      await fetch('/v1/engine/logs')
        .then((response) => (response.ok ? response.json() : Promise.reject(new Error())))
        .then((body: { lines?: string[] }) => setLines((body.lines ?? []).slice(-40)))
        .catch(() => undefined);
      await fetch('/health')
        .then((response) => (response.ok ? response.json() : Promise.reject(new Error())))
        .then((body: { music_engine?: { reachable?: boolean }; engine_bundle_present?: boolean; service_executable?: string }) => {
          if (body.music_engine?.reachable) onReady?.();
          // A service with no engine bundle is not this application's service:
          // another copy of the studio holds the port, and waiting for it to
          // start an engine it does not have is waiting forever.
          setForeign(body.engine_bundle_present === false ? body.service_executable ?? '' : null);
        })
        .catch(() => undefined);
      await fetch('/setup/status')
        .then((response) => (response.ok ? response.json() : Promise.reject(new Error())))
        .then((body: { engine_runtime?: { ready?: boolean; downloading?: boolean; downloaded_bytes?: number; total_bytes?: number; error?: string | null } }) => {
          const state = body.engine_runtime;
          setRuntime(
            state && state.ready === false
              ? {
                  downloading: state.downloading === true,
                  downloaded: state.downloaded_bytes ?? 0,
                  total: state.total_bytes ?? 0,
                  error: state.error,
                }
              : null,
          );
        })
        .catch(() => undefined);
    };
    void read();
    const timer = window.setInterval(() => void read(), 1000);
    return () => window.clearInterval(timer);
  }, [onReady]);

  // Starting takes about three seconds here. Much longer than that means
  // something is wrong, and silence would be the worst answer - unless half a
  // gigabyte of libraries is on its way, which legitimately takes minutes.
  useEffect(() => {
    if (seconds < 45 || runtime) return;
    setFailed(t('engineSlowToStart'));
  }, [seconds, t, runtime]);

  const percent = runtime && runtime.total > 0 ? Math.min(100, Math.round((runtime.downloaded / runtime.total) * 100)) : 0;
  const steps = runtime
    ? [
        { label: `${t('stepEngineLibraries')} — ${percent}%`, done: false, active: true },
        { label: t('stepEngine'), done: false, active: false },
        { label: t('stepReady'), done: false, active: false },
      ]
    : [
        { label: t('stepModels'), done: true, active: false },
        { label: t('stepEngine'), done: false, active: true },
        { label: t('stepReady'), done: false, active: false },
      ];

  return (
    <div className="flex h-full w-full items-center justify-center bg-white px-5 py-10 dark:bg-suno">
      <div className="w-full max-w-md">
        <div className="flex items-center gap-2 text-xs font-medium uppercase tracking-[0.18em] text-pink-500">
          <span className="h-2 w-2 animate-pulse rounded-full bg-pink-500 shadow-[0_0_10px_rgba(236,72,153,0.75)]" />
          {t('engineStartingBadge')}
        </div>
        <h1 className="mt-3 text-2xl font-bold tracking-tight text-zinc-900 dark:text-white">{t('engineStartingTitle')}</h1>
        <p className="mt-2 text-sm leading-6 text-zinc-500 dark:text-zinc-400">{t('engineStartingHint')}</p>

        <div className="mt-6 space-y-2">
          {steps.map((step) => (
            <div key={step.label} className="flex items-center gap-2.5">
              <span className={`grid h-5 w-5 shrink-0 place-items-center rounded-full ${step.done ? 'bg-pink-500 text-white' : step.active ? 'text-pink-500' : 'text-zinc-400'}`}>
                {step.done ? <Check size={12} strokeWidth={3} /> : step.active ? <Loader2 size={14} className="animate-spin" /> : <span className="h-1.5 w-1.5 rounded-full bg-current" />}
              </span>
              <span className={`text-sm ${step.active ? 'font-medium text-zinc-900 dark:text-white' : 'text-zinc-500 dark:text-zinc-400'}`}>{step.label}</span>
              {step.active && <span className="ml-auto text-xs tabular-nums text-zinc-400">{seconds} {t('secondsShort')}</span>}
            </div>
          ))}
        </div>

        <div className="mt-5 h-1.5 w-full overflow-hidden rounded-full bg-zinc-200 dark:bg-black/30">
          <div
            className={`h-full rounded-full bg-gradient-to-r from-orange-500 to-pink-500 ${runtime ? 'transition-[width]' : 'w-1/3 animate-pulse'}`}
            style={runtime ? { width: `${Math.max(2, percent)}%` } : undefined}
          />
        </div>

        {runtime && (
          <p className="mt-3 text-xs leading-5 text-zinc-500 dark:text-zinc-400">
            {t('engineLibrariesHint')}
            {runtime.total > 0 && (
              <span className="ml-1 tabular-nums">
                {(runtime.downloaded / 1e9).toFixed(2)} / {(runtime.total / 1e9).toFixed(2)} GB
              </span>
            )}
          </p>
        )}
        {runtime?.error && <p className="mt-2 text-xs leading-5 text-amber-600 dark:text-amber-300">{runtime.error}</p>}

        {foreign !== null && (
          <div className="mt-3 rounded-lg border border-amber-400/40 bg-amber-500/10 px-3 py-2 text-xs leading-5 text-amber-700 dark:text-amber-200">
            {t('engineForeignService')}
            {foreign && <div className="mt-1 break-all font-mono text-[11px] opacity-80">{foreign}</div>}
          </div>
        )}
        {failed && foreign === null && <p className="mt-3 text-xs leading-5 text-amber-600 dark:text-amber-300">{failed}</p>}

        <button
          type="button"
          onClick={() => setOpen((value) => !value)}
          className="mt-4 inline-flex items-center gap-1.5 text-[11px] uppercase tracking-wide text-zinc-500 hover:text-pink-500"
        >
          <ScrollText size={13} />{t('engineLog')}
          <ChevronDown size={13} className={open ? 'rotate-180 transition-transform' : 'transition-transform'} />
        </button>
        {open && (
          <div className="mt-2 max-h-56 overflow-y-auto rounded-lg bg-zinc-50 p-2 font-mono text-[11px] leading-4 text-zinc-600 dark:bg-black/30 dark:text-zinc-400">
            {lines.length === 0 ? <span className="text-zinc-400">—</span> : lines.map((line, index) => <div key={index} className="break-words">{line}</div>)}
          </div>
        )}
      </div>
    </div>
  );
};
