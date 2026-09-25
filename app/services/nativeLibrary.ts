import { apiUrl } from './apiBase';
import { Song } from '../types';
import { STUDIO } from '../studio';

interface NativeLibrarySong {
  id: string;
  title: string;
  audio_path?: string | null;
  caption: string;
  lyrics: string;
  metadata?: Record<string, unknown> | null;
  generation_settings?: Record<string, unknown> | null;
  engine_id: string;
  profile_id?: string | null;
  replay_request?: unknown | null;
  audio_codes?: unknown | null;
  created_at: string;
  updated_at?: string;
}

function audioVersions(metadata: Record<string, unknown>): import('../types').SongVersion[] | undefined {
  const list = metadata.audio_versions;
  if (!Array.isArray(list) || list.length === 0) return undefined;
  return list.flatMap(item => {
    if (!item || typeof item !== 'object') return [];
    const entry = item as Record<string, unknown>;
    if (typeof entry.id !== 'string') return [];
    return [{
      id: entry.id,
      label: typeof entry.label === 'string' ? entry.label : '',
      createdAt: typeof entry.created_at === 'string' ? entry.created_at : '',
      settings: entry.settings && typeof entry.settings === 'object' ? (entry.settings as Record<string, unknown>) : undefined,
    }];
  });
}

interface NativePlaylist {
  id: string;
  name: string;
  description: string | null;
  song_ids: string[];
  created_at: string;
  updated_at: string;
}

export interface NativeSongUpdate {
  title: string;
  audio_path?: string | null;
  caption?: string;
  lyrics?: string;
  metadata?: Record<string, unknown> | null;
  generation_settings?: Record<string, unknown> | null;
  engine_id?: string;
  profile_id?: string | null;
  replay_request?: Record<string, unknown> | null;
  audio_codes?: unknown;
  source?: string;
}

function nativeDate(value: string): Date {
  const epochSeconds = Number(value);
  return Number.isFinite(epochSeconds) && epochSeconds > 0 ? new Date(epochSeconds * 1000) : new Date(value);
}

function numberMetadata(metadata: Record<string, unknown> | null | undefined, key: string): number | undefined {
  const value = metadata?.[key];
  return typeof value === 'number' && Number.isFinite(value) ? value : undefined;
}

function stringMetadata(metadata: Record<string, unknown> | null | undefined, key: string): string | undefined {
  const value = metadata?.[key];
  return typeof value === 'string' ? value : undefined;
}

/** The one user of this desktop studio; every song in its library is theirs. */
export const LOCAL_USER_ID = 'local-studio';

export function mapNativeLibrarySong(song: NativeLibrarySong): Song {
  const metadata = song.metadata ?? {};
  const tags = Array.isArray(metadata.tags) ? metadata.tags.filter((tag): tag is string => typeof tag === 'string') : [];

  return {
    id: song.id,
    userId: LOCAL_USER_ID,
    title: song.title,
    lyrics: song.lyrics,
    style: song.caption,
    // A stored cover always wins. Without one the track still gets real
    // artwork the same way the original studio did — a photo seeded by the
    // song id, so it is stable per track — and AlbumCover falls back to its
    // generated pattern if that image cannot be loaded (offline, blocked).
    coverUrl: typeof metadata.cover_filename === 'string'
      ? apiUrl(`/v1/library/songs/${encodeURIComponent(song.id)}/cover`)
      : `https://picsum.photos/seed/${encodeURIComponent(song.id)}/400/400`,
    duration: (() => {
      const seconds = numberMetadata(metadata, 'duration_seconds') ?? numberMetadata(metadata, 'duration');
      return seconds && seconds > 0 ? `${Math.floor(seconds / 60)}:${String(Math.floor(seconds % 60)).padStart(2, '0')}` : '0:00';
    })(),
    createdAt: nativeDate(song.created_at),
    tags,
    derived: (() => {
      const derived = metadata.derived as { from?: unknown; from_title?: unknown; tool?: unknown; settings?: unknown } | null | undefined;
      if (!derived || typeof derived.from !== 'string' || typeof derived.tool !== 'string') return null;
      return { from: derived.from, fromTitle: typeof derived.from_title === 'string' ? derived.from_title : '', tool: derived.tool, settings: (derived.settings ?? undefined) as Record<string, unknown> | undefined };
    })(),
    // The address changes with the record, so a switched version is fetched
    // again instead of replayed from the browser's cache.
    audioUrl: song.audio_path
      ? apiUrl(`/v1/library/media/${encodeURIComponent(song.id)}${song.updated_at ? `?v=${encodeURIComponent(song.updated_at)}` : ''}`)
      : undefined,
    isPublic: false,
    ditModel: song.engine_id,
    lmModel: song.profile_id || undefined,
    bpm: numberMetadata(metadata, 'bpm'),
    keyScale: stringMetadata(metadata, 'key_scale') ?? stringMetadata(metadata, 'keyScale'),
    timeSignature: stringMetadata(metadata, 'time_signature') ?? stringMetadata(metadata, 'timeSignature'),
    generationParams: song.generation_settings ?? undefined,
    nativeReplayAvailable: Boolean(song.replay_request && song.audio_codes),
    // Karaoke timings, when someone has made them. The video studio and the
    // LRC download have always read this field.
    lrcContent: stringMetadata(metadata, 'lrc'),
    audioVersions: audioVersions(metadata),
    activeVersion: stringMetadata(metadata, 'active_version'),
  };
}

