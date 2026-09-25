import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { AlertTriangle, Check, Cloud, Cpu, Eye, EyeOff, Loader2, RefreshCw } from 'lucide-react';
import { useI18n } from '../context/I18nContext';

/**
 * Provider matrix.
 *
 * Each capability is configured on its own: music generation can stay local
 * while cover art goes to OpenRouter, or the whole studio can run in the cloud.
 * Two rules keep this honest:
 *
 *   * a capability can only be set to a provider that actually implements it —
 *     local engines come from `/v1/capabilities`, cloud models from the live
 *     OpenRouter catalog, and nothing is hardcoded in this file;
 *   * the API key is stored by the native server, never in browser storage,
 *     and the UI only ever learns whether one is configured.
 */

type CapabilityId = 'music_generation' | 'speech_to_text' | 'prompt_enhancement' | 'cover_art';

const CAPABILITY_KEY: Record<CapabilityId, string> = {
  music_generation: 'capabilityMusic',
  speech_to_text: 'capabilitySpeech',
  prompt_enhancement: 'capabilityAssistant',
  cover_art: 'coverArt',
};

interface EngineDescriptor {
  id: string;
  display_name: string;
  capabilities: CapabilityId[];
  execution_mode: 'local' | 'open_router';
  installed: boolean;
}

interface ProviderSelection {
  capability: CapabilityId;
  mode: 'local' | 'open_router';
  local_engine: string | null;
  cloud_model: string | null;
}

interface CatalogModel {
  id: string;
  name: string;
  capabilities: CapabilityId[];
  pricing?: { prompt?: string | null; completion?: string | null; image?: string | null; request?: string | null } | null;
}

interface OpenRouterSettings {
  configured: boolean;
  source: 'environment' | 'local_store' | null;
  environment_variable: string;
}

const CONTROL =
  'w-full rounded-lg border border-zinc-200 bg-white px-3 py-2 text-sm text-zinc-900 outline-none focus:border-pink-500 disabled:opacity-50 dark:border-white/10 dark:bg-black/20 dark:text-white';

