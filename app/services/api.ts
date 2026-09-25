/**
 * Studio API surface.
 *
 * Everything the desktop app talks to is the native Rust server on :8765.
 * There is deliberately no HTTP client for accounts, feeds, comments or the
 * retired ACE Node service: a route that does not exist must not be reachable
 * from the UI at all.
 */
export type { Song, Playlist } from '../types';

/** Media already resolves to a native `/v1/library/media/<id>` path. */
export function getAudioUrl(audioUrl: string | undefined | null): string | undefined {
  return audioUrl || undefined;
}

export interface StudioHealth {
  status: string;
  runtime: string;
  music_engine: { id: string; base_url: string; reachable: boolean };
}

export async function studioHealth(): Promise<StudioHealth> {
  const response = await fetch('/health');
  if (!response.ok) throw new Error(`Studio health request failed (${response.status})`);
  return response.json();
}

export interface EngineLogs {
  engine_id: string;
  lines: string[];
}

export async function engineLogs(): Promise<EngineLogs> {
  const response = await fetch('/v1/engine/logs');
  if (!response.ok) throw new Error(`Engine logs are unavailable (${response.status})`);
  return response.json();
}