export async function loadNativeLibrarySongs(): Promise<Song[]> {
  const response = await fetch('/v1/library/songs');
  if (!response.ok) {
    throw new Error(`Native library request failed (${response.status})`);
  }
  const songs: NativeLibrarySong[] = await response.json();
  return songs.map(mapNativeLibrarySong);
}

export function mapNativePlaylist(playlist: NativePlaylist): import('../types').Playlist {
  return {
    id: playlist.id,
    name: playlist.name,
    description: playlist.description || undefined,
    songIds: playlist.song_ids,
    created_at: playlist.created_at,
    song_count: playlist.song_ids.length,
    isPublic: false,
  };
}

export async function loadNativePlaylists(): Promise<import('../types').Playlist[]> {
  const response = await fetch('/v1/library/playlists');
  if (!response.ok) throw new Error(`Native playlists request failed (${response.status})`);
  const playlists: NativePlaylist[] = await response.json();
  return playlists.map(mapNativePlaylist);
}

export async function getNativePlaylist(id: string): Promise<import('../types').Playlist | null> {
  const response = await fetch(`/v1/library/playlists/${encodeURIComponent(id)}`);
  if (response.status === 404) return null;
  if (!response.ok) throw new Error(`Native playlist request failed (${response.status})`);
  return mapNativePlaylist(await response.json());
}

export async function createNativePlaylist(name: string, description: string, songIds: string[] = []): Promise<import('../types').Playlist> {
  const response = await fetch('/v1/library/playlists', {
    method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name, description: description || null, song_ids: songIds }),
  });
  if (!response.ok) throw new Error(`Native playlist creation failed (${response.status})`);
  return mapNativePlaylist(await response.json());
}

export async function updateNativePlaylist(id: string, playlist: import('../types').Playlist, songIds = playlist.songIds || []): Promise<import('../types').Playlist> {
  const response = await fetch(`/v1/library/playlists/${encodeURIComponent(id)}`, {
    method: 'PUT', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name: playlist.name, description: playlist.description || null, song_ids: songIds }),
  });
  if (!response.ok) throw new Error(`Native playlist update failed (${response.status})`);
  return mapNativePlaylist(await response.json());
}

export async function deleteNativePlaylist(id: string): Promise<void> {
  const response = await fetch(`/v1/library/playlists/${encodeURIComponent(id)}`, { method: 'DELETE' });
  if (!response.ok) throw new Error(`Native playlist deletion failed (${response.status})`);
}

export async function updateNativeSong(existing: Song, update: Partial<NativeSongUpdate>): Promise<Song> {
  const response = await fetch(`/v1/library/songs/${encodeURIComponent(existing.id)}`, {
    method: 'PUT', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      title: update.title ?? existing.title,
      audio_path: existing.audioUrl ? undefined : null,
      caption: update.caption ?? existing.style,
      lyrics: update.lyrics ?? existing.lyrics,
      metadata: update.metadata ?? { tags: existing.tags, duration: parseDuration(existing.duration), bpm: existing.bpm, keyScale: existing.keyScale, timeSignature: existing.timeSignature },
      generation_settings: update.generation_settings ?? existing.generationParams ?? {},
      engine_id: update.engine_id ?? existing.ditModel ?? STUDIO.engines[0],
      profile_id: update.profile_id ?? existing.lmModel ?? null,
      replay_request: update.replay_request ?? null,
      audio_codes: update.audio_codes ?? null,
      source: update.source ?? 'local_generation',
    }),
  });
  if (!response.ok) throw new Error(`Native song update failed (${response.status})`);
  return mapNativeLibrarySong(await response.json());
}

export async function deleteNativeSong(id: string): Promise<void> {
  const response = await fetch(`/v1/library/songs/${encodeURIComponent(id)}`, { method: 'DELETE' });
  if (!response.ok) throw new Error(`Native song deletion failed (${response.status})`);
}

function parseDuration(value: string): number | undefined {
  const match = /^(\d+):(\d{2})$/.exec(value);
  return match ? Number(match[1]) * 60 + Number(match[2]) : undefined;
}
