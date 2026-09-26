/**
 * LoRA adapters: the library the studio keeps, and what a request carries.
 *
 * The engine declares its slots, the parts of the model an adapter can change,
 * each with a role (composition, sound) the interface names. A request carries
 * one strength per slot; the service spells it the engine's way.
 */

import { useEffect, useState } from 'react';
import type { Language } from '../i18n/translations';

export type AdapterText = string | Partial<Record<Language, string>>;

/** The two DiT widths an adapter can be trained for; `lm` marks one of the planner. */
export type DitFamily = '2b' | 'xl' | 'lm';

export const familyLabel = (family: DitFamily) => (family === 'xl' ? 'XL' : family === '2b' ? '2B' : 'LM');

export interface AdapterSlot {
  id: string;
  role: string;
}

export interface InstalledAdapter {
  id: string;
  engine: string;
  name: AdapterText;
  description: AdapterText;
  kind: string;
  trigger?: string | null;
  author?: string | null;
  page?: string | null;
  scales: Record<string, number>;
  range?: [number, number] | null;
  /** The DiT width the weights fit, when known. */
  model?: DitFamily | null;
  slots: string[];
  origin: { type: 'catalog' | 'imported' | 'trained' | 'hub'; catalog_id?: string; repo?: string; revision?: string; file?: string };
  created_at: string;
  bytes: number;
  error?: string | null;
}

export interface OfferedAdapter {
  id: string;
  name: AdapterText;
  description: AdapterText;
  kind: string;
  author?: string | null;
  page?: string | null;
  trigger?: string | null;
  slots: string[];
  bytes: number;
  installed: boolean;
  model?: DitFamily | null;
  likes: number;
  downloads: number;
}

export interface AdapterDownload {
  asset_id: string;
  downloaded_bytes: number;
  total_bytes: number;
  done: boolean;
  error?: string | null;
}

export interface AdapterState {
  slots: AdapterSlot[];
  installed: InstalledAdapter[];
  catalog: OfferedAdapter[];
  /** The width of the DiT the studio renders with now. */
  model?: DitFamily | null;
  engine_checked: boolean;
  download: AdapterDownload | null;
  /** The catalogue entries of the download running now. */
  installing: string[];
}

/** One adapter of a song: its folder and a strength per slot. */
export interface AdapterUse {
  id: string;
  scales: Record<string, number>;
}

export function localized(text: AdapterText | undefined, language: Language): string {
  if (!text) return '';
  if (typeof text === 'string') return text;
  return text[language] || text.en || Object.values(text).find(Boolean) || '';
}

export async function fetchAdapters(): Promise<AdapterState> {
  const response = await fetch('/v1/adapters');
  if (!response.ok) throw new Error(`LoRA list: HTTP ${response.status}`);
  return response.json();
}

/** Fetches catalogue entries as one download, the way the model screen fetches its components. */
export async function installCatalogAdapters(ids: string[]): Promise<void> {
  const response = await fetch('/v1/adapters/install', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ ids }),
  });
  if (!response.ok) {
    const body = await response.json().catch(() => null);
    throw new Error(body?.error || `LoRA download: HTTP ${response.status}`);
  }
}

/** A repository of adapters on Hugging Face. */
export interface HubRepo {
  repo: string;
  author: string;
  likes: number;
  downloads: number;
  updated?: string | null;
  tags: string[];
}

/** One weight file of a repository and the adapter it becomes. */
export interface HubFile {
  path: string;
  bytes: number;
  adapter_id: string;
  installed: boolean;
  model?: DitFamily | null;
  format?: string | null;
  /** Why the engine cannot use the file, when it cannot. */
  problem?: string | null;
}

export interface HubListing {
  repo: string;
  revision: string;
  page: string;
  files: HubFile[];
}

async function hubJson<T>(url: string, init?: RequestInit): Promise<T> {
  const response = await fetch(url, init);
  const body = await response.json().catch(() => null);
  if (!response.ok) throw new Error(body?.error || `Hugging Face: HTTP ${response.status}`);
  return body as T;
}

export async function searchHub(query: string): Promise<HubRepo[]> {
  return (await hubJson<{ repos: HubRepo[] }>(`/v1/adapters/hub?q=${encodeURIComponent(query)}`)).repos;
}

/** A repository's files; `reference` may be its id or any link into it, a link to one file naming that file. */
export async function hubFiles(reference: string): Promise<{ listing: HubListing; file: string | null }> {
  return hubJson(`/v1/adapters/hub/files?repo=${encodeURIComponent(reference)}`);
}

