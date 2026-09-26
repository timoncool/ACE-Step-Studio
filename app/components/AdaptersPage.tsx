import React, { useCallback, useEffect, useRef, useState } from 'react';
import { AlertTriangle, Check, CheckSquare, ChevronDown, Download, ExternalLink, FolderOpen, Heart, Layers, Loader2, Pencil, Search, Square, Trash2, X } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { RescanButton } from './RescanButton';
import { openExternal } from '../services/externalLinks';
import { ConfirmDialog } from './ConfirmDialog';
import { TrainingPanel } from './TrainingPanel';
import {
  AdapterSlot,
  AdapterState,
  DitFamily,
  InstalledAdapter,
  OfferedAdapter,
  HubListing,
  HubRepo,
  cancelAdapterDownload,
  hubFiles,
  installHubAdapters,
  looksLikeHubReference,
  searchHub,
  deleteAdapter,
  familyLabel,
  fetchAdapters,
  importAdapter,
  installCatalogAdapters,
  localized,
  megabytes,
  updateAdapter,
} from '../services/adapters';

/**
 * The LoRA page: what is installed, what the catalogue offers, and the user's
 * own files. Picking adapters for a song happens on the create page; here they
 * are fetched, named and given the strength they start at.
 */

const CARD = 'rounded-2xl border border-zinc-200 bg-zinc-50 p-4 dark:border-white/10 dark:bg-white/[0.03]';
const CONTROL =
  'w-full rounded-lg border border-zinc-200 bg-white px-3 py-2 text-sm text-zinc-900 outline-none focus:border-pink-500 disabled:opacity-50 dark:border-white/10 dark:bg-black/30 dark:text-white';
const OUTLINE =
  'inline-flex items-center gap-1.5 rounded-lg border border-zinc-200 px-3 py-1.5 text-xs font-semibold text-zinc-700 hover:border-pink-400 hover:text-pink-600 disabled:cursor-not-allowed disabled:opacity-50 dark:border-white/10 dark:text-zinc-200';
const PRIMARY =
  'inline-flex items-center justify-center gap-2 rounded-lg bg-gradient-to-r from-orange-500 to-pink-600 px-4 py-2 text-sm font-bold text-white disabled:cursor-not-allowed disabled:opacity-50';
const KIND_ORDER = ['trained', 'quality', 'style', 'artist', 'composition', 'sound', 'slider', 'other'];

type Tab = 'installed' | 'catalog' | 'hub' | 'training';

const byKind = <T extends { kind: string }>(list: T[]) =>
  [...list].sort((a, b) => KIND_ORDER.indexOf(a.kind) - KIND_ORDER.indexOf(b.kind));

function useStrings() {
  const { t, language } = useI18n();
  return { t, language, tt: t as unknown as (key: string) => string };
}

/** What a slot changes, in words: the engine names the slot, the role names what it does. */
function slotLabel(tt: (key: string) => string, slots: AdapterSlot[], id: string): string {
  const role = slots.find(slot => slot.id === id)?.role;
  return role ? tt(`adapterRole_${role}`) : id;
}

/** The DiT width an adapter fits, amber when the studio renders with the other one. */
const FamilyBadge: React.FC<{ model?: DitFamily | null; current?: DitFamily | null }> = ({ model, current }) => {
  if (!model) return null;
  const other = model !== 'lm' && current && current !== model;
  return (
    <span className={`rounded-full px-2 py-0.5 text-[10px] font-bold tracking-wide ${other ? 'bg-amber-500/15 text-amber-700 dark:text-amber-300' : 'bg-zinc-200/70 text-zinc-600 dark:bg-white/10 dark:text-zinc-300'}`}>
      {familyLabel(model)}
    </span>
  );
};

/** Why an adapter will not apply to the model the studio renders with now. */
const FamilyNote: React.FC<{ model?: DitFamily | null; current?: DitFamily | null }> = ({ model, current }) => {
  const { t } = useStrings();
  if (!model || model === 'lm' || !current || current === model) return null;
  return (
    <p className="mt-2 flex items-start gap-1.5 text-[11px] leading-4 text-amber-700 dark:text-amber-300">
      <AlertTriangle size={12} className="mt-0.5 shrink-0" />
      {t('adaptersFamilyOther').replace('{model}', familyLabel(model)).replace('{current}', familyLabel(current))}
    </p>
  );
};