export const ProviderSettings: React.FC = () => {
  const { t } = useI18n();
  const [engines, setEngines] = useState<EngineDescriptor[]>([]);
  const [selections, setSelections] = useState<ProviderSelection[]>([]);
  const [models, setModels] = useState<CatalogModel[]>([]);
  // What the studio would use for a capability the user has not chosen for.
  // The select shows it as the value, so a key is enough to start.
  const [suggested, setSuggested] = useState<Partial<Record<CapabilityId, string>>>({});
  const [settings, setSettings] = useState<OpenRouterSettings | null>(null);
  const [apiKey, setApiKey] = useState('');
  const [showKey, setShowKey] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const load = useCallback(async () => {
    setError(null);
    try {
      const [capabilities, configuration, openRouter] = await Promise.all([
        fetch('/v1/capabilities').then(response => response.json()),
        fetch('/v1/configuration').then(response => response.json()),
        fetch('/v1/openrouter/settings').then(response => response.json()),
      ]);
      setEngines(capabilities.engines ?? []);
      setSelections(configuration.selections ?? []);
      setSettings(openRouter);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'Could not read the studio configuration.');
    }
  }, []);

  const loadCatalog = useCallback(async (refresh: boolean) => {
    setBusy('catalog');
    setError(null);
    try {
      const response = await fetch(refresh ? '/v1/openrouter/catalog/refresh' : '/v1/openrouter/catalog', {
        method: refresh ? 'POST' : 'GET',
      });
      const body = await response.json().catch(() => null);
      if (!response.ok) throw new Error(body?.error || `OpenRouter catalog request failed (${response.status})`);
      setModels(body?.models ?? []);
      setSuggested(body?.suggested ?? {});
      if (refresh) setNotice(`${t('refreshCatalog')}: ${(body?.models ?? []).length}`);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'Could not read the OpenRouter catalog.');
    } finally {
      setBusy(null);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  // The catalogue is OpenRouter's, and reading it needs OpenRouter's key. With
  // no key stored this used to spin anyway and then report a failure nobody
  // could act on; most people have no key at all and never will.
  useEffect(() => {
    if (settings?.configured) void loadCatalog(false);
  }, [settings?.configured, loadCatalog]);

  // A suggestion shown in the select is not a choice anyone made: until it is
  // saved, the capability has no model and the feature refuses to run while the
  // panel looks configured. Write it down once, so what is shown is what is
  // used.
  useEffect(() => {
    if (selections.length === 0 || Object.keys(suggested).length === 0) return;
    const filled = selections.map(selection =>
      selection.execution_mode === 'open_router' && !selection.cloud_model && suggested[selection.capability]
        ? { ...selection, cloud_model: suggested[selection.capability] as string }
        : selection,
    );
    if (filled.some((selection, index) => selection.cloud_model !== selections[index].cloud_model)) {
      void persist(filled);
    }
  }, [selections, suggested]);

  const localEnginesFor = useCallback(
    (capability: CapabilityId) =>
      engines.filter(engine => engine.execution_mode === 'local' && engine.capabilities.includes(capability)),
    [engines],
  );

  const cloudModelsFor = useCallback(
    (capability: CapabilityId) => models.filter(model => model.capabilities.includes(capability)),
    [models],
  );


  const persist = async (next: ProviderSelection[]) => {
    setSelections(next);
    setBusy('configuration');
    try {
      const response = await fetch('/v1/configuration', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ selections: next }),
      });
      if (!response.ok) throw new Error(`Saving the configuration failed (${response.status})`);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'Could not save the configuration.');
      await load();
    } finally {
      setBusy(null);
    }
  };

  const update = (capability: CapabilityId, patch: Partial<ProviderSelection>) =>
    void persist(selections.map(selection => (selection.capability === capability ? { ...selection, ...patch } : selection)));

  const saveKey = async (value: string | null) => {
    setBusy('key');
    setError(null);
    setNotice(null);
    try {
      const response = await fetch('/v1/openrouter/settings', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ api_key: value }),
      });
      const body = await response.json().catch(() => null);
      if (!response.ok) throw new Error(body?.error || `Storing the key failed (${response.status})`);
      setSettings(body);
      setApiKey('');
      setNotice(value ? t('keyStored') : t('keyRemoved'));
      await loadCatalog(true);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'Could not store the API key.');
    } finally {
      setBusy(null);
    }
  };

  const cloudReady = settings?.configured === true;
  const catalogCount = useMemo(() => models.length, [models]);

  return (
    <div className="space-y-5">
      <section className="rounded-xl border border-zinc-200 p-4 dark:border-white/10">
        <div className="flex items-start justify-between gap-3">
          <div>
            <h4 className="flex items-center gap-2 text-sm font-semibold text-zinc-900 dark:text-white">
              <Cloud size={16} className="text-pink-500" /> OpenRouter
            </h4>
            <p className="mt-1 text-xs leading-5 text-zinc-500 dark:text-zinc-400">
              {t('openRouterIntro')}
            </p>
          </div>
          <span className={`shrink-0 rounded-full px-2.5 py-1 text-[10px] font-semibold ${cloudReady ? 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-300' : 'bg-zinc-500/10 text-zinc-500'}`}>
            {cloudReady ? (settings?.source === 'environment' ? t('keyFromEnvironment') : t('keyConfigured')) : t('noKey')}
          </span>
        </div>

        {settings?.source !== 'environment' && (
          <div className="mt-3 flex flex-wrap gap-2">
            <div className="relative min-w-[220px] flex-1">
              <input
                type={showKey ? 'text' : 'password'}
                value={apiKey}
                onChange={event => setApiKey(event.target.value)}
                placeholder={cloudReady ? 'Replace the stored key…' : 'sk-or-…'}
                autoComplete="off"
                className={`${CONTROL} pr-10`}
              />
              <button
                type="button"
                onClick={() => setShowKey(value => !value)}
                className="absolute right-2 top-1/2 -translate-y-1/2 text-zinc-400 hover:text-pink-500"
                title={showKey ? 'Hide' : 'Show'}
              >
                {showKey ? <EyeOff size={15} /> : <Eye size={15} />}
              </button>
            </div>
            <button
              type="button"
              disabled={!apiKey.trim() || busy === 'key'}
              onClick={() => void saveKey(apiKey)}
              className="rounded-lg bg-gradient-to-r from-orange-500 to-pink-600 px-4 py-2 text-xs font-bold text-white disabled:opacity-50"
            >
              {busy === 'key' ? <Loader2 size={14} className="animate-spin" /> : t('saveKey')}
            </button>
            {cloudReady && (
              <button
                type="button"
                disabled={busy === 'key'}
                onClick={() => void saveKey(null)}
                className="rounded-lg border border-zinc-300 px-3 py-2 text-xs font-semibold text-zinc-600 hover:border-rose-400 hover:text-rose-600 dark:border-white/15 dark:text-zinc-300"
              >
                {t('removeKey')}
              </button>
            )}
          </div>
        )}
        {settings?.source === 'environment' && (
          <p className="mt-3 rounded-lg bg-zinc-500/10 px-3 py-2 text-xs text-zinc-600 dark:text-zinc-300">
            {settings.environment_variable} is set for this process and takes priority. Unset it to manage the key here.
          </p>
        )}

        <div className="mt-3 flex items-center gap-3">
          <button
            type="button"
            onClick={() => void loadCatalog(true)}
            disabled={busy === 'catalog'}
            className="inline-flex items-center gap-2 rounded-lg border border-zinc-300 px-3 py-2 text-xs font-semibold text-zinc-700 hover:border-pink-400 hover:text-pink-600 disabled:opacity-50 dark:border-white/15 dark:text-zinc-200"
          >
            <RefreshCw size={13} className={busy === 'catalog' ? 'animate-spin' : ''} /> {t('refreshCatalog')}
          </button>
          <span className="text-xs text-zinc-500">{catalogCount > 0 ? `${catalogCount}` : t('catalogNotLoaded')}</span>
        </div>
      </section>

      <section className="space-y-3">
        <h4 className="flex items-center gap-2 text-sm font-semibold text-zinc-900 dark:text-white">
          <Cpu size={16} className="text-pink-500" /> {t('capabilitiesSection')}
        </h4>
        {/* Music and cover art only. Where the writing assistant and the
            karaoke recogniser run is chosen where they are installed, on the
            models page - and this page wrote the very same settings, so the two
            screens could disagree about a decision that has one answer. */}
        {selections.filter(selection => selection.capability === 'music_generation' || selection.capability === 'cover_art').map(selection => {
          const local = localEnginesFor(selection.capability);
          const cloud = cloudModelsFor(selection.capability);
          const hasLocal = local.length > 0;
          return (
            <div key={selection.capability} className="rounded-xl border border-zinc-200 p-3 dark:border-white/10">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <span className="text-sm font-medium text-zinc-800 dark:text-zinc-100">{t(CAPABILITY_KEY[selection.capability])}</span>
                <div className="flex rounded-lg border border-zinc-200 p-0.5 dark:border-white/10">
                  {(['local', 'open_router'] as const).map(mode => {
                    // Only a capability with no local implementation at all
                    // stays out of reach; "not installed yet" is a thing to
                    // fix here, not a reason to lock the button.
                    const disabled = mode === 'local' ? local.length === 0 : !cloudReady;
                    return (
                      <button
                        key={mode}
                        type="button"
                        disabled={disabled || busy === 'configuration'}
                        title={disabled ? (mode === 'local' ? t('noLocalEngineForCapability') : t('configureKeyFirst')) : undefined}
                        onClick={() => update(selection.capability, {
                          mode,
                          local_engine: mode === 'local' ? (selection.local_engine ?? local[0]?.id ?? null) : null,
                          cloud_model: mode === 'open_router' ? selection.cloud_model : null,
                        })}
                        className={`rounded-md px-3 py-1 text-xs font-semibold transition-colors disabled:cursor-not-allowed disabled:opacity-40 ${
                          selection.mode === mode ? 'bg-zinc-900 text-white dark:bg-white dark:text-zinc-900' : 'text-zinc-500'
                        }`}
                      >
                        {mode === 'local' ? t('modeLocal') : 'OpenRouter'}
                      </button>
                    );
                  })}
                </div>
              </div>

              <div className="mt-2.5">
                {/* Installing is not this page's job. A capability was
                    installable from three places - here, its own settings page
                    and the download panel - each with its own button, its own
                    progress and its own idea of what "installed" meant. This
                    page chooses where a capability runs; the download panel
                    installs it. */}
                {selection.mode === 'local' ? (
                  hasLocal ? (
                    <select
                      value={selection.local_engine ?? ''}
                      disabled={busy === 'configuration'}
                      onChange={event => update(selection.capability, { local_engine: event.target.value })}
                      className={CONTROL}
                    >
                      {local.map(engine => (
                        <option key={engine.id} value={engine.id}>
                          {engine.display_name}{engine.installed ? '' : ` — ${t('notInstalledSuffix')}`}
                        </option>
                      ))}
                    </select>
                  ) : (
                    <p className="flex items-center gap-2 text-xs text-amber-600 dark:text-amber-300">
                      <AlertTriangle size={13} /> {t('noLocalEngineForCapability')}
                    </p>
                  )
                ) : cloud.length > 0 ? (
                  <select
                    value={selection.cloud_model ?? suggested[selection.capability] ?? ''}
                    disabled={busy === 'configuration'}
                    onChange={event => update(selection.capability, { cloud_model: event.target.value || null })}
                    className={CONTROL}
                  >
                    <option value="">{t('chooseCatalogModel')}</option>
                    {cloud.map(model => (
                      <option key={model.id} value={model.id}>{model.name}</option>
                    ))}
                  </select>
                ) : (
                  <p className="flex items-center gap-2 text-xs text-amber-600 dark:text-amber-300">
                    <AlertTriangle size={13} /> {t('noCatalogModelForCapability')}
                  </p>
                )}
              </div>
            </div>
          );
        })}
      </section>

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
