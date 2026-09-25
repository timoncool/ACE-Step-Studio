import React, { useCallback, useEffect, useState } from 'react';
import { AlertTriangle, Check, Cpu, Loader2, RotateCw , Terminal } from 'lucide-react';
import { useI18n } from '../context/I18nContext';

/**
 * Launch options for the local engine.
 *
 * These are `mm-server` command-line flags, and upstream reads them once at
 * startup — so saving them restarts the engine. Each one is a real flag, and
 * `--max-batch` in particular is the ceiling on how many songs a single request
 * may render, which is why the create panel clamps to it.
 */

type ComputeBackend = 'auto' | 'cuda' | 'vulkan' | 'cpu';

interface EngineOptions {
  backend: ComputeBackend;
  keep_loaded: boolean;
  max_seq: number | null;
  disable_flash_attention: boolean;
  split_cfg_forwards: boolean;
  clamp_fp16: boolean;
}

const DEFAULTS: EngineOptions = {
  backend: 'auto',
  keep_loaded: false,
  max_seq: null,
  disable_flash_attention: false,
  split_cfg_forwards: false,
  clamp_fp16: false,
};

const CONTROL =
  'w-full rounded-lg border border-zinc-200 bg-white px-3 py-2 text-sm text-zinc-900 outline-none focus:border-pink-500 dark:border-white/10 dark:bg-black/20 dark:text-white';

const Toggle: React.FC<{ label: string; hint: string; checked: boolean; onChange: (value: boolean) => void }> = ({ label, hint, checked, onChange }) => (
  <label className="flex cursor-pointer items-start gap-3 rounded-xl border border-zinc-200 p-3 dark:border-white/10">
    <input type="checkbox" checked={checked} onChange={event => onChange(event.target.checked)} className="mt-0.5 h-4 w-4 accent-pink-500" />
    <span className="min-w-0">
      <span className="block text-sm font-medium text-zinc-800 dark:text-zinc-100">{label}</span>
      <span className="mt-0.5 block text-xs leading-5 text-zinc-500 dark:text-zinc-400">{hint}</span>
    </span>
  </label>
);