const Popularity: React.FC<{ likes: number; downloads: number }> = ({ likes, downloads }) => {
  const { t } = useStrings();
  return (
    <>
      {likes > 0 && <span className="inline-flex items-center gap-0.5" title={t('adaptersLikes')}><Heart size={10} />{likes}</span>}
      {downloads > 0 && <span className="inline-flex items-center gap-0.5" title={t('adaptersDownloads')}><Download size={10} />{downloads}</span>}
    </>
  );
};

const KindBadge: React.FC<{ kind: string }> = ({ kind }) => {
  const { tt } = useStrings();
  return (
    <span className="rounded-full bg-pink-500/10 px-2 py-0.5 text-[10px] font-bold uppercase tracking-wide text-pink-600 dark:text-pink-300">
      {tt(`adapterKind_${kind}`) || kind}
    </span>
  );
};

const SlotBadges: React.FC<{ ids: string[]; slots: AdapterSlot[] }> = ({ ids, slots }) => {
  const { t, tt } = useStrings();
  if (ids.length === 0) return <span className="text-[11px] text-zinc-400">{t('adaptersNotChecked')}</span>;
  return (
    <span className="flex flex-wrap gap-1">
      {ids.map(id => (
        <span
          key={id}
          title={tt(`adapterRoleHint_${slots.find(slot => slot.id === id)?.role ?? ''}`)}
          className="rounded-md border border-zinc-200 px-1.5 py-0.5 text-[10px] font-semibold text-zinc-600 dark:border-white/10 dark:text-zinc-300"
        >
          {slotLabel(tt, slots, id)}
        </span>
      ))}
    </span>
  );
};

const CatalogCard: React.FC<{
  entry: OfferedAdapter;
  slots: AdapterSlot[];
  current?: DitFamily | null;
  selected: boolean;
  downloading: boolean;
  onToggle: () => void;
}> = ({ entry, slots, current, selected, downloading, onToggle }) => {
  const { t, language } = useStrings();
  return (
    <section className={`${CARD} flex flex-col ${selected ? 'border-pink-400 dark:border-pink-500/60' : ''}`}>
      <div className="flex items-start justify-between gap-2">
        <div className="min-w-0">
          <p className="truncate text-sm font-semibold text-zinc-900 dark:text-white">{localized(entry.name, language)}</p>
          <p className="mt-0.5 flex flex-wrap items-center gap-x-2 text-[11px] text-zinc-500">
            <span>{entry.author}{entry.trigger ? ` · ${entry.trigger}` : ''}</span>
            <Popularity likes={entry.likes} downloads={entry.downloads} />
          </p>
        </div>
        <span className="flex shrink-0 items-center gap-1"><FamilyBadge model={entry.model} current={current} /><KindBadge kind={entry.kind} /></span>
      </div>
      <p className="mt-2 flex-1 text-xs leading-5 text-zinc-600 dark:text-zinc-300">{localized(entry.description, language)}</p>
      <FamilyNote model={entry.model} current={current} />
      <div className="mt-3 flex flex-wrap items-center justify-between gap-2">
        <SlotBadges ids={entry.slots} slots={slots} />
        <div className="flex items-center gap-2">
          {entry.page && (
            <button type="button" onClick={() => openExternal(entry.page!)} className={OUTLINE} title={t('adaptersAuthorPage')}>
              <ExternalLink size={13} />
            </button>
          )}
          {entry.installed ? (
            <span className="inline-flex items-center gap-1 text-xs font-semibold text-emerald-600 dark:text-emerald-400"><Check size={13} />{t('adaptersInstalled')}</span>
          ) : downloading ? (
            <span className="inline-flex items-center gap-1.5 text-xs font-semibold text-pink-600 dark:text-pink-300"><Loader2 size={13} className="animate-spin" />{t('adaptersDownloading')}</span>
          ) : (
            <button type="button" role="checkbox" aria-checked={selected} onClick={onToggle} className={`${OUTLINE} ${selected ? 'border-pink-400 text-pink-600 dark:border-pink-500/60 dark:text-pink-300' : ''}`}>
              {selected ? <CheckSquare size={13} /> : <Square size={13} />}
              {megabytes(entry.bytes)}
            </button>
          )}
        </div>
      </div>
    </section>
  );
};

