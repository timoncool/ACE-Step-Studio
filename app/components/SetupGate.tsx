import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Check, ChevronDown, Download, FolderDown, FolderOpen, Loader2, Square, Trash2, X } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { RescanButton } from './RescanButton';
import { DevicePicker, type Device } from './DevicePicker';
import { AssistantExtras } from './AssistantSettings';
import { KaraokeExtras } from './KaraokeSettings';
import {
  componentKindLabel,
  componentPrecision,
  componentVariant,
  profileLabel,
  componentsByKind,
  completeCustomComponentIds,
  selectedComponentBytes,
  type ModelComponent,
} from '../services/modelCatalog';

/**
 * First run.
 *
 * The engine needs four components and refuses an incomplete set, so the page
 * is built around that: one row per role, one quantisation chosen inside each,
 * everything preselected for the detected card and marked installed or
 * missing. The optional extras - a writing assistant, karaoke timings - are
 * listed underneath, plainly marked optional and never preselected, because
 * the studio makes music without either of them.
 */

type DownloadJob = {
  id: string;
  profile_id: string | null;
  component_ids: string[];
  status: 'downloading' | 'completed' | 'cancelled' | 'failed';
  downloaded_bytes: number;
  total_bytes: number;
  error: string | null;
};

type SetupStatus = {
  ready: boolean;
  selected_profile_id?: string | null;
  selected_component_ids?: string[] | null;
  hardware?: { gpuName?: string; totalVramGb?: number; recommended?: string; reason?: string };
  engine_ready: boolean;
  engine_id: string;
  first_run: boolean;
  download_pending: number;
  recommended_profile_id: string;
  active: DownloadJob | null;
  installed_components: string[];
  data_directory?: string | null;
  portable?: boolean;
};

type Profile = {
  id: string;
  label: string;
  backend: string;
  installable: boolean;
  recommended: boolean;
  components: string[];
  total_bytes: number;
};

type Catalog = { engine_id: string; recommended_profile_id: string; profiles: Profile[]; components: ModelComponent[] };

type OptionalAsset = { id: string; label: string; bytes: number; note: string; installed: boolean; kind: 'model' | 'runtime'; vram_gb?: number | null };
type SetProgress = { bytes: number; installed_bytes: number; ready: boolean; files: number };
type OptionalStatus = {
  assets: OptionalAsset[];
  /// What the chosen engine is made of, as the server counts it: the panel
  /// used to add up every file in the group and show whichever download the
  /// shared counter happened to hold.
  set?: SetProgress;
  active_download?: { asset_id: string; downloaded_bytes: number; total_bytes: number; done: boolean; error?: string | null } | null;
};

const bytes = (value: number) => (value < 1024 ** 3 ? `${Math.round(value / 1024 ** 2)} MB` : `${(value / 1024 ** 3).toFixed(1)} GB`);

const errorMessage = async (response: Response) => {
  const body = await response.json().catch(() => null);
  return body?.error || `Request failed (${response.status})`;
};

/** A recogniser or backend: which one, what it runs on, and one button. */
type EngineChoice = { id: string; label: string; device?: boolean };

/**
 * One optional capability.
 *
 * A capability is a choice of engine and a choice of what it runs on - not a
 * list of files. Parakeet is six downloads and a runtime, Whisper is two, and
 * which two depends on the card; nobody should have to work that out from file
 * names. `engines` turns the group into that choice, and the whole set is
 * installed and removed by one button.
 */
