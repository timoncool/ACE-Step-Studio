import React, { useEffect, useMemo, useRef, useState } from 'react';
import { Check, ChevronDown, Download, Loader2, Square } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import type { ModelComponent } from '../services/modelCatalog';

/**
 * The model the studio renders with, switched where songs are made.
 *
 * Only the DiT, the model people change from song to song; the planner and
 * the decoder are chosen in Settings - Models. Its list holds every variant of
 * the catalogue with its quantisations: an installed one is used at once, a
 * missing one is downloaded and becomes the studio's model when the download
 * finishes.
 */

export type SwitcherStatus = {
  installed_components?: string[];
  selected_profile_id?: string | null;
  selected_component_ids?: string[] | null;
  active?: { status: string; downloaded_bytes: number; total_bytes: number; component_ids: string[] } | null;
};

type Catalog = { components: ModelComponent[]; profiles: Array<{ id: string; components: string[] }> };
type Role = 'dit';

const QUANTS = ['q4', 'q5', 'q6', 'q8', 'mxfp4', 'bf16'];
const QUANT_LABEL: Record<string, string> = { q4: 'Q4', q5: 'Q5', q6: 'Q6', q8: 'Q8', mxfp4: 'MXFP4', bf16: 'BF16' };

/** `dit-xl-turbo-q8` is the variant `xl-turbo` in the quantisation `q8`. */
const split = (component: ModelComponent) => {
  const parts = component.id.split('-');
  const quant = parts[parts.length - 1];
  return { variant: parts.slice(1, -1).join('-'), quant };
};

const DIT_GROUPS: Array<{ key: string; variants: string[] }> = [
  { key: 'xl', variants: ['xl-turbo', 'xl-sft', 'xl-base', 'xl-sftturbo50'] },
  { key: 'standard', variants: ['turbo', 'sft', 'base', 'sftturbo50', 'turbo-shift1', 'turbo-shift3', 'turbo-continuous'] },
  { key: 'merges', variants: ['merge-sft-turbo-xl-ta-0.3', 'merge-sft-turbo-xl-ta-0.7', 'merge-base-turbo-xl-ta-0.5', 'merge-base-sft-xl-ta-0.5'] },
  { key: 'community', variants: ['xl-turbo-regrind'] },
];

const bytes = (value: number) => (value < 1024 ** 3 ? `${Math.round(value / 1024 ** 2)} MB` : `${(value / 1024 ** 3).toFixed(1)} GB`);

