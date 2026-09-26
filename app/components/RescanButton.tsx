import React, { useState } from 'react';
import { Loader2, RefreshCw } from 'lucide-react';
import { useI18n } from '../context/I18nContext';

/**
 * Finds every model and LoRA on disk again. The engine reads its folders when
 * it starts, so a file put there by hand is found by restarting it; the
 * service waits for the songs in flight before it does.
 */
export const RescanButton: React.FC<{ onDone?: () => void }> = ({ onDone }) => {
  const { t } = useI18n();
  const tt = t as unknown as (key: string) => string;
  const [busy, setBusy] = useState(false);

  const rescan = async () => {
    setBusy(true);
    try {
      const response = await fetch('/v1/resources/rescan', { method: 'POST' });
      const body: { error?: string; restarted?: boolean; models?: { dit?: string[] }; adapters?: string[] } | null = await response.json().catch(() => null);
      if (!response.ok) throw new Error(body?.error || `${response.status}`);
      for (const name of ['studio:adapters-changed', 'studio:models-changed', 'studio:settings-changed']) window.dispatchEvent(new CustomEvent(name));
      onDone?.();
      const message = body?.restarted
        ? tt('rescanFound').replace('{dit}', String(body.models?.dit?.length ?? 0)).replace('{lora}', String(body.adapters?.length ?? 0))
        : tt('rescanEngineOff');
      window.dispatchEvent(new CustomEvent('studio:toast', { detail: { message, type: 'success' } }));
    } catch (problem) {
      const reason = problem instanceof Error ? problem.message : String(problem);
      window.dispatchEvent(new CustomEvent('studio:toast', { detail: { message: `${tt('rescanFailed')}: ${reason}`, type: 'error' } }));
    } finally {
      setBusy(false);
    }
  };

  return (
    <button
      type="button"
      onClick={() => void rescan()}
      disabled={busy}
      title={tt('rescanHint')}
      className="inline-flex items-center gap-2 rounded-lg border border-zinc-200 px-3 py-1.5 text-xs font-semibold text-zinc-700 transition-colors hover:border-pink-400 hover:text-pink-600 disabled:opacity-50 dark:border-white/10 dark:text-zinc-200"
    >
      {busy ? <Loader2 size={13} className="animate-spin" /> : <RefreshCw size={13} />}
      {busy ? tt('rescanRunning') : tt('rescanResources')}
    </button>
  );
};