const InstalledCard: React.FC<{
  adapter: InstalledAdapter;
  slots: AdapterSlot[];
  current?: DitFamily | null;
  onDelete: () => void;
  onSaved: () => void;
  onError: (message: string) => void;
}> = ({ adapter, slots, current, onDelete, onSaved, onError }) => {
  const { t, tt, language } = useStrings();
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState('');
  const [trigger, setTrigger] = useState('');
  const [scales, setScales] = useState<Record<string, number>>({});
  const range = adapter.range ?? [0, 1.5];

  // Editing starts from what is stored now, not from what was stored when the card first drew.
  const startEditing = () => {
    setName(localized(adapter.name, language));
    setTrigger(adapter.trigger ?? '');
    const start: Record<string, number> = {};
    for (const slot of adapter.slots) start[slot] = adapter.scales[slot] ?? 1;
    setScales(start);
    setEditing(true);
  };

  const save = async () => {
    try {
      await updateAdapter(adapter.id, { name, trigger, scales });
      setEditing(false);
      onSaved();
    } catch (problem) {
      onError(problem instanceof Error ? problem.message : String(problem));
    }
  };

  const description = localized(adapter.description, language);
  return (
    <section className={CARD}>
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0 flex-1">
          {editing ? (
            <input value={name} onChange={event => setName(event.target.value)} aria-label={t('adaptersRename')} className={CONTROL} />
          ) : (
            <p className="flex items-center gap-2 truncate text-sm font-semibold text-zinc-900 dark:text-white">
              <Layers size={15} className="shrink-0 text-pink-500" />
              {localized(adapter.name, language)}
            </p>
          )}
          <p className="mt-1 text-[11px] text-zinc-500">
            {adapter.author ? `${adapter.author} · ` : ''}{megabytes(adapter.bytes)}
            {!editing && adapter.trigger ? ` · ${adapter.trigger}` : ''}
          </p>
        </div>
        <div className="flex shrink-0 items-center gap-1.5">
          <FamilyBadge model={adapter.model} current={current} />
          <KindBadge kind={adapter.kind} />
          {adapter.page && (
            <button type="button" onClick={() => openExternal(adapter.page!)} className={OUTLINE} title={t('adaptersAuthorPage')}><ExternalLink size={13} /></button>
          )}
          {editing && (
            <button type="button" onClick={() => setEditing(false)} className={OUTLINE} title={t('adaptersCancel')}><X size={13} /></button>
          )}
          <button type="button" onClick={() => (editing ? void save() : startEditing())} className={OUTLINE} title={editing ? t('adaptersSave') : t('adaptersRename')}>
            {editing ? <Check size={13} /> : <Pencil size={13} />}
          </button>
          <button type="button" onClick={onDelete} className={`${OUTLINE} hover:border-rose-400 hover:text-rose-600`} title={t('adaptersDelete')}><Trash2 size={13} /></button>
        </div>
      </div>

      {description && <p className="mt-2 text-xs leading-5 text-zinc-600 dark:text-zinc-300">{description}</p>}
      <FamilyNote model={adapter.model} current={current} />
      {adapter.error && (
        <p className="mt-2 flex items-start gap-1.5 text-xs text-rose-600 dark:text-rose-300"><AlertTriangle size={13} className="mt-0.5 shrink-0" />{t('adaptersBrokenFiles')}: {adapter.error}</p>
      )}

      <div className="mt-3"><SlotBadges ids={adapter.slots} slots={slots} /></div>

      {editing && (
        <div className="mt-3 space-y-3">
          <label className="block">
            <span className="text-[11px] font-bold uppercase tracking-wide text-zinc-500">{t('adaptersTrigger')}</span>
            <input value={trigger} onChange={event => setTrigger(event.target.value)} className={`${CONTROL} mt-1`} />
            <span className="mt-1 block text-[11px] leading-4 text-zinc-500">{t('adaptersTriggerHint')}</span>
          </label>
          {adapter.slots.length > 0 && (
            <div>
              <span className="text-[11px] font-bold uppercase tracking-wide text-zinc-500">{t('adaptersDefaultStrength')}</span>
              {adapter.slots.map(slot => (
                <div key={slot} className="mt-2">
                  <div className="flex items-baseline justify-between text-[11px] text-zinc-600 dark:text-zinc-300">
                    <span>{slotLabel(tt, slots, slot)}</span>
                    <span className="tabular-nums">{(scales[slot] ?? 1).toFixed(2)}</span>
                  </div>
                  <input
                    type="range"
                    min={range[0]}
                    max={range[1]}
                    step={0.05}
                    value={scales[slot] ?? 1}
                    aria-label={slotLabel(tt, slots, slot)}
                    onChange={event => setScales(current => ({ ...current, [slot]: Number(event.target.value) }))}
                    className="mt-1 h-1 w-full cursor-pointer accent-pink-500"
                  />
                </div>
              ))}
            </div>
          )}
        </div>
      )}
    </section>
  );
};

