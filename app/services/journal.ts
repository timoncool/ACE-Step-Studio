import { useQuery } from '@tanstack/react-query';
import { queryClient, readJson } from './studioQueries';

/** A line of the studio's journal, as the service keeps it. */
export interface JournalLine {
  id: number;
  /** Seconds since 1970, as the library keeps moments. */
  at: string;
  /** `agent` for a change an agent made, `studio` for a message a window showed. */
  source: 'agent' | 'studio';
  tone: string;
  /** An agent's change: what was done and to what kind of thing, and its name. */
  verb: string;
  kind: string;
  target: string;
  /** A studio message, in the words it was shown in. */
  text: string;
}

const journalKey = ['journal'] as const;

/** The journal's latest lines, newest first; every window reads the same. */
export function useJournal() {
  return useQuery({ queryKey: journalKey, queryFn: () => readJson<JournalLine[]>('/v1/journal') });
}

function journalChanged(): void {
  void queryClient.invalidateQueries({ queryKey: journalKey });
}

async function send(path: string, init: RequestInit): Promise<void> {
  const response = await fetch(path, init);
  if (!response.ok) throw new Error(`${path} answered ${response.status}`);
}

/** A message the window showed goes into the journal, so it can be read after it is gone. */
export function noteStudioMessage(text: string, tone: string): void {
  if (!text.trim()) return;
  void send('/v1/journal', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ source: 'studio', text, tone }),
  }).catch(error => console.error('[ERROR] the journal did not take a message:', error));
}

export async function clearJournal(): Promise<void> {
  await send('/v1/journal', { method: 'DELETE' });
  journalChanged();
}

export async function removeJournalLine(id: number): Promise<void> {
  await send(`/v1/journal/${id}`, { method: 'DELETE' });
  journalChanged();
}

if (typeof window !== 'undefined') {
  // a line written by any window or agent reaches every window
  window.addEventListener('studio:journal-changed', journalChanged);
}