export async function installHubAdapters(repo: string, paths: string[]): Promise<void> {
  await hubJson('/v1/adapters/hub/install', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ repo, paths }),
  });
}

/** Whether typed text is a link or an `owner/name` id rather than words to search for. */
export function looksLikeHubReference(text: string): boolean {
  const value = text.trim();
  return /huggingface\.co\//i.test(value) || /^[\w.-]+\/[\w.-]+$/.test(value);
}

export async function cancelAdapterDownload(): Promise<void> {
  await fetch('/v1/adapters/cancel', { method: 'POST' });
}

export async function importAdapter(name: string, files: File[]): Promise<InstalledAdapter> {
  const form = new FormData();
  form.append('name', name);
  for (const file of files) form.append('files', file, file.name);
  const response = await fetch('/v1/adapters/import', { method: 'POST', body: form });
  const body = await response.json().catch(() => ({}));
  if (!response.ok) throw new Error(body.error || `LoRA import: HTTP ${response.status}`);
  return body;
}

export async function updateAdapter(id: string, patch: { name?: string; trigger?: string; scales?: Record<string, number> }): Promise<void> {
  const response = await fetch(`/v1/adapters/${encodeURIComponent(id)}`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(patch),
  });
  if (!response.ok) {
    const body = await response.json().catch(() => ({}));
    throw new Error(body.error || `LoRA update: HTTP ${response.status}`);
  }
}

export async function deleteAdapter(id: string): Promise<void> {
  const response = await fetch(`/v1/adapters/${encodeURIComponent(id)}`, { method: 'DELETE' });
  if (!response.ok && response.status !== 204) throw new Error(`LoRA delete: HTTP ${response.status}`);
}

/** The strengths a newly picked adapter starts at: its own, else full on every slot it touches. */
export function startingScales(adapter: InstalledAdapter, slots: AdapterSlot[]): Record<string, number> {
  const touched = adapter.slots.length ? adapter.slots : slots.map(slot => slot.id);
  const scales: Record<string, number> = {};
  for (const slot of touched) scales[slot] = adapter.scales[slot] ?? 1;
  return scales;
}

/**
 * The adapters a stored request used, read back from the engine's spelling
 * (`name` and `<slot>_scale`), so reusing a song brings its LoRA back.
 */
export function usesFromSettings(settings: Record<string, unknown>): AdapterUse[] {
  const list = settings.adapters;
  if (!Array.isArray(list)) return [];
  const uses: AdapterUse[] = [];
  for (const item of list) {
    if (!item || typeof item !== 'object') continue;
    const entry = item as Record<string, unknown>;
    const id = typeof entry.id === 'string' ? entry.id : typeof entry.name === 'string' ? entry.name : '';
    if (!id) continue;
    // A saved prompt carries {id, scales}; a stored song the engine's {name, <slot>_scale}.
    const source =
      entry.scales && typeof entry.scales === 'object'
        ? Object.entries(entry.scales as Record<string, unknown>)
        : Object.entries(entry).flatMap(([key, value]) => (key.endsWith('_scale') ? [[key.slice(0, -'_scale'.length), value] as const] : []));
    const scales: Record<string, number> = {};
    for (const [slot, value] of source) {
      if (typeof value === 'number' && Number.isFinite(value) && value !== 0) scales[slot] = value;
    }
    uses.push({ id, scales });
  }
  return uses;
}

export const megabytes = (bytes: number) => `${Math.max(1, Math.round(bytes / 1024 / 1024))} MB`;

/** Installed adapters and slots, kept current while the library changes. */
export function useAdapterLibrary(): { installed: InstalledAdapter[]; slots: AdapterSlot[]; model: DitFamily | null } {
  const [library, setLibrary] = useState<{ installed: InstalledAdapter[]; slots: AdapterSlot[]; model: DitFamily | null }>({ installed: [], slots: [], model: null });
  useEffect(() => {
    const read = () =>
      void fetchAdapters()
        .then(state => setLibrary({ installed: state.installed, slots: state.slots, model: state.model ?? null }))
        .catch(() => undefined);
    read();
    window.addEventListener('studio:adapters-changed', read);
    window.addEventListener('studio:models-changed', read);
    return () => {
      window.removeEventListener('studio:adapters-changed', read);
      window.removeEventListener('studio:models-changed', read);
    };
  }, []);
  return library;
}