export const OptionalGroup: React.FC<{
  title: string;
  purpose: string;
  statusUrl: string;
  installUrl: string;
  removeUrl?: string;
  engines?: EngineChoice[];
  /** Where the chosen engine and device are saved, when they are a setting. */
  settingsUrl?: string;
  /** Whether one of the engines is a server the user runs themselves. */
  serverField?: boolean;
  /** Where to stop a running download. Without it the only way was to quit. */
  cancelUrl?: string;
  /**
   * On a settings page this is the page, so it drops its own heading and stays
   * open. Wrapping it in a second collapsible titled the same thing is how one
   * capability ended up with two rows of tabs and two ideas of what was
   * installed.
   */
  embedded?: boolean;
  /**
   * Anything only this page has: a path to a file, a button to unload. Given
   * the chosen engine, because a field that belongs to OpenRouter has no
   * business appearing under llama.cpp.
   */
  children?: React.ReactNode | ((engine: string) => React.ReactNode);
  /** Opened and scrolled to: the page was asked for this capability. */
  focused?: boolean;
}> = ({ title, purpose, statusUrl, installUrl, removeUrl, engines, settingsUrl, serverField, cancelUrl, embedded, children, focused }) => {
  const { t } = useI18n();
  const box = useRef<HTMLDivElement>(null);
  const [status, setStatus] = useState<OptionalStatus | null>(null);
  const [open, setOpen] = useState(Boolean(embedded || focused));
  const loaded = status !== null;
  // Scrolled to once its own state is in: before that the page above it is
  // still growing, and the group ends up below the fold again.
  useEffect(() => {
    if (!focused || !loaded) return;
    setOpen(true);
    const frame = window.requestAnimationFrame(() => box.current?.scrollIntoView({ block: 'start', behavior: 'smooth' }));
    return () => window.cancelAnimationFrame(frame);
  }, [focused, loaded]);
  // A download starts in the background, so the server's progress cell appears
  // a moment after the request is answered. Until it does the row showed
  // nothing at all, which read as a button that does not work.
  const [starting, setStarting] = useState(false);
  // A refused download used to be silent: the request failed, the reply was
  // thrown away, and the button simply did nothing twice in a row.
  const [failed, setFailed] = useState<string | null>(null);
  const [engine, setEngine] = useState<string>(engines?.[0]?.id ?? '');
  const [device, setDevice] = useState<Device>('cuda');
  const [model, setModel] = useState('');
  const [apiKey, setApiKey] = useState('');
  const [keyStored, setKeyStored] = useState(false);

  // The saved choice is read once: every later status is about downloads, and
  // taking its provider again undid a choice the user had just made.
  const choiceRead = useRef(false);
  const load = useCallback(async () => {
    const response = await fetch(statusUrl);
    if (response.ok) {
      const body = await response.json();
      setStatus(body);
      if (!choiceRead.current && body.provider && engines?.some((choice) => choice.id === body.provider)) {
        choiceRead.current = true;
        setEngine(body.provider);
      }
      if (body.runtime) setDevice(body.runtime);
      if (body.whisper_model) setModel(body.whisper_model);
      if (body.chosen_model) setModel(body.chosen_model);
      if (body.settings?.runtime) setDevice(body.settings.runtime);
    }
  }, [statusUrl, engines]);

  /// The engine and the device are a setting before they are a download: they
  /// decide which files the one button fetches.
  const remember = async (next: { engine?: string; device?: Device }) => {
    if (next.engine !== undefined) setEngine(next.engine);
    if (next.device !== undefined) setDevice(next.device);
    if (!settingsUrl) return;
    // a local server is chosen by its address: the fields below save the switch once one is typed
    if (settingsUrl.endsWith('/assistant/status') && next.engine === 'local') return;
    await fetch(settingsUrl, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(
        settingsUrl.endsWith('/separation/settings')
          ? { ...(status as unknown as { settings?: object })?.settings, runtime: next.device ?? device }
          : settingsUrl.endsWith('/assistant/status')
            ? { provider: next.engine ?? engine }
            : { provider: next.engine ?? engine, runtime: next.device ?? device },
      ),
    })
      .then(async (response) => {
        if (!response.ok) setFailed(await errorMessage(response));
        return load();
      })
      .catch((error: Error) => setFailed(error.message));
  };

  useEffect(() => { void load().catch(() => undefined); }, [load]);

  useEffect(() => {
    if (!open || !engines?.some((choice) => choice.device === false)) return;
    void fetch('/v1/openrouter/settings')
      .then((response) => (response.ok ? response.json() : null))
      .then((body: { configured?: boolean } | null) => setKeyStored(Boolean(body?.configured)))
      .catch(() => undefined);
    // Asked whenever the panel is opened, not once for the life of the window:
    // a key saved on another screen afterwards left this one sure there was
    // none.
  }, [engines, open]);

  useEffect(() => {
    const active = status?.active_download;
    const running = Boolean(active && !active.done);
    if (!running && !starting) return;
    const timer = window.setInterval(() => void load().catch(() => undefined), 1000);
    return () => window.clearInterval(timer);
  }, [status?.active_download, starting, load]);

  // The moment the server admits a download is running, the press has landed
  // and the real figures take over. If it never does - a refusal, a dead
  // request - the row stops pretending after a few seconds rather than sitting
  // at nought for ever.
  useEffect(() => {
    if (!starting) return;
    if (status?.active_download && !status.active_download.done) {
      setStarting(false);
      return;
    }
    const timer = window.setTimeout(() => setStarting(false), 8000);
    return () => window.clearTimeout(timer);
  }, [status, starting]);

  const assets = status?.assets ?? [];
  if (assets.length === 0) return null;
  const installed = assets.filter((asset) => asset.installed).length;
  // Ready means what the server says the chosen engine needs is on disk. A
  // runtime shared with another role (the karaoke's ONNX and CUDA libraries
  // serve the stem separator too) is not the role being installed.
  const roleReady = status?.set ? status.set.ready : assets.some((asset) => asset.kind === 'model' && asset.installed);
  // The models the chosen engine can run. A recogniser or an assistant is a
  // runtime plus one of these; the runtime is the studio's business, the model
  // is the user's choice.
  const models = assets.filter((asset) => {
    if (asset.kind !== 'model') return false;
    if (!engines || engines.length === 0) return false;
    // A faster-whisper model is a directory of files - weights, tokenizer,
    // vocabulary - and only the weights are a choice. The rest carry no VRAM
    // figure, which is how they are told apart here.
    if (engine === 'whisper') return asset.id.startsWith('whisper-') && asset.vram_gb != null;
    // Parakeet comes in two precisions; the rest of its files are shared, so
    // only the encoders are a choice.
    if (engine === 'parakeet') return asset.id === 'parakeet-tdt-int8' || asset.id === 'parakeet-tdt-fp32';
    if (engine === 'open_router') return false;
    return true;
  });
  const chosenModel = model || models.find((asset) => asset.installed)?.id || models[0]?.id || '';
  /// Engines that run here and therefore have something to download.
  const localEngines = (engines ?? []).filter((choice) => choice.device !== false);

  return (
    <div ref={box} className={embedded ? '' : 'scroll-mt-4 overflow-hidden rounded-xl border border-zinc-200 bg-white dark:border-white/10 dark:bg-suno-card'}>
      {!embedded && (
      <button type="button" onClick={() => setOpen((value) => !value)} className="flex w-full items-center justify-between gap-3 px-4 py-3 text-left transition-colors hover:bg-zinc-50 dark:hover:bg-white/5">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <span className="truncate text-sm font-semibold text-zinc-900 dark:text-white">{title}</span>
            <span className="shrink-0 rounded-full bg-zinc-200/70 px-2 py-0.5 text-[10px] font-bold uppercase tracking-wide text-zinc-500 dark:bg-white/10 dark:text-zinc-400">{t('optionalBadge')}</span>
          </div>
          <p className="mt-0.5 truncate text-xs text-zinc-500 dark:text-zinc-400">{purpose}</p>
        </div>
        <div className="flex shrink-0 items-center gap-2 text-xs text-zinc-500">
          <span className="tabular-nums">
            {engines && engines.length > 0
              ? (roleReady ? t('installed') : t('notInstalledSuffix'))
              : `${installed}/${assets.length}`}
          </span>
          <ChevronDown size={15} className={open ? 'rotate-180 transition-transform' : 'transition-transform'} />
        </div>
      </button>
      )}
      {open && (
        <div className={embedded ? 'space-y-2' : 'space-y-2 border-t border-zinc-100 p-3 dark:border-white/5'}>
          {engines && engines.length > 0 && (
            <div className="space-y-2 pb-1">
              <div className="grid grid-cols-3 gap-1.5">
                {engines.map((choice) => (
                  <button
                    key={choice.id}
                    type="button"
                    onClick={() => void remember({ engine: choice.id })}
                    className={`rounded-lg border px-2 py-1.5 text-xs font-medium transition-colors ${
                      engine === choice.id
                        ? 'border-pink-500 bg-pink-500/10 text-zinc-900 dark:text-white'
                        : 'border-zinc-200 text-zinc-500 hover:border-pink-300 hover:bg-pink-500/5 hover:text-zinc-900 dark:border-white/10 dark:hover:border-pink-500/40 dark:hover:text-white'
                    }`}
                  >
                    {choice.label}
                  </button>
                ))}
              </div>
              {engines.find((choice) => choice.id === engine)?.device !== false && (
                <DevicePicker value={device} onChange={(next) => void remember({ device: next })} />
              )}
            </div>
          )}
          {engines && engines.length > 0 ? (
            // One recogniser, one button. What it is made of - a runtime and
            // five model files, or two files, depending on the card - is the
            // studio's business, not a list to read.
            (() => {
              const chosen = engines.find((choice) => choice.id === engine);
              // Off is a choice, not a thing to install: no files, no key, no
              // server. Without it here the only way to switch the assistant
              // off was the page this control replaced.
              if (engine === 'none') return null;
              if (chosen?.device === false) {
                // The cloud recogniser downloads nothing and needs one thing:
                // the key. Sending the user to another page to type it, and
                // back here to use it, is two pages for one field.
                // The local server's own fields - address and model - live in
                // the slot below, saved as they are typed, so there is no second
                // Save button and nothing to forget to press.
                if (serverField && engine === 'local') {
                  return null;
                }
                // the connected agent needs neither a key nor a download
                if (engine === 'agent') {
                  return null;
                }
                return (
                  <div className="flex items-center gap-2">
                    <input
                      type="password"
                      value={apiKey}
                      onChange={(event) => setApiKey(event.target.value)}
                      placeholder={keyStored ? '••••••••' : 'sk-or-...'}
                      className="min-w-0 flex-1 rounded-lg border border-zinc-200 bg-white px-2.5 py-2 text-xs text-zinc-900 outline-none focus:border-pink-400 dark:border-white/10 dark:bg-black/20 dark:text-white"
                    />
                    <button
                      type="button"
                      onClick={() => {
                        setFailed(null);
                        void fetch('/v1/openrouter/settings', {
                          method: 'PUT',
                          headers: { 'Content-Type': 'application/json' },
                          body: JSON.stringify({ api_key: apiKey.trim() || null }),
                        })
                          .then(async (response) => {
                            if (!response.ok) setFailed(await errorMessage(response));
                            else { setApiKey(''); setKeyStored(true); }
                          })
                          .catch((error: Error) => setFailed(error.message));
                      }}
                      className="shrink-0 rounded-lg border border-zinc-300 px-3 py-2 text-xs font-semibold text-zinc-700 hover:border-pink-400 hover:text-pink-600 dark:border-white/15 dark:text-zinc-200"
                    >
                      {t('save')}
                    </button>
                  </div>
                );
              }
              const running = status?.active_download && !status.active_download.done ? status.active_download : null;
              const busy = Boolean(running) || starting;
              const modelBytes = models.find((asset) => asset.id === chosenModel)?.bytes ?? 0;
              const setBytes = (status?.set?.bytes ?? 0) + modelBytes;
              const haveBytes = (status?.set?.installed_bytes ?? 0)
                + (models.find((asset) => asset.id === chosenModel)?.installed ? modelBytes : 0);
              // While a download runs, progress is that download - the server
              // reports a whole set as one figure. Between downloads it is what
              // is on disk out of what this engine needs. Reading only the
              // second is why a seven-gigabyte file sat at nought per cent from
              // the first byte to the last.
              const percent = running && running.total_bytes > 0
                ? Math.min(100, Math.round((running.downloaded_bytes / running.total_bytes) * 100))
                : setBytes > 0 ? Math.min(100, Math.round((haveBytes / setBytes) * 100)) : 0;
              const ready = setBytes > 0 && haveBytes >= setBytes;
              return (
                // One row, the way Dub Studio states it: what this is, what it
                // weighs, whether it is here, which variant, and the two
                // buttons that change that.
                <div className="rounded-lg border border-zinc-200 px-2.5 py-2 dark:border-white/10">
                  <div className="flex items-center gap-2.5">
                    <span className={`h-1.5 w-1.5 shrink-0 rounded-full ${ready ? 'bg-pink-500' : 'bg-zinc-400'}`} />
                    <div className="min-w-0 flex-1">
                      <div className="truncate text-xs font-medium text-zinc-900 dark:text-white">
                        {engines.find((choice) => choice.id === engine)?.label}
                      </div>
                      <div className="truncate text-[10px] tabular-nums text-zinc-500 dark:text-zinc-400">
                        {bytes(setBytes)}{ready ? ` · ${t('installed')}` : ` · ${t('notInstalledSuffix')}`}
                      </div>
                    </div>
                    {models.length > 1 && (
                      <select
                        value={chosenModel}
                        onChange={(event) => {
                          setModel(event.target.value);
                          // Which model was picked is a setting, not a detail of
                          // the download: the studio decides the assistant is
                          // ready by reading it back. Saving it only for Whisper
                          // is why a downloaded Gemma still counted as no
                          // assistant at all.
                          // Parakeet's two precisions are chosen through the
                          // same field the recogniser reads for Whisper - which
                          // is how the server decides which encoder to fetch.
                          const field =
                            engine === 'whisper' || engine === 'parakeet'
                              ? 'whisper_model'
                              : engine === 'managed'
                                ? 'managed_model'
                                : null;
                          if (settingsUrl && field) {
                            void fetch(settingsUrl, {
                              method: 'PUT',
                              headers: { 'Content-Type': 'application/json' },
                              body: JSON.stringify({ [field]: event.target.value }),
                            }).catch(() => undefined);
                          }
                        }}
                        className="shrink-0 rounded-md border border-zinc-200 bg-white px-2 py-1 text-[11px] tabular-nums text-zinc-800 outline-none focus:border-pink-400 dark:border-white/10 dark:bg-black/20 dark:text-zinc-100"
                      >
                        {models.map((asset) => (
                          <option key={asset.id} value={asset.id}>
                            {asset.label}{asset.installed ? ' ✓' : ''} · {bytes(asset.bytes)}
                          </option>
                        ))}
                      </select>
                    )}
                    {busy ? (
                      <>
                        <span className="w-10 shrink-0 text-right text-[11px] tabular-nums text-pink-500">{percent}%</span>
                        {cancelUrl && (
                          <button
                            type="button"
                            onClick={() => {
                              setFailed(null);
                              setStarting(false);
                              void fetch(cancelUrl, { method: 'POST' })
                                .then(() => load())
                                .catch((error: Error) => setFailed(error.message));
                            }}
                            title={t('cancelDownload')}
                            className="shrink-0 rounded-md border border-zinc-300 p-1.5 text-zinc-500 transition-colors hover:border-rose-400 hover:bg-rose-500/10 hover:text-rose-600 dark:border-white/15 dark:text-zinc-400"
                          >
                            <Square size={13} />
                          </button>
                        )}
                      </>
                    ) : (
                      <>
                        <button
                          type="button"
                          onClick={() => {
                            setFailed(null);
                            setStarting(true);
                            void fetch(installUrl, {
                              method: 'POST',
                              headers: { 'Content-Type': 'application/json' },
                              body: JSON.stringify({ asset_id: localEngines.length === 1 ? device : engine, model_id: chosenModel || undefined }),
                            })
                              .then(async (response) => {
                                if (!response.ok) setFailed(await errorMessage(response));
                                return load();
                              })
                              .catch((error: Error) => setFailed(error.message));
                          }}
                          title={t('download')}
                          className="shrink-0 rounded-md border border-zinc-300 p-1.5 text-zinc-600 transition-colors hover:border-pink-400 hover:bg-pink-500/10 hover:text-pink-600 dark:border-white/15 dark:text-zinc-300"
                        >
                          <Download size={13} />
                        </button>
                        {removeUrl && ready && (
                          <button
                            type="button"
                            onClick={() => {
                              setFailed(null);
                              void fetch(removeUrl, {
                                method: 'POST',
                                headers: { 'Content-Type': 'application/json' },
                                body: JSON.stringify({ asset_id: engine }),
                              })
                                .then(async (response) => {
                                  if (!response.ok) setFailed(await errorMessage(response));
                                  return load();
                                })
                                .catch((error: Error) => setFailed(error.message));
                            }}
                            title={t('remove')}
                            className="shrink-0 rounded-md border border-zinc-300 p-1.5 text-zinc-500 transition-colors hover:border-rose-400 hover:bg-rose-500/10 hover:text-rose-600 dark:border-white/15 dark:text-zinc-400"
                          >
                            <Trash2 size={13} />
                          </button>
                        )}
                      </>
                    )}
                  </div>
                  {busy && (
                    <div className="mt-2 h-1 overflow-hidden rounded-full bg-zinc-200 dark:bg-black/30">
                      <div className="h-full bg-gradient-to-r from-orange-500 to-pink-500 transition-[width]" style={{ width: `${percent}%` }} />
                    </div>
                  )}
                </div>
              );
            })()
          ) : (
            <>
              {assets.map((asset) => {
            const active = status?.active_download?.asset_id === asset.id && !status?.active_download?.done;
            const percent = active && status?.active_download
              ? Math.min(100, Math.round((status.active_download.downloaded_bytes / Math.max(1, status.active_download.total_bytes)) * 100))
              : 0;
            return (
              <div key={asset.id} className="rounded-lg border border-zinc-200 p-3 dark:border-white/10">
                <div className="flex items-center justify-between gap-3">
                  <span className="truncate text-sm text-zinc-800 dark:text-zinc-100">{asset.label}</span>
                  <div className="flex shrink-0 items-center gap-2">
                    <span className="text-xs tabular-nums text-zinc-500">{bytes(asset.bytes)}</span>
                    {asset.installed ? (
                      <>
                        <span className="rounded-full bg-emerald-500/10 px-2 py-0.5 text-[10px] font-semibold text-emerald-600 dark:text-emerald-300">{t('installed')}</span>
                        {removeUrl && (
                          <button
                            type="button"
                            onClick={() => {
                              setFailed(null);
                              void fetch(removeUrl, {
                                method: 'POST',
                                headers: { 'Content-Type': 'application/json' },
                                body: JSON.stringify({ asset_id: asset.id }),
                              })
                                .then(async (response) => {
                                  if (!response.ok) setFailed(await errorMessage(response));
                                  return load();
                                })
                                .catch((error: Error) => setFailed(error.message));
                            }}
                            disabled={Boolean(status?.active_download && !status.active_download.done)}
                            title={t('removeDownloaded')}
                            className="inline-flex items-center gap-1 rounded-lg border border-zinc-300 px-2 py-1 text-xs font-medium text-zinc-500 hover:border-rose-400 hover:text-rose-600 disabled:opacity-40 dark:border-white/15 dark:text-zinc-400"
                          >
                            <Trash2 size={13} />
                            {t('remove')}
                          </button>
                        )}
                      </>
                    ) : (
                      <button
                        type="button"
                        onClick={() => {
                          setFailed(null);
                          void fetch(installUrl, {
                            method: 'POST',
                            headers: { 'Content-Type': 'application/json' },
                            body: JSON.stringify({ asset_id: asset.id }),
                          })
                            .then(async (response) => {
                              if (!response.ok) setFailed(await errorMessage(response));
                              return load();
                            })
                            .catch((error: Error) => setFailed(error.message));
                        }}
                        disabled={Boolean(status?.active_download && !status.active_download.done)}
                        className="inline-flex items-center gap-1 rounded-lg border border-zinc-300 px-2 py-1 text-xs font-medium text-zinc-600 hover:border-pink-400 hover:text-pink-600 disabled:opacity-40 dark:border-white/15 dark:text-zinc-300"
                      >
                        {active ? <Loader2 size={13} className="animate-spin" /> : <Download size={13} />}
                        {active ? `${percent}%` : t('download')}
                      </button>
                    )}
                  </div>
                </div>
                <p className="mt-1 text-[11px] leading-4 text-zinc-500">{asset.note}</p>
                {active && (
                  <div className="mt-2 h-1 overflow-hidden rounded-full bg-zinc-200 dark:bg-zinc-700">
                    <div className="h-full bg-pink-500 transition-[width]" style={{ width: `${percent}%` }} />
                  </div>
                )}
              </div>
            );
              })}
            </>
          )}
          {typeof children === 'function' ? (children as (engine: string) => React.ReactNode)(engine) : children}
          {failed && <p className="text-xs text-rose-600 dark:text-rose-300">{failed}</p>}
          {status?.active_download?.error && <p className="text-xs text-rose-600 dark:text-rose-300">{status.active_download.error}</p>}
        </div>
      )}
    </div>
  );
};

