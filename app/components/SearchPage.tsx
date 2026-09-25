import React, { useMemo, useState } from 'react';
import { Check, Copy, ListMusic, Music2, Search as SearchIcon, X } from 'lucide-react';
import { Song, Playlist } from '../types';
import { useI18n } from '../context/I18nContext';
import { GENRE_KEYS } from '../data/genres';
import { AlbumCover } from './AlbumCover';
import { captionSummary } from '../services/examples';

/**
 * Local search.
 *
 * ACE Studio searched a public feed. This desktop studio has no feed and no
 * accounts, so search does the thing that is actually useful here: it looks
 * through the tracks and playlists stored on this machine, across titles,
 * captions, lyrics and tags. The genre palette stays because it is a real
 * authoring aid — clicking a genre copies it for use in a caption.
 */

interface SearchPageProps {
  songs: Song[];
  playlists: Playlist[];
  onPlaySong?: (song: Song, list?: Song[]) => void;
  currentSong?: Song | null;
  isPlaying?: boolean;
  onNavigateToPlaylist?: (playlistId: string) => void;
}

const matchesSong = (song: Song, needle: string) =>
  [song.title, song.style, song.lyrics, ...(song.tags || [])]
    .some(value => value?.toLocaleLowerCase().includes(needle));

const matchesPlaylist = (playlist: Playlist, needle: string) =>
  [playlist.name, playlist.description || ''].some(value => value.toLocaleLowerCase().includes(needle));