export const EngineSettings: React.FC = () => {
  const { t } = useI18n();
  const [options, setOptions] = useState<EngineOptions>(DEFAULTS);
  const [saved, setSaved] = useState<EngineOptions>(DEFAULTS);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const response = await fetch('/engine/options');
      if (!response.ok) throw new Error(`Engine options are unavailable (${response.status})`);
      const body: { options: EngineOptions } = await response.json();
      setOptions(body.options);
      setSaved(body.options);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'Could not read the engine options.');
    }
  }, []);

  useEffect(() => { void load(); }, [load]);

  const dirty = JSON.stringify(options) !== JSON.stringify(saved);

  const save = async () => {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const response = await fetch('/engine/options', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(options),
      });
      const body = await response.json().catch(() => null);
      if (!response.ok) throw new Error(body?.error || `Saving the engine options failed (${response.status})`);
      setSaved(body.options);
      setNotice(body.engine_restarted
        ? t('engineOptionsSavedRestarted')
        : t('engineOptionsSavedLater'));
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'Saving the engine options failed.');
    } finally {
      setBusy(false);
    }
  };

  // What the engine is saying. It used to be on a page of its own; it belongs
  // with the flags that started the process.
  const [logs, setLogs] = useState<string[]>([]);
  const [logsOpen, setLogsOpen] = useState(false);
  useEffect(() => {
    if (!logsOpen) return;
    const read = () => void fetch('/v1/engine/logs')
      .then(response => (response.ok ? response.json() : Promise.reject(new Error())))
      .then((body: { lines?: string[] }) => setLogs((body.lines ?? []).slice(-200)))
      .catch(() => undefined);
    read();
    const timer = window.setInterval(read, 2000);
    return () => window.clearInterval(timer);
  }, [logsOpen]);

  const restart = async () => {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const response = await fetch('/engine/restart', { method: 'POST' });
      const body = await response.json().catch(() => null);
      if (!response.ok) throw new Error(body?.error || `Restarting the engine failed (${response.status})`);
      setNotice(t('engineRestarted'));
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'Restarting the engine failed.');
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="space-y-3">
      <h4 className="flex items-center gap-2 text-sm font-semibold text-zinc-900 dark:text-white">
        <Cpu size={16} className="text-pink-500" /> {t('localEngine')}
      </h4>

      <div className="rounded-xl border border-zinc-200 p-3 dark:border-white/10">
        <span className="block text-sm font-medium text-zinc-800 dark:text-zinc-100">{t('computeBackendLabel')}</span>
        <div className="mt-2 grid grid-cols-2 gap-1 rounded-lg bg-zinc-100 p-1 sm:grid-cols-4 dark:bg-black/30" role="radiogroup" aria-label={t('computeBackendLabel')}>
          {(['auto', 'cuda', 'vulkan', 'cpu'] as const).map(value => (
            <button
              key={value}
              type="button"
              role="radio"
              aria-checked={options.backend === value}
              onClick={() => setOptions(current => ({ ...current, backend: value }))}
              className={`rounded-md px-2 py-1.5 text-xs font-semibold transition-all ${options.backend === value ? 'bg-white text-black shadow-sm dark:bg-zinc-700 dark:text-white' : 'text-zinc-500 hover:text-zinc-800 dark:hover:text-zinc-200'}`}
            >
              {t(`computeBackend_${value}`)}
            </button>
          ))}
        </div>
        <span className="mt-2 block text-xs leading-5 text-zinc-500 dark:text-zinc-400">{t(`computeBackendHint_${options.backend}`)}</span>
      </div>

      <Toggle
        label={t('keepLoadedLabel')}
        hint={t('keepLoadedHint')}
        checked={options.keep_loaded}
        onChange={value => setOptions(current => ({ ...current, keep_loaded: value }))}
      />

      {/* The batch ceiling is not here and is not sent: it reserves KV cache
          for the whole batch when the weights load - memory paid for on every
          single-song generation, which is all but every generation - and the
          engine takes it only as a launch flag. */}
      <div className="grid gap-3 sm:grid-cols-2">
        <label className="block text-xs font-medium text-zinc-600 dark:text-zinc-300">
          <span className="mb-1.5 block">{t('maxSeqLabel')}</span>
          <input
            type="number"
            min={512}
            step={512}
            placeholder={t('maxSeqHint')}
            value={options.max_seq ?? ''}
            onChange={event => {
              const value = Number(event.target.value);
              setOptions(current => ({ ...current, max_seq: event.target.value === '' || !Number.isFinite(value) ? null : value }));
            }}
            className={CONTROL}
          />
          <span className="mt-1 block font-normal text-zinc-500">{t('maxSeqHint')}</span>
        </label>
      </div>

      <details className="rounded-xl border border-zinc-200 p-3 dark:border-white/10">
        <summary className="cursor-pointer text-xs font-semibold uppercase tracking-wide text-zinc-500">{t('diagnostics')}</summary>
        <div className="mt-3 space-y-3">
          <Toggle
            label={t('noFlashAttentionLabel')}
            hint={t('noFlashAttentionHint')}
            checked={options.disable_flash_attention}
            onChange={value => setOptions(current => ({ ...current, disable_flash_attention: value }))}
          />
          <Toggle
            label={t('splitCfgLabel')}
            hint={t('splitCfgHint')}
            checked={options.split_cfg_forwards}
            onChange={value => setOptions(current => ({ ...current, split_cfg_forwards: value }))}
          />
          <Toggle
            label={t('clampFp16Label')}
            hint={t('clampFp16Hint')}
            checked={options.clamp_fp16}
            onChange={value => setOptions(current => ({ ...current, clamp_fp16: value }))}
          />
        </div>
      </details>

      <div className="flex flex-wrap items-center gap-2">
        <button
          type="button"
          onClick={() => void save()}
          disabled={!dirty || busy}
          className="inline-flex items-center gap-2 rounded-lg bg-gradient-to-r from-orange-500 to-pink-600 px-4 py-2 text-xs font-bold text-white disabled:opacity-50"
        >
          {busy ? <Loader2 size={14} className="animate-spin" /> : null} {t('saveAndRestartEngine')}
        </button>
        <button
          type="button"
          onClick={() => void restart()}
          disabled={busy}
          className="inline-flex items-center gap-2 rounded-lg border border-zinc-300 px-3 py-2 text-xs font-semibold text-zinc-700 hover:border-pink-400 hover:text-pink-600 disabled:opacity-50 dark:border-white/15 dark:text-zinc-200"
        >
          <RotateCw size={13} /> {t('restartEngine')}
        </button>
      </div>

      <div className="rounded-xl border border-zinc-200 dark:border-white/10">
        <button
          type="button"
          onClick={() => setLogsOpen(open => !open)}
          className="flex w-full items-center gap-2 px-3 py-2 text-[11px] font-bold uppercase tracking-wide text-zinc-500 hover:text-pink-500"
        >
          <Terminal size={13} /> {t('engineLog')}
        </button>
        {logsOpen && (
          <div className="max-h-64 overflow-y-auto border-t border-zinc-200 bg-zinc-50 p-3 font-mono text-[11px] leading-4 text-zinc-600 dark:border-white/10 dark:bg-black/30 dark:text-zinc-400">
            {logs.length === 0 ? <span className="text-zinc-400">—</span> : logs.map((line, index) => <div key={index} className="break-words">{line}</div>)}
          </div>
        )}
      </div>

      {notice && (
        <p className="flex items-center gap-2 rounded-lg bg-emerald-500/10 px-3 py-2 text-xs text-emerald-700 dark:text-emerald-300">
          <Check size={14} /> {notice}
        </p>
      )}
      {error && (
        <p role="alert" className="flex items-center gap-2 rounded-lg bg-rose-500/10 px-3 py-2 text-xs text-rose-700 dark:text-rose-300">
          <AlertTriangle size={14} /> {error}
        </p>
      )}
    </div>
  );
};
