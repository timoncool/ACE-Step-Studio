import React, { useEffect, useState } from 'react';
import { Check, Loader2, Square } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { AgentPanel } from './AgentPanel';
import { loadNativeOpenRouterCatalog, refreshNativeOpenRouterCatalog, type NativeOpenRouterModel } from '../services/nativeOpenRouter';

/**
 * The parts of the writing assistant that are not the download.
 *
 * Which engine it is, what it runs on, which model, and the one button that
 * installs or removes it all live in `OptionalGroup` on the models page. This
 * is what is left over and belongs to nothing else: a GGUF the user already
 * has, the cloud model to send prompts to, the name a self-hosted server calls
 * its model, and the button that unloads the sidecar.
 *
 * It used to be a page of its own, with its own row of engine tabs and an
 * `OptionalGroup` nested inside it - the same capability drawn twice, one
 * inside the other, each with its own idea of what was installed.
 */

interface RuntimeStatus {
  running_model?: string | null;
  active_download?: { error?: string | null } | null;
}

const INPUT =
  'w-full rounded-lg border border-zinc-200 bg-white px-3 py-2 text-sm text-zinc-900 outline-none focus:border-pink-400 dark:border-white/10 dark:bg-black/20 dark:text-white';

export const AssistantExtras: React.FC<{ engine: string }> = ({ engine }) => {
  const { t } = useI18n();

  const [localBaseUrl, setLocalBaseUrl] = useState('');
  const [localModel, setLocalModel] = useState('');
  const [localModels, setLocalModels] = useState<string[]>([]);
  const [localBusy, setLocalBusy] = useState(false);
  const [openRouterModel, setOpenRouterModel] = useState('');
  const [managedPath, setManagedPath] = useState('');
  const [runtime, setRuntime] = useState<RuntimeStatus | null>(null);
  const [catalog, setCatalog] = useState<NativeOpenRouterModel[]>([]);
  const [busy, setBusy] = useState<'save' | 'catalog' | null>(null);
  const [message, setMessage] = useState<{ tone: 'ok' | 'error'; text: string } | null>(null);

  // The catalog is already there; asking the user to press refresh to see it
  // is the program making them do its work.
  useEffect(() => {
    void loadNativeOpenRouterCatalog()
      .then((models) => setCatalog(models.filter((model) => model.capabilities.includes('prompt_enhancement'))))
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    void fetch('/v1/assistant/status')
      .then((response) => (response.ok ? response.json() : null))
      .then((status: { local_base_url?: string | null; local_model?: string | null; openrouter_model?: string | null; managed_path?: string | null } | null) => {
        if (!status) return;
        setLocalBaseUrl(status.local_base_url ?? '');
        setLocalModel(status.local_model ?? '');
        setOpenRouterModel(status.openrouter_model ?? '');
        setManagedPath(status.managed_path ?? '');
      })
      .catch(() => undefined);
  }, []);

  // The local server's settings save themselves as they change - no button to
  // press, and none to forget. Debounced so a URL is stored once it is typed,
  // not once per keystroke.
  const saveLocal = React.useCallback((base: string, model: string) => {
    void fetch('/v1/assistant/status', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ provider: 'local', local_base_url: base.trim() || null, local_model: model.trim() || null }),
    }).catch(() => undefined);
  }, []);
  useEffect(() => {
    if (engine !== 'local') return;
    const timer = window.setTimeout(() => saveLocal(localBaseUrl, localModel), 500);
    return () => window.clearTimeout(timer);
  }, [engine, localBaseUrl, localModel, saveLocal]);

  // The models the server offers, asked of it through the studio so the browser
  // never has to reach another origin. Typing the name by hand was a typo away
  // from a server that answered nothing.
  const fetchLocalModels = React.useCallback(async (base: string) => {
    if (!base.trim()) return;
    setLocalBusy(true);
    try {
      const response = await fetch(`/v1/assistant/local-models?base=${encodeURIComponent(base.trim())}`);
      const body = await response.json().catch(() => null);
      if (response.ok && Array.isArray(body?.models)) {
        setLocalModels(body.models);
        if (!localModel && body.models.length) setLocalModel(body.models[0]);
      }
    } finally {
      setLocalBusy(false);
    }
  }, [localModel]);
  useEffect(() => {
    if (engine === 'local' && localBaseUrl.trim()) void fetchLocalModels(localBaseUrl);
    // Only when the address changes, not on every model edit.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [engine, localBaseUrl]);

  const loadRuntime = React.useCallback(async () => {
    const response = await fetch('/v1/assistant/runtime');
    if (response.ok) setRuntime(await response.json());
  }, []);

  useEffect(() => { void loadRuntime().catch(() => undefined); }, [loadRuntime]);

  const stopSidecar = async () => {
    await fetch('/v1/assistant/runtime/stop', { method: 'POST' }).catch(() => undefined);
    void loadRuntime().catch(() => undefined);
  };

  const loadCatalog = async () => {
    setBusy('catalog');
    setMessage(null);
    try {
      const models = await refreshNativeOpenRouterCatalog();
      setCatalog(models.filter((model) => model.capabilities.includes('prompt_enhancement')));
    } catch (reason) {
      setMessage({ tone: 'error', text: reason instanceof Error ? reason.message : String(reason) });
    } finally {
      setBusy(null);
    }
  };

  /// Only the fields on screen are sent: the engine and the device belong to
  /// the control above, and writing them from here would fight it.
  const save = async () => {
    setBusy('save');
    setMessage(null);
    try {
      const response = await fetch('/v1/assistant/status', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(
          engine === 'open_router'
            ? { openrouter_model: openRouterModel.trim() || null }
            : engine === 'local'
              ? { local_model: localModel.trim() || null }
              : { managed_path: managedPath.trim() || null },
        ),
      });
      const body = await response.json().catch(() => null);
      if (!response.ok) throw new Error(body?.error || String(response.status));

      // The provider page owns which model each capability uses, and the
      // assistant reads it from there. Saving here without updating it left two
      // screens naming different models and the request going to the other one.
      if (engine === 'open_router') {
        const configuration = await fetch('/v1/configuration').then((response) => response.json()).catch(() => null);
        const selections = configuration?.selections;
        if (Array.isArray(selections)) {
          await fetch('/v1/configuration', {
            method: 'PUT',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
              selections: selections.map((selection: { capability: string; cloud_model: string | null }) =>
                selection.capability === 'prompt_enhancement'
                  ? { ...selection, execution_mode: 'open_router', cloud_model: openRouterModel.trim() || null }
                  : selection,
              ),
            }),
          }).catch(() => undefined);
        }
      }
      setMessage({ tone: 'ok', text: t('assistantSaved') });
    } catch (reason) {
      setMessage({ tone: 'error', text: reason instanceof Error ? reason.message : String(reason) });
    } finally {
      setBusy(null);
    }
  };

  if (engine === 'agent') return <AgentPanel />;

  return (
    <div className="space-y-2">
      {engine === 'managed' && (
        <div className="rounded-lg border-2 border-dashed border-zinc-300 p-3 transition-colors hover:border-zinc-400 dark:border-white/10 dark:hover:border-white/20">
          <label className="block text-xs font-medium text-zinc-500 dark:text-zinc-400">{t('assistantOwnFile')}</label>
          <input
            value={managedPath}
            onChange={(event) => setManagedPath(event.target.value)}
            placeholder="D:\models\gemma-4-12b-it-qat-q4_0.gguf"
            className={`${INPUT} mt-1`}
          />
          <p className="mt-1 text-[11px] leading-4 text-zinc-500">{t('assistantOwnFileHint')}</p>
        </div>
      )}

      {engine === 'local' && (
        <div className="space-y-2">
          <label className="block text-xs font-medium text-zinc-500 dark:text-zinc-400">{t('assistantBaseUrl')}</label>
          <input
            value={localBaseUrl}
            onChange={(event) => setLocalBaseUrl(event.target.value)}
            placeholder="http://127.0.0.1:1234/v1"
            className={INPUT}
          />
          <label className="block text-xs font-medium text-zinc-500 dark:text-zinc-400">{t('assistantModel')}</label>
          <div className="flex items-center gap-2">
            {localModels.length > 0 ? (
              <select value={localModel} onChange={(event) => setLocalModel(event.target.value)} className={INPUT}>
                {localModel && !localModels.includes(localModel) && <option value={localModel}>{localModel}</option>}
                {localModels.map((id) => <option key={id} value={id}>{id}</option>)}
              </select>
            ) : (
              <input value={localModel} onChange={(event) => setLocalModel(event.target.value)} placeholder="gemma-3-4b-it" className={INPUT} />
            )}
            <button
              type="button"
              onClick={() => void fetchLocalModels(localBaseUrl)}
              disabled={localBusy || !localBaseUrl.trim()}
              title={t('refresh')}
              className="shrink-0 rounded-lg border border-zinc-200 px-3 py-2 text-sm font-medium text-zinc-600 transition-colors hover:border-pink-400 hover:text-pink-600 disabled:opacity-50 dark:border-white/10 dark:text-zinc-300"
            >
              {localBusy ? <Loader2 size={16} className="animate-spin" /> : t('refresh')}
            </button>
          </div>
          <p className="text-[11px] leading-4 text-zinc-500">{t('assistantLocalHint')}</p>
        </div>
      )}

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
          </div>
          <p className="text-[11px] leading-4 text-zinc-500">{t('assistantOpenRouterHint')}</p>
        </div>
      )}

      <div className="flex items-center gap-3 pt-1">
        {/* The local server saves itself as its fields change, so it needs no
            button; the others still keep their explicit Save. */}
        {engine !== 'local' && (
          <button
            type="button"
            onClick={() => void save()}
            disabled={busy !== null}
            className="inline-flex items-center gap-2 rounded-lg border border-zinc-300 px-3 py-1.5 text-xs font-semibold text-zinc-700 transition-colors hover:border-pink-400 hover:text-pink-600 disabled:opacity-50 dark:border-white/15 dark:text-zinc-200"
          >
            {busy === 'save' ? <Loader2 size={14} className="animate-spin" /> : <Check size={14} />}
            {t('save')}
          </button>
        )}
        {runtime?.running_model && (
          <button
            type="button"
            onClick={() => void stopSidecar()}
            className="inline-flex items-center gap-2 rounded-lg border border-zinc-200 px-3 py-1.5 text-xs font-medium text-zinc-600 transition-colors hover:border-pink-400 hover:text-pink-600 dark:border-white/10 dark:text-zinc-300"
          >
            <Square size={13} /> {t('assistantUnload')}
          </button>
        )}
        {message && (
          <span className={`text-xs ${message.tone === 'ok' ? 'text-emerald-600 dark:text-emerald-300' : 'text-red-600 dark:text-red-300'}`}>
            {message.text}
          </span>
        )}
        {runtime?.active_download?.error && (
          <span className="text-xs text-red-600 dark:text-red-300">{runtime.active_download.error}</span>
        )}
      </div>
    </div>
  );
};