/**
 * Adapters on Hugging Face: a search over the ones tagged for this model, or a
 * link pasted straight in. A repository opens into its weight files; the
 * ticked ones download together, each becoming its own adapter.
 */
const HubPanel: React.FC<{ current?: DitFamily | null; downloading: boolean; onStarted: () => void; onError: (message: string) => void }> = ({ current, downloading, onStarted, onError }) => {
  const { t } = useStrings();
  const [query, setQuery] = useState('');
  const [repos, setRepos] = useState<HubRepo[] | null>(null);
  const [searching, setSearching] = useState(false);
  const [open, setOpen] = useState<HubListing | null>(null);
  const [opening, setOpening] = useState<string | null>(null);
  const [picked, setPicked] = useState<string[]>([]);
  const [starting, setStarting] = useState(false);

  const openRepo = useCallback(async (reference: string) => {
    setOpening(reference);
    try {
      const { listing, file } = await hubFiles(reference);
      setOpen(listing);
      setPicked(file && listing.files.some(entry => entry.path === file && !entry.installed) ? [file] : []);
    } catch (problem) {
      onError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setOpening(null);
    }
  }, [onError]);

  const search = useCallback(async (text: string) => {
    if (looksLikeHubReference(text)) {
      await openRepo(text.trim());
      return;
    }
    setSearching(true);
    try {
      setRepos(await searchHub(text));
    } catch (problem) {
      onError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setSearching(false);
    }
  }, [onError, openRepo]);

  // searches as the user types or pastes, once the text settles
  useEffect(() => {
    const timer = window.setTimeout(() => void search(query), query ? 400 : 0);
    return () => window.clearTimeout(timer);
  }, [query, search]);

  const files = open?.files ?? [];
  const chosen = files.filter(file => picked.includes(file.path));
  const download = async () => {
    if (!open) return;
    setStarting(true);
    try {
      await installHubAdapters(open.repo, chosen.map(file => file.path));
      setPicked([]);
      onStarted();
    } catch (problem) {
      onError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setStarting(false);
    }
  };

  return (
    <>
      <p className="text-sm text-zinc-600 dark:text-zinc-400">{t('adaptersHubHint')}</p>
      <form onSubmit={event => { event.preventDefault(); void search(query); }} className="relative">
        <Search size={15} className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-zinc-400" />
        <input value={query} onChange={event => setQuery(event.target.value)} placeholder={t('adaptersHubSearch')} aria-label={t('adaptersHubSearch')} className={`${CONTROL} pl-9 pr-9`} />
        {query && (
          <button type="button" onClick={() => setQuery('')} className="absolute right-2.5 top-1/2 -translate-y-1/2 text-zinc-400 hover:text-zinc-700 dark:hover:text-zinc-200" aria-label={t('adaptersCancel')}>
            <X size={15} />
          </button>
        )}
      </form>
      {searching && <p className="flex items-center gap-2 text-sm text-zinc-500"><Loader2 size={14} className="animate-spin" />{t('adaptersHubSearching')}</p>}
      {repos && !searching && repos.length === 0 && !open && <p className="text-sm text-zinc-500">{t('adaptersHubEmpty')}</p>}
      <div className="space-y-3">
        {[...(open && !(repos ?? []).some(repo => repo.repo === open.repo) ? [{ repo: open.repo, author: open.repo.split('/')[0], likes: 0, downloads: 0, tags: [] } as HubRepo] : []), ...(repos ?? [])].map(repo => {
          const expanded = open?.repo === repo.repo;
          return (
            <section key={repo.repo} className={`${CARD} ${expanded ? 'border-pink-400 dark:border-pink-500/60' : ''}`}>
              <button type="button" onClick={() => (expanded ? setOpen(null) : void openRepo(repo.repo))} className="flex w-full items-start justify-between gap-3 text-left" aria-expanded={expanded}>
                <div className="min-w-0">
                  <p className="truncate text-sm font-semibold text-zinc-900 dark:text-white">{repo.repo.split('/')[1]}</p>
                  <p className="mt-0.5 flex flex-wrap items-center gap-x-2 text-[11px] text-zinc-500">
                    <span>{repo.author}</span>
                    <Popularity likes={repo.likes} downloads={repo.downloads} />
                    {repo.updated && <span>{new Date(repo.updated).toLocaleDateString()}</span>}
                  </p>
                  {repo.tags.length > 0 && (
                    <span className="mt-1.5 flex flex-wrap gap-1">
                      {repo.tags.map(tag => <span key={tag} className="rounded-md border border-zinc-200 px-1.5 py-0.5 text-[10px] text-zinc-500 dark:border-white/10">{tag}</span>)}
                    </span>
                  )}
                </div>
                {opening === repo.repo ? <Loader2 size={15} className="mt-0.5 shrink-0 animate-spin text-pink-500" /> : <ChevronDown size={15} className={`mt-0.5 shrink-0 text-zinc-400 transition-transform ${expanded ? 'rotate-180' : ''}`} />}
              </button>
              {expanded && open && (
                <div className="mt-3 space-y-1.5 border-t border-zinc-200 pt-3 dark:border-white/10">
                  {files.length === 0 && <p className="text-xs text-zinc-500">{t('adaptersHubNoFiles')}</p>}
                  {/* said once for the repository, the amber size on each file marks which */}
                  <FamilyNote model={files.find(file => !file.problem && file.model && file.model !== 'lm' && file.model !== current)?.model} current={current} />
                  {files.map(file => (
                    <div key={file.path} className="flex items-center justify-between gap-3">
                      <span className="min-w-0 flex-1">
                        <span className="flex min-w-0 items-center gap-1.5">
                          <span className="min-w-0 truncate text-xs text-zinc-700 dark:text-zinc-200" title={file.path}>{file.path}</span>
                          <FamilyBadge model={file.model} current={current} />
                          {file.format && <span className="shrink-0 text-[10px] uppercase text-zinc-400">{file.format}</span>}
                        </span>
                        {file.problem ? (
                          <span className="mt-0.5 block text-[11px] text-rose-600 dark:text-rose-300">{t('adaptersHubProblem').replace('{problem}', file.problem)}</span>
                        ) : file.model === 'lm' ? (
                          <span className="mt-0.5 block text-[11px] text-zinc-500">{t('adaptersHubPlanner')}</span>
                        ) : !file.model ? (
                          <span className="mt-0.5 block text-[11px] text-zinc-500">{t('adaptersHubUnknown')}</span>
                        ) : null}
                      </span>
                      {file.installed ? (
                        <span className="inline-flex shrink-0 items-center gap-1 text-xs font-semibold text-emerald-600 dark:text-emerald-400"><Check size={13} />{t('adaptersInstalled')}</span>
                      ) : (
                        <button
                          type="button"
                          role="checkbox"
                          aria-checked={picked.includes(file.path)}
                          onClick={() => setPicked(current => (current.includes(file.path) ? current.filter(path => path !== file.path) : [...current, file.path]))}
                          className={`${OUTLINE} shrink-0 ${picked.includes(file.path) ? 'border-pink-400 text-pink-600 dark:border-pink-500/60 dark:text-pink-300' : ''}`}
                        >
                          {picked.includes(file.path) ? <CheckSquare size={13} /> : <Square size={13} />}
                          {megabytes(file.bytes)}
                        </button>
                      )}
                    </div>
                  ))}
                  <button type="button" onClick={() => openExternal(open.page)} className={`${OUTLINE} mt-2`}><ExternalLink size={13} />{t('adaptersHubOpen')}</button>
                </div>
              )}
            </section>
          );
        })}
      </div>
      {!downloading && chosen.length > 0 && (
        <div className="sticky bottom-0 -mx-1 bg-white/90 px-1 py-3 backdrop-blur dark:bg-suno/90">
          <button type="button" onClick={() => void download()} disabled={starting} className={PRIMARY}>
            {starting ? <Loader2 size={16} className="animate-spin" /> : <Download size={16} />}
            {t('adaptersDownloadSelected')} · {chosen.length} · {megabytes(chosen.reduce((sum, file) => sum + file.bytes, 0))}
          </button>
        </div>
      )}
    </>
  );
};

