import { QueryClient, useQuery } from '@tanstack/react-query';
import type { Playlist, Song } from '../types';
import { loadNativeLibrarySongs, loadNativePlaylists } from './nativeLibrary';

/**
 * The window's shared reads of the studio service. What several screens show
 * at once - the karaoke switch, the setup state, the assistant - is asked for
 * once and handed to all of them: a request per screen sent the same question
 * eight times on one click, and those waited ahead of the song being played.
 */
export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      // the service is on this computer: a failed read is shown, not retried in a loop
      retry: false,
      refetchOnWindowFocus: false,
    },
  },
});

export async function readJson<T>(path: string): Promise<T> {
  const response = await fetch(path);
  if (!response.ok) throw new Error(`${path} answered ${response.status}`);
  return (await response.json()) as T;
}

export interface KaraokeStatus {
  enabled?: boolean;
  ready?: boolean;
  openrouter_model?: string | null;
}

const karaokeStatusKey = ['karaoke-status'] as const;

/** The karaoke switch and whether its recogniser is ready. */
export function useKaraokeStatus() {
  return useQuery({ queryKey: karaokeStatusKey, queryFn: () => readJson<KaraokeStatus>('/v1/karaoke/status'), refetchInterval: 15_000 });
}

/** The karaoke settings were written, here or by an agent. */
export function karaokeStatusChanged(): void {
  void queryClient.invalidateQueries({ queryKey: karaokeStatusKey });
}

const setupStatusKey = ['setup-status'] as const;

/** Whether the models are on disk and the engine answers; each screen reads the fields it shows. */
export function useSetupStatus<T extends object>() {
  return useQuery({ queryKey: setupStatusKey, queryFn: () => readJson<T>('/setup/status'), refetchInterval: 2_000 });
}

export function setupStatusChanged(): void {
  void queryClient.invalidateQueries({ queryKey: setupStatusKey });
}

const assistantStatusKey = ['assistant-status'] as const;

/** Whether the writing assistant is set up and answers. */
export function useAssistantStatus<T extends object>() {
  return useQuery({ queryKey: assistantStatusKey, queryFn: () => readJson<T>('/v1/assistant/status'), refetchInterval: 5_000 });
}

export function assistantStatusChanged(): void {
  void queryClient.invalidateQueries({ queryKey: assistantStatusKey });
}

const openRouterSettingsKey = ['openrouter-settings'] as const;

/** Whether an OpenRouter key is saved; the key itself stays in the service. */
export function useOpenRouterSettings<T extends object>() {
  return useQuery({ queryKey: openRouterSettingsKey, queryFn: () => readJson<T>('/v1/openrouter/settings'), refetchInterval: 15_000 });
}

export function openRouterSettingsChanged(): void {
  void queryClient.invalidateQueries({ queryKey: openRouterSettingsKey });
}

/** Load of the processor, memory and cards, as often as the service says it measures. */
export function useSystemResources<T extends { poll_interval_ms?: number }>(enabled = true) {
  return useQuery({
    queryKey: ['system-resources'],
    queryFn: () => readJson<T>('/v1/system/resources'),
    refetchInterval: (query) => query.state.data?.poll_interval_ms ?? 1_000,
    enabled,
  });
}


export interface ActivityEntry {
  song_id: string;
  title: string;
  kind: string;
  state: string;
  detail?: string;
}

/** Covers and lyric timings being made for finished songs. */
export function useActivity() {
  return useQuery({
    queryKey: ['activity'],
    queryFn: () => readJson<{ activity?: ActivityEntry[] }>('/v1/activity'),
    select: body => body.activity ?? [],
    refetchInterval: 2_000,
  });
}

const libraryKey = ['library'] as const;
const librarySongsKey = [...libraryKey, 'songs'] as const;
const libraryPlaylistsKey = [...libraryKey, 'playlists'] as const;

/**
 * The library's songs, newest first. Everything that writes a song says so,
 * so a read is kept until then rather than repeated for every screen showing it.
 */
export function useLibrarySongs() {
  return useQuery({ queryKey: librarySongsKey, queryFn: loadNativeLibrarySongs, staleTime: Infinity });
}

export function useLibraryPlaylists() {
  return useQuery({ queryKey: libraryPlaylistsKey, queryFn: loadNativePlaylists, staleTime: Infinity });
}

/** Something was written into the library: every screen showing it reads it again. */
export function libraryChanged(): void {
  void queryClient.invalidateQueries({ queryKey: libraryKey });
}

export function playlistsChanged(): void {
  void queryClient.invalidateQueries({ queryKey: libraryPlaylistsKey });
}

/**
 * A write this window made and already holds the result of. Before the first
 * read has landed there is nothing to apply it to, so the library is read.
 */
export function updateLibrarySongs(change: (songs: Song[]) => Song[]): void {
  if (queryClient.getQueryData(librarySongsKey) === undefined) {
    libraryChanged();
    return;
  }
  queryClient.setQueryData<Song[]>(librarySongsKey, songs => songs && change(songs));
}

export function updateLibraryPlaylists(change: (playlists: Playlist[]) => Playlist[]): void {
  if (queryClient.getQueryData(libraryPlaylistsKey) === undefined) {
    playlistsChanged();
    return;
  }
  queryClient.setQueryData<Playlist[]>(libraryPlaylistsKey, playlists => playlists && change(playlists));
}

/** The library as this window last read it, for code outside a component. */
export function librarySongsNow(): Song[] | undefined {
  return queryClient.getQueryData<Song[]>(librarySongsKey);
}

if (typeof window !== 'undefined') {
  // an agent's change reaches every screen showing what it changed
  window.addEventListener('studio:settings-changed', () => {
    karaokeStatusChanged();
    assistantStatusChanged();
    openRouterSettingsChanged();
  });
  window.addEventListener('studio:models-changed', setupStatusChanged);
  // six stems or a batch delete arrive as one burst and are read once
  let libraryBurst = 0;
  window.addEventListener('studio:library-changed', () => {
    window.clearTimeout(libraryBurst);
    libraryBurst = window.setTimeout(libraryChanged, 200);
  });
}
