import React, { useEffect, useState } from 'react';
import { Check, Loader2 } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { loadNativeOpenRouterCatalog, refreshNativeOpenRouterCatalog, type NativeOpenRouterModel } from '../services/nativeOpenRouter';

/**
 * The parts of karaoke timing that are not the download.
 *
 * Which recogniser, what it runs on, which model, and the button that installs
 * or removes the set are `OptionalGroup` on the models page. What is left is
 * whether timings are wanted at all, and - for the cloud recogniser - which
 * model on OpenRouter does the listening.
 *
 * This was a page with its own recogniser tabs, its own device picker and an
 * `OptionalGroup` underneath repeating both.
 */

const INPUT =
  'w-full rounded-lg border border-zinc-200 bg-white px-3 py-2 text-sm text-zinc-900 outline-none focus:border-pink-400 dark:border-white/10 dark:bg-black/20 dark:text-white';

export const KaraokeExtras: React.FC<{ engine: string }> = ({ engine }) => {
  const { t } = useI18n();

  const [enabled, setEnabled] = useState(false);
  const [openRouterModel, setOpenRouterModel] = useState('');
  const [catalog, setCatalog] = useState<NativeOpenRouterModel[]>([]);
  const [busy, setBusy] = useState<'save' | 'catalog' | null>(null);
  const [message, setMessage] = useState<{ tone: 'ok' | 'error'; text: string } | null>(null);

  useEffect(() => {
    void loadNativeOpenRouterCatalog()
      .then((models) => setCatalog(models.filter((model) => model.capabilities.includes('speech_to_text'))))
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    void fetch('/v1/karaoke/status')
      .then((response) => (response.ok ? response.json() : null))
      .then((status: { enabled?: boolean; openrouter_model?: string | null } | null) => {
        if (!status) return;
        setEnabled(Boolean(status.enabled));
        setOpenRouterModel(status.openrouter_model ?? '');
      })
      .catch(() => undefined);
  }, []);

  /// The recogniser and the device belong to the control above; sending them
  /// from here would overwrite whatever it just saved.
  const write = async (body: object) => {
    const response = await fetch('/v1/karaoke/status', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
    if (!response.ok) {
      const failure = await response.json().catch(() => null);
      throw new Error(failure?.error || String(response.status));
    }
  };

  const toggle = async (next: boolean) => {
    setEnabled(next);
    setMessage(null);
    await write({ enabled: next }).catch((reason: Error) => setMessage({ tone: 'error', text: reason.message }));
  };

  const save = async () => {
    setBusy('save');
    setMessage(null);
    try {
      await write({ openrouter_model: openRouterModel.trim() || null });
      setMessage({ tone: 'ok', text: t('assistantSaved') });
    } catch (reason) {
      setMessage({ tone: 'error', text: reason instanceof Error ? reason.message : String(reason) });
    } finally {
      setBusy(null);
    }
  };

  const loadCatalog = async () => {
    setBusy('catalog');
    setMessage(null);
    try {
      const models = await refreshNativeOpenRouterCatalog();
      setCatalog(models.filter((model) => model.capabilities.includes('speech_to_text')));
    } catch (reason) {
      setMessage({ tone: 'error', text: reason instanceof Error ? reason.message : String(reason) });
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="space-y-2">
      <label className="flex w-fit cursor-pointer items-center gap-2 rounded-lg px-1 py-0.5 text-xs font-medium text-zinc-700 transition-colors hover:text-pink-600 dark:text-zinc-200 dark:hover:text-pink-400">
        <input type="checkbox" checked={enabled} onChange={(event) => void toggle(event.target.checked)} className="h-4 w-4 accent-pink-500" />
        {t('karaokeEnable')}
      </label>

      {engine === 'open_router' && (
        <div className="space-y-1">
          <div className="flex items-center gap-2">
            <select value={openRouterModel} onChange={(event) => setOpenRouterModel(event.target.value)} className={INPUT}>
              <option value="">{t('assistantPickModel')}</option>
              {openRouterModel && !catalog.some((model) => model.id === openRouterModel) && (
                <option value={openRouterModel}>{openRouterModel}</option>
              )}
              {catalog.map((model) => <option key={model.id} value={model.id}>{model.name}</option>)}
            </select>
            <button
              type="button"
              onClick={() => void loadCatalog()}
              disabled={busy !== null}
              className="shrink-0 rounded-lg border border-zinc-200 px-3 py-2 text-sm font-medium text-zinc-600 transition-colors hover:border-pink-400 hover:text-pink-600 disabled:opacity-50 dark:border-white/10 dark:text-zinc-300"
            >
              {busy === 'catalog' ? <Loader2 size={16} className="animate-spin" /> : t('refresh')}
            </button>
            <button
              type="button"
              onClick={() => void save()}
              disabled={busy !== null}
              className="inline-flex shrink-0 items-center gap-2 rounded-lg border border-zinc-300 px-3 py-2 text-xs font-semibold text-zinc-700 transition-colors hover:border-pink-400 hover:text-pink-600 disabled:opacity-50 dark:border-white/15 dark:text-zinc-200"
            >
              {busy === 'save' ? <Loader2 size={14} className="animate-spin" /> : <Check size={14} />}
              {t('save')}
            </button>
          </div>
          <p className="text-[11px] leading-4 text-zinc-500">{t('karaokeOpenRouterHint')}</p>
        </div>
      )}

      {message && (
        <span className={`text-xs ${message.tone === 'ok' ? 'text-emerald-600 dark:text-emerald-300' : 'text-red-600 dark:text-red-300'}`}>
          {message.text}
        </span>
      )}
    </div>
  );
};