export function AdaptersPage(): React.ReactElement {
  const { t, tt, language } = useStrings();
  const [state, setState] = useState<AdapterState | null>(null);
  const [tab, setTab] = useState<Tab>('installed');
  const [error, setError] = useState<string | null>(null);
  const [importName, setImportName] = useState('');
  const [importing, setImporting] = useState(false);
  const [deleting, setDeleting] = useState<InstalledAdapter | null>(null);
  const [selected, setSelected] = useState<string[]>([]);
  const [starting, setStarting] = useState(false);
  const [catalogQuery, setCatalogQuery] = useState('');
  const [family, setFamily] = useState<DitFamily | 'all'>('all');
  const filePicker = useRef<HTMLInputElement | null>(null);

  const refresh = useCallback(async () => {
    setState(await fetchAdapters());
  }, []);

  const downloading = Boolean(state?.download && !state.download.done);
  useEffect(() => {
    void refresh().catch(problem => setError(String(problem)));
    const timer = window.setInterval(() => void refresh().catch(() => undefined), downloading ? 1000 : 5000);
    return () => window.clearInterval(timer);
  }, [refresh, downloading]);

  // A finished download is news for the create page's picker.
  const wasDownloading = useRef(false);
  useEffect(() => {
    if (wasDownloading.current && !downloading) window.dispatchEvent(new CustomEvent('studio:adapters-changed'));
    wasDownloading.current = downloading;
  }, [downloading]);

  // A new library opens on the catalogue, where there is something to take.
  const opened = useRef(false);
  useEffect(() => {
    if (state && !opened.current) {
      opened.current = true;
      if (state.installed.length === 0) setTab('catalog');
    }
  }, [state]);

  const importFiles = async (files: File[]) => {
    if (files.length === 0) return;
    setImporting(true);
    setError(null);
    try {
      await importAdapter(importName.trim(), files);
      setImportName('');
      setTab('installed');
      window.dispatchEvent(new CustomEvent('studio:toast', { detail: { message: t('adaptersImported'), type: 'success' } }));
      window.dispatchEvent(new CustomEvent('studio:adapters-changed'));
      await refresh();
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setImporting(false);
      if (filePicker.current) filePicker.current.value = '';
    }
  };

  const confirmDelete = async () => {
    const target = deleting;
    setDeleting(null);
    if (!target) return;
    try {
      await deleteAdapter(target.id);
      window.dispatchEvent(new CustomEvent('studio:adapters-changed'));
      await refresh();
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    }
  };

  const slots = state?.slots ?? [];
  const download = state?.download;
  const current = state?.model ?? null;
  const families = [...new Set((state?.catalog ?? []).map(entry => entry.model).filter((model): model is DitFamily => Boolean(model)))];
  const words = catalogQuery.trim().toLowerCase().split(/\s+/).filter(Boolean);
  // the adapters that fit the model in use first, then by kind, the most liked and downloaded first within each
  const offered = (state?.catalog ?? [])
    .filter(entry => family === 'all' || entry.model === family)
    .filter(entry => {
      const text = [localized(entry.name, language), localized(entry.description, language), entry.author ?? '', entry.trigger ?? '', tt(`adapterKind_${entry.kind}`)].join(' ').toLowerCase();
      return words.every(word => text.includes(word));
    })
    .sort((a, b) =>
      Number(b.model === current) - Number(a.model === current)
      || KIND_ORDER.indexOf(a.kind) - KIND_ORDER.indexOf(b.kind)
      || b.likes - a.likes
      || b.downloads - a.downloads);
  // what is ticked goes down even when a search hides it
  const chosen = (state?.catalog ?? []).filter(entry => selected.includes(entry.id) && !entry.installed);
  const toggle = (id: string) => setSelected(current => (current.includes(id) ? current.filter(value => value !== id) : [...current, id]));
  const missing = offered.filter(entry => !entry.installed);
  const allChosen = missing.length > 0 && missing.every(entry => selected.includes(entry.id));
  const toggleAll = () => setSelected(allChosen ? [] : missing.map(entry => entry.id));

  const downloadSelected = async () => {
    setStarting(true);
    setError(null);
    try {
      await installCatalogAdapters(chosen.map(entry => entry.id));
      setSelected([]);
      await refresh();
    } catch (problem) {
      setError(problem instanceof Error ? problem.message : String(problem));
    } finally {
      setStarting(false);
    }
  };

  return (
    <div className="flex-1 overflow-y-auto bg-white px-5 py-6 dark:bg-suno md:px-8">
      <div className="mx-auto max-w-4xl space-y-6">
        <div>
          <p className="text-xs font-semibold uppercase tracking-[0.18em] text-pink-500">{t('adaptersEyebrow')}</p>
          <h1 className="mt-1 text-2xl font-bold text-zinc-950 dark:text-white">{t('adaptersHeading')}</h1>
          <p className="mt-2 max-w-3xl text-sm text-zinc-600 dark:text-zinc-400">{t('adaptersIntro')}</p>
          <div className="mt-3"><RescanButton onDone={() => void refresh()} /></div>
        </div>

        <div role="tablist" className="flex rounded-lg bg-zinc-100 p-1 dark:bg-white/5">
          {(['installed', 'catalog', 'hub', 'training'] as Tab[]).map(value => (
            <button
              key={value}
              type="button"
              role="tab"
              aria-selected={tab === value}
              onClick={() => setTab(value)}
              className={`flex-1 rounded-md py-1.5 text-xs font-semibold transition-all ${tab === value ? 'bg-white text-black shadow-sm dark:bg-zinc-800 dark:text-white' : 'text-zinc-500 hover:text-zinc-900 dark:hover:text-zinc-300'}`}
            >
              {value === 'installed' ? `${t('adaptersInstalledTab')} · ${state?.installed.length ?? 0}` : value === 'catalog' ? t('adaptersCatalogTab') : value === 'hub' ? t('adaptersHubTab') : t('trainingNav')}
            </button>
          ))}
        </div>

        {download && !download.done && (
          <div className={CARD}>
            <div className="flex items-center justify-between gap-3 text-sm text-zinc-800 dark:text-zinc-200">
              <span className="inline-flex items-center gap-2"><Loader2 size={14} className="animate-spin text-pink-500" />{t('adaptersDownloading')}</span>
              <span className="tabular-nums text-xs text-zinc-500">{megabytes(download.downloaded_bytes)} / {megabytes(download.total_bytes)}</span>
            </div>
            <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-zinc-200 dark:bg-white/10">
              <div
                className="h-full bg-gradient-to-r from-orange-500 to-pink-600"
                style={{ width: `${Math.min(100, (100 * download.downloaded_bytes) / Math.max(1, download.total_bytes))}%` }}
              />
            </div>
            <button type="button" onClick={() => void cancelAdapterDownload()} className={`${OUTLINE} mt-3`}><X size={13} />{t('adaptersCancel')}</button>
          </div>
        )}
        {download?.done && download.error && download.error !== 'cancelled' && (
          <p role="alert" className="rounded-lg bg-rose-500/10 px-3 py-2 text-sm text-rose-700 dark:text-rose-300">{download.error}</p>
        )}

        {tab === 'installed' && (
          <>
            <section className={CARD}>
              <div className="flex items-center gap-2 text-sm font-semibold text-zinc-900 dark:text-white">
                <FolderOpen size={17} className="text-pink-500" /> {t('adaptersImport')}
              </div>
              <p className="mt-2 text-sm text-zinc-600 dark:text-zinc-300">{t('adaptersImportHint')}</p>
              <div className="mt-3 flex flex-col gap-2 sm:flex-row">
                <input
                  value={importName}
                  onChange={event => setImportName(event.target.value)}
                  placeholder={t('adaptersImportName')}
                  aria-label={t('adaptersImportName')}
                  className={CONTROL}
                />
                <button type="button" onClick={() => filePicker.current?.click()} disabled={importing} className={`${PRIMARY} shrink-0`}>
                  {importing ? <Loader2 size={14} className="animate-spin" /> : <FolderOpen size={14} />}
                  {importing ? t('adaptersImporting') : t('adaptersImport')}
                </button>
                <input
                  ref={filePicker}
                  type="file"
                  multiple
                  accept=".safetensors,.json"
                  className="hidden"
                  onChange={event => void importFiles(Array.from(event.target.files ?? []))}
                />
              </div>
            </section>

            {state && !state.engine_checked && state.installed.some(adapter => adapter.slots.length === 0) && (
              <p className="text-xs text-amber-600 dark:text-amber-300">{t('adaptersEngineOff')}</p>
            )}
            {state && state.installed.length === 0 && <p className="text-sm text-zinc-500">{t('adaptersEmpty')}</p>}
            <div className="space-y-3">
              {byKind<InstalledAdapter>(state?.installed ?? []).map(adapter => (
                <InstalledCard
                  key={adapter.id}
                  adapter={adapter}
                  slots={slots}
                  current={current}
                  onDelete={() => setDeleting(adapter)}
                  onSaved={() => {
                    window.dispatchEvent(new CustomEvent('studio:adapters-changed'));
                    void refresh();
                  }}
                  onError={setError}
                />
              ))}
            </div>
          </>
        )}

        {tab === 'catalog' && (
          <>
            <p className="text-sm text-zinc-600 dark:text-zinc-400">{t('adaptersSelectHint')}</p>
            <div className="flex flex-col gap-2 sm:flex-row sm:items-center">
              <div className="relative flex-1">
                <Search size={15} className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-zinc-400" />
                <input value={catalogQuery} onChange={event => setCatalogQuery(event.target.value)} placeholder={t('adaptersCatalogSearch')} aria-label={t('adaptersCatalogSearch')} className={`${CONTROL} pl-9`} />
              </div>
              {families.length > 1 && (
                <div role="radiogroup" className="flex shrink-0 rounded-lg bg-zinc-100 p-1 dark:bg-white/5">
                  {(['all', ...families] as Array<DitFamily | 'all'>).map(value => (
                    <button
                      key={value}
                      type="button"
                      role="radio"
                      aria-checked={family === value}
                      onClick={() => setFamily(value)}
                      className={`rounded-md px-3 py-1 text-xs font-semibold ${family === value ? 'bg-white text-black shadow-sm dark:bg-zinc-800 dark:text-white' : 'text-zinc-500 hover:text-zinc-900 dark:hover:text-zinc-300'}`}
                    >
                      {value === 'all' ? t('adaptersFamilyAll') : value === current ? t('adaptersFamilyFits').replace('{model}', familyLabel(value)) : familyLabel(value)}
                    </button>
                  ))}
                </div>
              )}
            </div>
            {offered.length === 0 && <p className="text-sm text-zinc-500">{t('adaptersCatalogEmpty')}</p>}
            <div className="grid gap-3 md:grid-cols-2">
              {offered.map(entry => (
                <CatalogCard
                  key={entry.id}
                  entry={entry}
                  slots={slots}
                  current={current}
                  selected={selected.includes(entry.id)}
                  downloading={downloading && (state?.installing ?? []).includes(entry.id)}
                  onToggle={() => toggle(entry.id)}
                />
              ))}
            </div>
            {!downloading && (
              <div className="sticky bottom-0 -mx-1 flex flex-wrap items-center gap-2 bg-white/90 px-1 py-3 backdrop-blur dark:bg-suno/90">
                <button type="button" role="checkbox" aria-checked={allChosen} onClick={toggleAll} disabled={missing.length === 0} className={`${OUTLINE} ${allChosen ? 'border-pink-400 text-pink-600 dark:border-pink-500/60 dark:text-pink-300' : ''}`}>
                  {allChosen ? <CheckSquare size={13} /> : <Square size={13} />}
                  {t('adaptersSelectAll')}{missing.length > 0 ? ` · ${missing.length}` : ''}
                </button>
                <button type="button" onClick={() => void downloadSelected()} disabled={starting || chosen.length === 0} className={PRIMARY}>
                  {starting ? <Loader2 size={16} className="animate-spin" /> : <Download size={16} />}
                  {t('adaptersDownloadSelected')}{chosen.length > 0 ? ` · ${chosen.length} · ${megabytes(chosen.reduce((sum, entry) => sum + entry.bytes, 0))}` : ''}
                </button>
              </div>
            )}
          </>
        )}

        {tab === 'training' && <TrainingPanel />}

        {tab === 'hub' && <HubPanel current={current} downloading={downloading} onStarted={() => void refresh()} onError={setError} />}

        {error && <p role="alert" className="rounded-lg bg-rose-500/10 px-3 py-2 text-sm text-rose-700 dark:text-rose-300">{error}</p>}
      </div>

      <ConfirmDialog
        isOpen={deleting !== null}
        title={t('adaptersDeleteTitle')}
        message={`${deleting ? localized(deleting.name, language) : ''}. ${t('adaptersDeleteMessage')}`}
        confirmLabel={t('adaptersDelete')}
        onConfirm={() => void confirmDelete()}
        onCancel={() => setDeleting(null)}
      />
    </div>
  );
}