export const ModelSwitcher: React.FC<{ status: SwitcherStatus | null; onChanged: () => void }> = ({ status, onChanged }) => {
  const { t } = useI18n();
  const tt = t as unknown as (key: string) => string;
  const [catalog, setCatalog] = useState<Catalog | null>(null);
  const [open, setOpen] = useState<Role | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const box = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    void fetch('/setup/catalog')
      .then(response => (response.ok ? response.json() : Promise.reject(new Error(String(response.status)))))
      .then((body: Catalog) => setCatalog(body))
      .catch((reason: Error) => setError(reason.message));
  }, []);

  useEffect(() => {
    if (!open) return;
    const close = (event: MouseEvent) => {
      if (box.current && !box.current.contains(event.target as Node)) setOpen(null);
    };
    window.addEventListener('mousedown', close);
    return () => window.removeEventListener('mousedown', close);
  }, [open]);

  const installed = useMemo(() => new Set(status?.installed_components ?? []), [status]);
  const current = useMemo(() => {
    if (status?.selected_component_ids?.length) return status.selected_component_ids;
    return catalog?.profiles.find(profile => profile.id === status?.selected_profile_id)?.components ?? [];
  }, [status, catalog]);
  const byId = useMemo(() => new Map((catalog?.components ?? []).map(component => [component.id, component])), [catalog]);
  const chosen = (role: Role) => current.map(id => byId.get(id)).find(component => component?.kind === role);
  const downloading = status?.active?.status === 'downloading' ? status.active : null;

  // what fits the model changes with it: the LoRA list marks adapters of the other size
  const ditId = chosen('dit')?.id;
  useEffect(() => {
    if (ditId) window.dispatchEvent(new CustomEvent('studio:models-changed'));
  }, [ditId]);

  const name = (variant: string) => tt(`aceVariant_${variant}`);

  /** The current set with this role's component replaced. */
  const setWith = (component: ModelComponent) => {
    const others = current.filter(id => byId.get(id)?.kind !== component.kind);
    return [...others, component.id];
  };

  const pick = async (component: ModelComponent) => {
    setError(null);
    setBusy(component.id);
    try {
      const ids = setWith(component);
      const url = installed.has(component.id) ? '/setup/select' : '/setup/download';
      const response = await fetch(url, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ component_ids: ids }) });
      if (!response.ok) {
        const body = await response.json().catch(() => null);
        throw new Error(body?.error || String(response.status));
      }
      setOpen(null);
      onChanged();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setBusy(null);
    }
  };

  const cancel = async () => {
    await fetch('/setup/cancel', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: '{}' });
    onChanged();
  };

  const button = (role: Role) => {
    const component = chosen(role);
    const parts = component ? split(component) : null;
    return (
      <button
        type="button"
        onClick={() => setOpen(current => (current === role ? null : role))}
        className={`flex min-w-0 flex-1 items-center gap-2 rounded-lg border px-2.5 py-2 text-left transition-colors ${open === role ? 'border-pink-500 bg-pink-500/5' : 'border-zinc-200 bg-zinc-50 hover:border-pink-300 dark:border-white/10 dark:bg-black/25'}`}
      >
        <span className="shrink-0 text-[10px] font-bold uppercase tracking-wide text-zinc-400">DiT</span>
        <span className="min-w-0 flex-1 truncate text-xs font-semibold text-zinc-900 dark:text-white">
          {parts ? `${name(parts.variant)} · ${QUANT_LABEL[parts.quant] ?? parts.quant}` : '—'}
        </span>
        <ChevronDown size={14} className={`shrink-0 text-zinc-400 transition-transform ${open === role ? 'rotate-180' : ''}`} />
      </button>
    );
  };

  const list = (role: Role) => {
    const groups = DIT_GROUPS;
    const components = (catalog?.components ?? []).filter(component => component.kind === role);
    const active = chosen(role)?.id;
    return (
      <div className="absolute left-0 right-0 top-full z-30 mt-1 max-h-[60vh] overflow-y-auto rounded-xl border border-zinc-200 bg-white p-2 shadow-2xl dark:border-white/10 dark:bg-zinc-900">
        <p className="px-2 pb-1 text-[11px] text-zinc-500">
          <Check size={10} className="inline" /> {tt('aceInstalledLegend')} · <Download size={10} className="inline" /> {tt('aceDownloadLegend')}
        </p>
        {groups.map(group => (
          <div key={group.key} className="mb-1 last:mb-0">
            <p className="px-2 pb-1 pt-1.5 text-[10px] font-bold uppercase tracking-wide text-zinc-400">{tt(`aceVariantGroup_${group.key}`)}</p>
            {group.variants.map(variant => {
              const quants = components.filter(component => split(component).variant === variant).sort((a, b) => QUANTS.indexOf(split(a).quant) - QUANTS.indexOf(split(b).quant));
              if (quants.length === 0) return null;
              const selectedHere = quants.some(component => component.id === active);
              return (
                <div key={variant} className={`rounded-lg px-2 py-2 ${selectedHere ? 'bg-pink-500/5' : ''}`}>
                  <div className="flex items-baseline justify-between gap-2">
                    <span className="text-xs font-semibold text-zinc-900 dark:text-white">{name(variant)}</span>
                  </div>
                  <p className="mt-0.5 text-[11px] leading-4 text-zinc-500">{tt(`aceVariantHint_${variant}`)}</p>
                  <div className="mt-1.5 flex flex-wrap gap-1">
                    {quants.map(component => {
                      const here = installed.has(component.id);
                      const on = component.id === active;
                      return (
                        <button
                          key={component.id}
                          type="button"
                          disabled={busy !== null || (downloading !== null && !here)}
                          onClick={() => void pick(component)}
                          title={`${bytes(component.bytes)}${here ? '' : ` · ${tt('aceDownloadAndUse')}`}`}
                          className={`inline-flex items-center gap-1 rounded-md border px-1.5 py-0.5 text-[10px] font-semibold tabular-nums transition-colors disabled:opacity-40 ${
                            on
                              ? 'border-pink-500 bg-pink-500 text-white'
                              : here
                                  ? 'border-zinc-300 text-zinc-700 hover:border-pink-400 dark:border-white/20 dark:text-zinc-200'
                                  : 'border-dashed border-zinc-300 text-zinc-500 hover:border-pink-400 hover:text-pink-600 dark:border-white/15 dark:text-zinc-400'
                          }`}
                        >
                          {busy === component.id ? <Loader2 size={10} className="animate-spin" /> : on || here ? <Check size={10} /> : <Download size={10} />}
                          {QUANT_LABEL[split(component).quant] ?? split(component).quant}
                          <span className="font-normal opacity-80">{bytes(component.bytes)}</span>
                        </button>
                      );
                    })}
                  </div>
                </div>
              );
            })}
          </div>
        ))}
        <button
          type="button"
          onClick={() => { setOpen(null); window.dispatchEvent(new CustomEvent('studio:open-settings', { detail: 'models' })); }}
          className="mt-1 w-full rounded-lg px-2 py-1.5 text-left text-[11px] font-semibold text-zinc-500 hover:bg-zinc-100 hover:text-pink-600 dark:hover:bg-white/5"
        >
          {t('modelsSection')} · {t('modelsSectionHint')}
        </button>
      </div>
    );
  };

  return (
    <div ref={box} className="relative rounded-xl border border-zinc-200 bg-white p-2 dark:border-white/5 dark:bg-suno-card">
      <div className="flex gap-1.5">
        {button('dit')}
      </div>
      {open && list(open)}
      {downloading && (
        <div className="mt-2 px-1">
          <div className="flex items-center justify-between gap-2 text-[11px] text-zinc-500">
            <span className="flex min-w-0 items-center gap-1.5">
              <Loader2 size={11} className="shrink-0 animate-spin text-pink-500" />
              <span className="truncate">{tt('aceModelDownloading')}</span>
            </span>
            <span className="flex shrink-0 items-center gap-2 tabular-nums">
              {bytes(downloading.downloaded_bytes)} / {bytes(downloading.total_bytes)}
              <button type="button" onClick={() => void cancel()} className="text-zinc-400 hover:text-rose-500" title={t('cancelDownload')}><Square size={11} /></button>
            </span>
          </div>
          <div className="mt-1 h-1 overflow-hidden rounded-full bg-zinc-200 dark:bg-black/30">
            <div className="h-full bg-gradient-to-r from-orange-500 to-pink-600" style={{ width: `${downloading.total_bytes ? Math.min(100, (downloading.downloaded_bytes / downloading.total_bytes) * 100) : 0}%` }} />
          </div>
        </div>
      )}
      {error && <p className="mt-2 px-1 text-[11px] text-rose-600 dark:text-rose-300">{error}</p>}
    </div>
  );
};
