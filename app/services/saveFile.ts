import { apiUrl } from './apiBase';

/**
 * "Save as" for anything the window hands the user.
 *
 * Windows' own Save dialog asks where, starting in the folder chosen last; the
 * service then writes the file - one it serves, read from its own address, or
 * bytes the window made - and reports how far it has got. The files panel
 * shows each save from those reports.
 */

export type SaveSource = { url: string } | { blob: Blob };

export interface SavingFile {
  id: string;
  name: string;
  path?: string;
  state: 'saving' | 'done' | 'error';
  written?: number;
  total?: number | null;
  error?: string;
}

const EVENT = 'studio:saving';

function tell(file: Partial<SavingFile> & { id: string }): void {
  window.dispatchEvent(new CustomEvent(EVENT, { detail: file }));
}

/** Every change to a save, from this window or the service's reports. */
export function onSaving(listener: (file: Partial<SavingFile> & { id: string }) => void): () => void {
  const handler = (event: Event) => listener((event as CustomEvent<Partial<SavingFile> & { id: string }>).detail);
  window.addEventListener(EVENT, handler);
  return () => window.removeEventListener(EVENT, handler);
}

async function answer(response: Response): Promise<Record<string, unknown>> {
  const body = await response.json().catch(() => null);
  if (!response.ok) throw new Error(body?.error || `HTTP ${response.status}`);
  return body ?? {};
}

/**
 * Asks where, then saves. A cancelled dialog saves nothing; anything that goes
 * wrong is shown in the files panel with its reason, so callers have nothing
 * to catch.
 */
export async function saveFile(name: string, source: SaveSource): Promise<void> {
  let id = `failed-${Date.now()}`;
  try {
    const place = await answer(await fetch(apiUrl('/v1/files/save'), {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name }),
    }));
    if (place.cancelled) return;
    id = String(place.id);
    tell({ id, name: String(place.name ?? name), path: String(place.path), state: 'saving', written: 0 });
    const body = 'url' in source
      ? { headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ url: source.url }) }
      : { headers: { 'Content-Type': 'application/octet-stream' }, body: source.blob };
    await answer(await fetch(apiUrl(`/v1/files/save/${encodeURIComponent(id)}`), { method: 'POST', ...body }));
  } catch (problem) {
    tell({ id, name, state: 'error', error: problem instanceof Error ? problem.message : String(problem) });
  }
}

/** Explorer, open on a saved file. */
export async function revealSaved(path: string): Promise<void> {
  await answer(await fetch(apiUrl('/v1/files/reveal'), {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ path }),
  }));
}