/**
 * Same catalogue, two situations: the first run, where nothing is installed and
 * the studio cannot work yet, and the settings page, where the models are there
 * and the question is only which of them to use.
 */
export const SetupGate: React.FC<{ onReady?: () => void; mode?: 'first-run' | 'settings'; focus?: string | null }> = ({ onReady, mode = 'first-run', focus = null }) => {
  const { t } = useI18n();
  const [status, setStatus] = useState<SetupStatus | null>(null);
  const [catalog, setCatalog] = useState<Catalog | null>(null);
  const [choice, setChoice] = useState<Record<string, string>>({});
  const chosenValues = useMemo(() => Object.values(choice).filter((id): id is string => typeof id === 'string' && id.length > 0), [choice]);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    const response = await fetch('/setup/status');
    if (!response.ok) throw new Error(await errorMessage(response));
    const next: SetupStatus = await response.json();
    setStatus(next);
    if (next.ready && next.engine_ready) onReady?.();
    return next;
  }, [onReady]);

  // What to show as chosen: what the studio is actually set to, not what it
  // would recommend. Preselecting the recommendation made an installed studio
  // look like it had 26 GB to download.
  const [preselected, setPreselected] = useState(false);

  useEffect(() => {
    if (preselected || !catalog || !status) return;
    const ids = status.selected_component_ids?.length
      ? status.selected_component_ids
      : catalog.profiles.find((profile) => profile.id === status.selected_profile_id)?.components
        ?? catalog.profiles.find((profile) => profile.id === catalog.recommended_profile_id)?.components
        ?? [];
    if (ids.length === 0) return;
    const picked: Record<string, string> = {};
    for (const id of ids) {
      const component = catalog.components.find((entry) => entry.id === id);
      if (component) picked[component.kind] = component.id;
    }
    setChoice(picked);
    setPreselected(true);
  }, [catalog, status, preselected]);

  useEffect(() => {
    void fetch('/setup/catalog')
      .then(async (response) => {
        if (!response.ok) throw new Error(await errorMessage(response));
        return response.json() as Promise<Catalog>;
      })
      .then(setCatalog)
      .catch((reason) => setError(reason.message));
  }, []);

  useEffect(() => {
    void refresh().catch((reason) => setError(reason.message));
    const timer = window.setInterval(() => void refresh().catch((reason) => setError(reason.message)), 1000);
    return () => window.clearInterval(timer);
  }, [refresh]);

  const groups = useMemo(() => componentsByKind(catalog?.components || []), [catalog]);
  const chosenIds = useMemo(() => completeCustomComponentIds(catalog?.components || [], choice), [catalog, choice]);
  const chosenBytes = useMemo(
    () => selectedComponentBytes(catalog?.components || [], chosenIds ?? chosenValues),
    [catalog, chosenIds, chosenValues],
  );
  /// The set the studio would pick for this card, by name, so the sentence at
  /// the top says the same thing as the list underneath.
  const recommended = catalog?.profiles.find((profile) => profile.id === status?.recommended_profile_id);
  const active = status?.active;
  const progress = active && active.total_bytes > 0 ? Math.min(100, (active.downloaded_bytes / active.total_bytes) * 100) : 0;
  const installedIds = status?.installed_components ?? [];
  const missing = chosenValues.filter((id) => !installedIds.includes(id));

  // What the studio is set to right now, so "use this set" only appears when
  // the selection on screen is something else.
  const persisted = useMemo(() => {
    const ids = status?.selected_component_ids;
    if (ids?.length) return [...ids].sort().join('|');
    const profile = catalog?.profiles.find((entry) => entry.id === status?.selected_profile_id);
    return profile ? [...profile.components].sort().join('|') : '';
  }, [status, catalog]);
  const chosenKey = useMemo(() => (chosenIds ? [...chosenIds].sort().join('|') : ''), [chosenIds]);
  const canApply = Boolean(chosenIds) && missing.length === 0 && chosenKey !== persisted;

  const apply = async () => {
    if (!chosenIds) return;
    setError(null);
    const response = await fetch('/setup/select', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ component_ids: chosenIds }),
    });
    if (!response.ok) setError(await errorMessage(response));
    else await refresh().catch(() => undefined);
  };

  // The request takes about a second to answer, and for that second the button
  // looked untouched: people pressed it again and could not tell which press
  // counted.
  const [starting, setStarting] = useState(false);
  const download = async () => {
    if (!chosenIds || starting) return;
    setError(null);
    setStarting(true);
    try {
      const response = await fetch('/setup/download', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ component_ids: chosenIds }),
      });
      if (!response.ok) setError(await errorMessage(response));
      else await refresh().catch(() => undefined);
    } finally {
      setStarting(false);
    }
  };

  // Ten gigabytes arrived on request; they leave on request too, without
  // anyone having to find the folder by hand.
  const removeComponents = async (ids: string[]) => {
    if (ids.length === 0) return;
    setError(null);
    const response = await fetch('/setup/remove', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ ids }),
    });
    if (!response.ok) setError(await errorMessage(response));
    else await refresh().catch(() => undefined);
  };

  const cancel = async () => {
    await fetch('/setup/cancel', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({}) }).catch(() => undefined);
    await refresh().catch(() => undefined);
  };

  return (
    <div className="flex h-full w-full justify-center overflow-y-auto bg-white px-5 py-10 dark:bg-suno">
      <div className="w-full max-w-2xl">
        {mode === 'first-run' && (
          <div className="flex items-center gap-2 text-xs font-medium uppercase tracking-[0.18em] text-pink-500">
            <span className="h-2 w-2 rounded-full bg-pink-500 shadow-[0_0_10px_rgba(236,72,153,0.75)]" />
            {t('firstRun')}
          </div>
        )}
        <h1 className={`${mode === 'first-run' ? 'mt-4 text-3xl' : 'text-2xl'} font-extrabold tracking-tight text-zinc-900 dark:text-white`}>
          {mode === 'first-run' ? t('setupTitle') : t('resourcesTitle')}
        </h1>
        <p className="mt-3 max-w-xl text-sm leading-6 text-zinc-500 dark:text-zinc-400">
          {mode === 'first-run' ? t('setupSubtitle') : t('resourcesSubtitle')}
        </p>
        {mode === 'settings' && <div className="mt-4"><RescanButton onDone={() => void refresh().catch((problem: unknown) => setError(problem instanceof Error ? problem.message : String(problem)))} /></div>}

        {error && (
          <div className="mt-5 flex gap-2 rounded-xl border border-rose-300 bg-rose-50 px-4 py-3 text-sm text-rose-700 dark:border-rose-500/30 dark:bg-rose-500/10 dark:text-rose-200">
            <X size={16} className="mt-0.5 shrink-0" />{error}
          </div>
        )}

        {mode === 'first-run' && <div className="mt-5 rounded-xl border border-pink-300/50 bg-pink-50 px-4 py-3 text-sm text-zinc-700 dark:border-pink-500/25 dark:bg-pink-500/10 dark:text-zinc-200">
          <span className="font-semibold text-pink-600 dark:text-pink-400">{t('recommendedForMachine')}</span>{' '}
          {/* The card, its memory, and the set that fits - named as the set is
              named in the list below. The server's own sentence was English
              prose in a Russian window, and it called the answer "Native
              quality", which reads as the unquantised weights it is not. */}
          {recommended
            ? `${status?.hardware?.gpuName ?? ''}${status?.hardware?.totalVramGb ? `, ${status.hardware.totalVramGb.toFixed(1)} GB` : ''} — ${profileLabel(t, recommended)} · ${bytes(recommended.total_bytes)}`
            : status?.hardware?.reason || t('recommendedFallback')}
          . {t('setupResumable')}
        </div>}



        {/* The engine itself: one row per role, one quantisation inside each. */}
        <div className="mt-6 overflow-hidden rounded-xl border border-zinc-200 bg-white dark:border-white/10 dark:bg-suno-card">
          <div className="flex items-center justify-between gap-3 border-b border-zinc-100 px-4 py-3 dark:border-white/5">
            <div className="min-w-0">
              <div className="flex items-center gap-2">
                <span className="text-sm font-semibold text-zinc-900 dark:text-white">{t('engineGroupTitle')}</span>
                <span className="rounded-full bg-pink-500/10 px-2 py-0.5 text-[10px] font-bold uppercase tracking-wide text-pink-600 dark:text-pink-300">{t('requiredBadge')}</span>
              </div>
              <p className="mt-0.5 text-xs text-zinc-500 dark:text-zinc-400">{t('engineGroupHint')}</p>
            </div>
            <span className="shrink-0 text-xs tabular-nums text-zinc-500">{bytes(chosenBytes)}</span>
          </div>

          <div className="space-y-3 p-4">
            {/* The ready-made sets, before the per-role choice: most people want
                "the one my card can run", not five separate decisions. */}
            {(catalog?.profiles ?? []).filter((profile) => profile.installable).length > 0 && (
              <div className="space-y-2">
                <p className="text-[11px] font-bold uppercase tracking-wide text-zinc-500 dark:text-zinc-400">{t('readyMadeSets')}</p>
                <div className="space-y-2">
                  {(catalog?.profiles ?? []).filter((profile) => profile.installable).map((profile) => {
                    const missingHere = profile.components.filter((id) => !installedIds.includes(id));
                    const active = chosenKey === [...profile.components].sort().join('|');
                    // What the set is actually made of, role by role, instead of
                    // a name cut off after twenty characters.
                    const parts = profile.components
                      .map((id) => catalog?.components.find((entry) => entry.id === id))
                      .filter((component): component is ModelComponent => Boolean(component))
                      .map((component) => `${componentKindLabel(component.kind)} ${componentVariant(component)} ${componentPrecision(component)}`);
                    return (
                      <button
                        key={profile.id}
                        type="button"
                        onClick={() => {
                          const picked: Record<string, string> = {};
                          for (const id of profile.components) {
                            const component = catalog?.components.find((entry) => entry.id === id);
                            if (component) picked[component.kind] = component.id;
                          }
                          setChoice(picked);
                        }}
                        className={`block w-full rounded-xl border p-3 text-left transition-colors ${
                          active ? 'border-pink-400 bg-pink-500/10' : 'border-zinc-200 hover:border-pink-300 dark:border-white/10'
                        }`}
                      >
                        <div className="flex flex-wrap items-center gap-2">
                          <span className="text-sm font-semibold text-zinc-900 dark:text-white">{profileLabel(t, profile)}</span>
                          {profile.recommended && (
                            <span className="rounded-full bg-pink-500/10 px-2 py-0.5 text-[10px] font-bold uppercase tracking-wide text-pink-600 dark:text-pink-300">
                              {t('recommendedBadge')}
                            </span>
                          )}
                          <span className="ml-auto shrink-0 text-xs tabular-nums text-zinc-500">{bytes(profile.total_bytes)}</span>
                        </div>
                        <p className="mt-1 text-[11px] leading-5 text-zinc-500 dark:text-zinc-400">{parts.join(' · ')}</p>
                        <div className="mt-1 flex items-center justify-between gap-2">
                          <p className={`text-[11px] font-medium ${missingHere.length === 0 ? 'text-emerald-600 dark:text-emerald-300' : 'text-zinc-500'}`}>
                            {missingHere.length === 0
                              ? t('installed')
                              : `${t('toDownload')} ${bytes(selectedComponentBytes(catalog?.components || [], missingHere))}`}
                          </p>
                          {missingHere.length === 0 && (
                            <span
                              role="button"
                              tabIndex={0}
                              onClick={(event) => { event.stopPropagation(); void removeComponents(profile.components); }}
                              onKeyDown={(event) => { if (event.key === 'Enter') { event.stopPropagation(); void removeComponents(profile.components); } }}
                              title={t('removeDownloaded')}
                              className="inline-flex shrink-0 cursor-pointer items-center gap-1 rounded-lg border border-zinc-300 px-2 py-1 text-[11px] font-medium text-zinc-500 hover:border-rose-400 hover:text-rose-600 dark:border-white/15 dark:text-zinc-400"
                            >
                              <Trash2 size={12} />
                              {t('remove')}
                            </span>
                          )}
                        </div>
                      </button>
                    );
                  })}                </div>
              </div>
            )}

            <p className="text-xs leading-5 text-zinc-500 dark:text-zinc-400">{t('pickOneQuantHint')}</p>
            {groups.map((group) => {
              const selectedId = choice[group.kind] || '';
              const isInstalled = selectedId ? installedIds.includes(selectedId) : false;
              return (
                <div key={group.kind} className="grid grid-cols-[minmax(0,1fr)_auto] items-end gap-2">
                  <label className="min-w-0">
                    <span className="mb-1 block text-[11px] font-semibold uppercase tracking-wide text-zinc-500 dark:text-zinc-400">{componentKindLabel(group.kind)}</span>
                    <select
                      value={selectedId}
                      onChange={(event) => setChoice((current) => ({ ...current, [group.kind]: event.target.value }))}
                      disabled={active?.status === 'downloading'}
                      className="w-full rounded-lg border border-zinc-200 bg-zinc-50 px-3 py-2 text-sm text-zinc-900 focus:border-pink-500 focus:outline-none disabled:opacity-50 dark:border-white/10 dark:bg-black/20 dark:text-white"
                    >
                      <option value="">{t('chooseComponent')}</option>
                      {group.components.map((component) => (
                        <option key={component.id} value={component.id}>
                          {componentVariant(component)} · {componentPrecision(component)} — {bytes(component.bytes)}{installedIds.includes(component.id) ? ` · ${t('installed')}` : ''}
                        </option>
                      ))}
                    </select>
                  </label>
                  <div className="mb-2 flex shrink-0 items-center gap-2">
                    <span className={`rounded-full px-2 py-0.5 text-[10px] font-semibold ${isInstalled ? 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-300' : 'bg-zinc-200/70 text-zinc-500 dark:bg-white/10 dark:text-zinc-400'}`}>
                      {isInstalled ? t('installed') : t('missingBadge')}
                    </span>
                    {isInstalled && (
                      <button
                        type="button"
                        onClick={() => void removeComponents([selectedId])}
                        disabled={active?.status === 'downloading'}
                        title={t('removeDownloaded')}
                        className="inline-flex items-center rounded-lg border border-zinc-300 p-1 text-zinc-500 hover:border-rose-400 hover:text-rose-600 disabled:opacity-40 dark:border-white/15 dark:text-zinc-400"
                      >
                        <Trash2 size={13} />
                      </button>
                    )}
                  </div>
                </div>
              );
            })}
            {!chosenIds && <p className="text-xs text-amber-600 dark:text-amber-300">{t('incompleteSet')}</p>}
            {/* Where the weights actually are. People asked for this after
                hunting through their profile folder for ten gigabytes. */}
            {status?.data_directory && (
              <div className="mt-2 rounded-lg border border-zinc-200 p-3 dark:border-white/10">
                <p className="text-[11px] font-semibold uppercase tracking-wide text-zinc-500 dark:text-zinc-400">{t('dataFolder')}</p>
                <p className="mt-1 break-all font-mono text-[11px] text-zinc-600 dark:text-zinc-300">{status.data_directory}</p>
                {status.portable && <p className="mt-1 text-[11px] leading-4 text-emerald-600 dark:text-emerald-300">{t('portableFolder')}</p>}
                <button
                  type="button"
                  onClick={() => void fetch('/v1/open-data-directory', { method: 'POST' })}
                  className="mt-2 inline-flex items-center gap-1 rounded-lg border border-zinc-300 px-2 py-1 text-[11px] font-medium text-zinc-600 hover:border-pink-400 hover:text-pink-600 dark:border-white/15 dark:text-zinc-300"
                >
                  <FolderOpen size={13} />
                  {t('openFolder')}
                </button>
                {/* Anyone who has run this engine elsewhere already has these files.
                    Downloading ten gigabytes again to get them is a waste of a
                    line and a disk. */}
                <button
                  type="button"
                  onClick={() => {
                    void fetch('/setup/adopt', { method: 'POST' })
                      .then((response) => (response.ok ? response.json() : null))
                      .then((body: { picked?: boolean; adopted?: string[] } | null) => {
                        if (body?.picked) void refresh();
                      })
                      .catch(() => undefined);
                  }}
                  className="ml-2 mt-2 inline-flex items-center gap-1 rounded-lg border border-zinc-300 px-2 py-1 text-[11px] font-medium text-zinc-600 hover:border-pink-400 hover:text-pink-600 dark:border-white/15 dark:text-zinc-300"
                >
                  <FolderDown size={13} />
                  {t('useExistingModels')}
                </button>
              </div>
            )}
          </div>
        </div>

        {active?.status === 'downloading' && (
          <div className="mt-5 rounded-xl border border-zinc-200 bg-white p-4 dark:border-white/10 dark:bg-suno-card">
            <div className="flex items-center justify-between text-sm">
              <span className="flex items-center gap-2 font-medium text-zinc-700 dark:text-zinc-200">
                <Loader2 size={16} className="animate-spin text-pink-500" />{t('downloading')}
              </span>
              <span className="text-zinc-500">{progress.toFixed(1)}%</span>
            </div>
            <div className="mt-3 h-2 overflow-hidden rounded-full bg-zinc-200 dark:bg-black/30">
              <div className="h-full bg-gradient-to-r from-pink-500 to-purple-500 transition-[width]" style={{ width: `${progress}%` }} />
            </div>
            <div className="mt-2 text-xs text-zinc-500">{bytes(active.downloaded_bytes)} / {bytes(active.total_bytes)}</div>
          </div>
        )}

        <div className="mt-6 flex flex-wrap items-center gap-3">
          {active?.status === 'downloading' ? (
            <button type="button" onClick={() => void cancel()} className="inline-flex items-center gap-2 rounded-xl border border-zinc-300 px-4 py-2.5 text-sm font-semibold text-zinc-700 hover:border-rose-400 hover:text-rose-600 dark:border-white/15 dark:text-zinc-200">
              <Square size={15} />{t('cancelDownload')}
            </button>
          ) : (
            <button
              type="button"
              onClick={() => void download()}
              disabled={starting || !chosenIds || missing.length === 0}
              className="inline-flex items-center gap-2 rounded-xl bg-gradient-to-r from-orange-500 to-pink-600 px-5 py-2.5 text-sm font-bold text-white shadow-lg transition-all hover:brightness-110 active:scale-[0.98] disabled:cursor-not-allowed disabled:opacity-50"
            >
              {starting ? <Loader2 size={16} className="animate-spin" /> : <Download size={16} />}
              {missing.length === 0 ? t('everythingInstalled') : `${t('downloadSelectedProfile')} · ${bytes(selectedComponentBytes(catalog?.components || [], missing))}`}
            </button>
          )}
          {canApply && (
            <button
              type="button"
              onClick={() => void apply()}
              className="inline-flex items-center gap-2 rounded-xl border border-pink-400 px-4 py-2.5 text-sm font-semibold text-pink-600 hover:bg-pink-500/10 dark:border-pink-500/40 dark:text-pink-300"
            >
              <Check size={15} />{t('applySelection')}
            </button>
          )}
          {missing.length === 0 && chosenIds && !canApply && (
            <span className="inline-flex items-center gap-1 text-xs font-medium text-emerald-600 dark:text-emerald-300">
              <Check size={14} />{chosenKey === persisted ? t('selectionApplied') : t('setComplete')}
            </span>
          )}
        </div>

        {/* Everything below is optional, and stays that way. */}
        <div className="mt-8">
          <div className="text-xs font-bold uppercase tracking-wide text-zinc-500 dark:text-zinc-400">{t('optionalExtras')}</div>
          <p className="mt-1 text-xs leading-5 text-zinc-500 dark:text-zinc-400">{t('optionalExtrasHint')}</p>
          <div className="mt-3 space-y-3">
            <OptionalGroup
              focused={focus === 'assistant'}
              title={t('assistantSection')}
              purpose={t('assistantOptionalPurpose')}
              statusUrl="/v1/assistant/runtime"
              installUrl="/v1/assistant/runtime/install"
              engines={[
                { id: 'managed', label: 'llama.cpp' },
                { id: 'local', label: t('assistantLocal'), device: false },
                { id: 'open_router', label: 'OpenRouter', device: false },
                { id: 'agent', label: t('assistantAgent'), device: false },
                { id: 'none', label: t('assistantDisabled'), device: false },
              ]}
              settingsUrl="/v1/assistant/status"
              removeUrl="/v1/assistant/runtime/remove"
              cancelUrl="/v1/assistant/runtime/cancel"
              serverField
            >
              {(engine) => <AssistantExtras engine={engine} />}
            </OptionalGroup>
            <OptionalGroup
              title={t('stemsTitle')}
              purpose={t('separationSectionHint')}
              statusUrl="/v1/separation/runtime"
              installUrl="/v1/separation/runtime/install"
              settingsUrl="/v1/separation/settings"
              removeUrl="/v1/separation/remove"
              cancelUrl="/v1/separation/runtime/cancel"
              engines={[{ id: 'htdemucs', label: 'HT-Demucs' }]}
            />
            <OptionalGroup
              title={t('karaokeSection')}
              purpose={t('karaokeOptionalPurpose')}
              statusUrl="/v1/karaoke/status"
              installUrl="/v1/karaoke/install"
              removeUrl="/v1/karaoke/remove"
              settingsUrl="/v1/karaoke/status"
              cancelUrl="/v1/karaoke/cancel"
              engines={[
                { id: 'parakeet', label: 'Parakeet' },
                { id: 'whisper', label: 'Whisper' },
                { id: 'open_router', label: 'OpenRouter', device: false },
              ]}
            >
              {(engine) => <KaraokeExtras engine={engine} />}
            </OptionalGroup>
          </div>
        </div>
      </div>
    </div>
  );
};