export const SearchPage: React.FC<SearchPageProps> = ({
  songs,
  playlists,
  onPlaySong,
  currentSong,
  isPlaying,
  onNavigateToPlaylist,
}) => {
  const { t } = useI18n();
  const [query, setQuery] = useState('');
  const [copiedTag, setCopiedTag] = useState<string | null>(null);

  const needle = query.trim().toLocaleLowerCase();
  const results = useMemo(() => {
    if (!needle) return null;
    return {
      songs: songs.filter(song => matchesSong(song, needle)),
      playlists: playlists.filter(playlist => matchesPlaylist(playlist, needle)),
    };
  }, [needle, playlists, songs]);

  const copyTag = async (tag: string) => {
    try {
      await navigator.clipboard.writeText(tag);
      setCopiedTag(tag);
      window.setTimeout(() => setCopiedTag(current => (current === tag ? null : current)), 1500);
    } catch {
      // Clipboard permission is not something the user needs an error about.
    }
  };

  const recent = useMemo(() => songs.slice(0, 12), [songs]);

  return (
    <div className="flex-1 overflow-y-auto bg-white px-5 py-6 dark:bg-suno md:px-8">
      <div className="mx-auto max-w-5xl">
        <h1 className="text-2xl font-bold text-zinc-900 dark:text-white">{t('search')}</h1>
        <p className="mt-1 text-sm text-zinc-500 dark:text-zinc-400">
          {t('searchLocalHint')}
        </p>

        <div className="relative mt-5">
          <SearchIcon size={18} className="pointer-events-none absolute left-3.5 top-1/2 -translate-y-1/2 text-zinc-400" />
          <input
            value={query}
            onChange={event => setQuery(event.target.value)}
            placeholder={t('searchPlaceholder') || 'Search your library…'}
            className="w-full rounded-xl border border-zinc-200 bg-zinc-50 py-3 pl-11 pr-10 text-sm text-zinc-900 outline-none focus:border-pink-500 dark:border-white/10 dark:bg-black/20 dark:text-white"
          />
          {query && (
            <button type="button" onClick={() => setQuery('')} className="absolute right-3 top-1/2 -translate-y-1/2 text-zinc-400 hover:text-pink-500" title={t('clear') || 'Clear'}>
              <X size={16} />
            </button>
          )}
        </div>

        {results && (
          <div className="mt-7 space-y-8">
            <section>
              <h2 className="mb-3 text-sm font-bold uppercase tracking-wide text-zinc-500">
                {t('songs') || 'Tracks'} · {results.songs.length}
              </h2>
              {results.songs.length === 0
                ? <p className="text-sm text-zinc-500">{t('noResults') || 'Nothing in the local library matches this query.'}</p>
                : <SongGrid songs={results.songs} onPlaySong={onPlaySong} currentSong={currentSong} isPlaying={isPlaying} />}
            </section>

            {results.playlists.length > 0 && (
              <section>
                <h2 className="mb-3 text-sm font-bold uppercase tracking-wide text-zinc-500">
                  {t('playlists') || 'Playlists'} · {results.playlists.length}
                </h2>
                <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-4">
                  {results.playlists.map(playlist => (
                    <button
                      key={playlist.id}
                      type="button"
                      onClick={() => onNavigateToPlaylist?.(playlist.id)}
                      className="flex items-center gap-3 rounded-xl border border-zinc-200 p-3 text-left transition-colors hover:border-pink-400 dark:border-white/10 dark:hover:border-pink-500/60"
                    >
                      <span className="grid h-10 w-10 shrink-0 place-items-center rounded-lg bg-gradient-to-br from-pink-500 to-purple-600 text-white">
                        <ListMusic size={18} />
                      </span>
                      <span className="min-w-0">
                        <span className="block truncate text-sm font-semibold text-zinc-900 dark:text-white">{playlist.name}</span>
                        <span className="block text-xs text-zinc-500">{playlist.songIds?.length ?? playlist.song_count ?? 0}</span>
                      </span>
                    </button>
                  ))}
                </div>
              </section>
            )}
          </div>
        )}

        {!results && (
          <div className="mt-7 space-y-8">
            {recent.length > 0 && (
              <section>
                <h2 className="mb-3 text-sm font-bold uppercase tracking-wide text-zinc-500">{t('recent') || 'Recent tracks'}</h2>
                <SongGrid songs={recent} onPlaySong={onPlaySong} currentSong={currentSong} isPlaying={isPlaying} />
              </section>
            )}

            <section>
              <h2 className="mb-3 text-sm font-bold uppercase tracking-wide text-zinc-500">{t('genres')}</h2>
              <div className="flex flex-wrap gap-2">
                {GENRE_KEYS.map(genre => (
                  <button
                    key={genre}
                    type="button"
                    onClick={() => void copyTag(genre)}
                    title={t('copyToClipboard') || 'Copy'}
                    className="inline-flex items-center gap-1.5 rounded-full border border-zinc-200 px-3 py-1.5 text-xs font-medium text-zinc-700 transition-colors hover:border-pink-400 hover:text-pink-600 dark:border-white/10 dark:text-zinc-300"
                  >
                    {genre}
                    {copiedTag === genre ? <Check size={12} className="text-emerald-500" /> : <Copy size={11} className="text-zinc-400" />}
                  </button>
                ))}
              </div>
            </section>
          </div>
        )}
      </div>
    </div>
  );
};

const SongGrid: React.FC<{
  songs: Song[];
  onPlaySong?: (song: Song, list?: Song[]) => void;
  currentSong?: Song | null;
  isPlaying?: boolean;
}> = ({ songs, onPlaySong, currentSong, isPlaying }) => (
  <div className="grid grid-cols-2 gap-4 sm:grid-cols-3 lg:grid-cols-4 xl:grid-cols-6">
    {songs.map(song => {
      const active = currentSong?.id === song.id && isPlaying;
      return (
        <button
          key={song.id}
          type="button"
          onClick={() => onPlaySong?.(song, songs)}
          className="group text-left"
        >
          <span className="relative block aspect-square overflow-hidden rounded-xl">
            <AlbumCover seed={song.id} size="full" coverUrl={song.coverUrl} className="rounded-xl" />
            <span className={`absolute inset-0 grid place-items-center bg-black/40 transition-opacity ${active ? 'opacity-100' : 'opacity-0 group-hover:opacity-100'}`}>
              <Music2 size={22} className="text-white" />
            </span>
          </span>
          <span className="mt-2 block truncate text-sm font-semibold text-zinc-900 dark:text-white">{song.title}</span>
          <span className="block truncate text-xs text-zinc-500">{captionSummary(song.style)}</span>
        </button>
      );
    })}
  </div>
);
