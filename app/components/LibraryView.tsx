import React, { useRef, useState } from 'react';
import { Song, Playlist } from '../types';
import { Heart, Plus, Play, Upload, Loader2, MoreHorizontal, ListMusic } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { useSongActions } from '../context/SongActionsContext';
import { deleteNativePlaylist } from '../services/nativeLibrary';
import { updateLibraryPlaylists } from '../services/studioQueries';
import { named } from '../services/accessibleName';
import { ConfirmDialog } from './ConfirmDialog';
import { PlaylistCover, PlaylistEditModal, PlaylistMenu, playlistSongs } from './PlaylistEditor';
import { SongList } from './SongList';
import { SONG_ORDERS, type SongOrder } from '../services/songOrder';
import { AUDIO_ACCEPT } from '../services/audioFiles';

/** The liked songs are read from the latest like first. */
const LIKED_ORDERS: SongOrder[] = ['likedNewest', ...SONG_ORDERS];

type LibraryTab = 'all' | 'playlists' | 'liked' | 'import';
const TAB_KEY = 'studio.library.tab';
/** The tab last open: coming back from a playlist lands on the playlists. */
function readTab(): LibraryTab {
  try {
    const kept = window.sessionStorage.getItem(TAB_KEY);
    return kept === 'playlists' || kept === 'liked' || kept === 'import' ? kept : 'all';
  } catch {
    return 'all';
  }
}

function shuffled<T>(items: T[]): T[] {
  const copy = [...items];
  for (let index = copy.length - 1; index > 0; index -= 1) {
    const other = Math.floor(Math.random() * (index + 1));
    [copy[index], copy[other]] = [copy[other], copy[index]];
  }
  return copy;
}

interface LibraryViewProps {
  allSongs: Song[];
  likedSongs: Song[];
  playlists: Playlist[];
  onPlaySong: (song: Song, list?: Song[]) => void;
  /** What the player holds, so its row says so. */
  currentSong: Song | null;
  isPlaying: boolean;
  onCreatePlaylist: () => void;
  onSelectPlaylist: (playlist: Playlist) => void;
  /** Plays a playlist's songs from the first given, the playlist named as the queue's source. */
  onPlayPlaylist: (playlist: Playlist, songs: Song[]) => void;
  /** Says why a change did not go through. */
  onError: (message: string) => void;
  onImported?: (count: number) => void;
  likedSongIds: Set<string>;
  onToggleLike: (songId: string) => void;
  onAddToPlaylist: (songs: Song | Song[]) => void;
  onDeleteMany: (songs: Song[]) => void;
  onMidiMany: (songs: Song[]) => void;
  /** The song the details show; a row click chooses it without playing it. */
  selectedSong: Song | null;
  onSelect: (song: Song) => void;
  /** Opens a song's details, or closes them when they are its. */
  onShowDetails: (song: Song) => void;
  onOpenCoverRegen?: (song: Song) => void;
  detailsSongId: string | null;
}

export const LibraryView: React.FC<LibraryViewProps> = ({ 
    allSongs,
    likedSongs, 
    playlists, 
    onPlaySong, 
    currentSong,
    isPlaying,
    onCreatePlaylist,
    onSelectPlaylist,
    onPlayPlaylist,
    onError,
    onImported,
    likedSongIds,
    onToggleLike,
    onAddToPlaylist,
    onDeleteMany,
    onMidiMany,
    selectedSong,
    onSelect,
    onShowDetails,
    onOpenCoverRegen,
    detailsSongId,
}) => {
    const { t, songCount } = useI18n();
    const songActions = useSongActions();
    const [activeTab, setTab] = useState<LibraryTab>(readTab);
    const setActiveTab = (tab: LibraryTab) => {
        setTab(tab);
        try { window.sessionStorage.setItem(TAB_KEY, tab); } catch (error) { console.error('The library tab could not be kept:', error); }
    };
    const [playlistMenu, setPlaylistMenu] = useState<{ playlist: Playlist; at: { x: number; y: number } } | null>(null);
    const [editing, setEditing] = useState<Playlist | null>(null);
    const [deleting, setDeleting] = useState<Playlist | null>(null);
    const playable = (playlist: Playlist) => playlistSongs(playlist, allSongs).filter(song => song.audioUrl && !song.isGenerating);
    const removePlaylist = async (playlist: Playlist) => {
        setDeleting(null);
        try {
            await deleteNativePlaylist(playlist.id);
            updateLibraryPlaylists(lists => lists.filter(entry => entry.id !== playlist.id));
        } catch (error) {
            onError(error instanceof Error ? error.message : String(error));
        }
    };
    const [importing, setImporting] = useState(false);
    const [importError, setImportError] = useState<string | null>(null);
    const importInput = useRef<HTMLInputElement | null>(null);

    /// Imports an existing MP3, WAV, FLAC or OGG into the local library. The service
    /// stores the media and the row atomically, rolling both back on failure, and names
    /// the track by the file's own title tag before its file name.
    const importAudio = async (files: FileList | null) => {
        if (!files || files.length === 0) return;
        setImporting(true);
        setImportError(null);
        try {
            let done = 0;
            for (const file of Array.from(files)) {
                const form = new FormData();
                form.append('audio', file, file.name);
                const response = await fetch('/v1/library/import', { method: 'POST', body: form });
                if (!response.ok) {
                    const body = await response.json().catch(() => null);
                    throw new Error(body?.error || `${file.name}: import failed (${response.status})`);
                }
                done += 1;
            }
            onImported?.(done);
        } catch (reason) {
            setImportError(reason instanceof Error ? reason.message : 'Import failed.');
        } finally {
            setImporting(false);
        }
    };

    // the list of a tab: the same rows, search, filters and selection as the create page's
    const songList = (songs: Song[], orderKey: string, orders: SongOrder[], defaultOrder: SongOrder, emptyMessage?: string) => (
        <SongList
            embedded
            songs={songs}
            librarySongs={allSongs}
            currentSong={currentSong}
            selectedSong={selectedSong}
            likedSongIds={likedSongIds}
            isPlaying={isPlaying}
            onPlay={onPlaySong}
            onSelect={onSelect}
            onToggleLike={onToggleLike}
            onAddToPlaylist={song => onAddToPlaylist(song)}
            onAddManyToPlaylist={chosen => onAddToPlaylist(chosen)}
            onShowDetails={onShowDetails}
            onOpenCoverRegen={onOpenCoverRegen}
            detailsSongId={detailsSongId}
            onDeleteMany={onDeleteMany}
            onMidiMany={onMidiMany}
            orderKey={orderKey}
            orders={orders}
            defaultOrder={defaultOrder}
            emptyMessage={emptyMessage}
        />
    );
    const tabButton = (tab: LibraryTab, label: string, count?: number) => (
        <button
            type="button"
            role="tab"
            aria-selected={activeTab === tab}
            onClick={() => setActiveTab(tab)}
            className={`relative whitespace-nowrap pb-3 text-sm font-bold transition-colors ${activeTab === tab ? 'text-zinc-900 dark:text-white' : 'text-zinc-500 hover:text-zinc-900 dark:hover:text-white'}`}
        >
            {label}
            {count !== undefined && <span className="ml-1.5 text-xs font-medium text-zinc-400">{count}</span>}
            {activeTab === tab && <div className="absolute bottom-0 left-0 right-0 h-0.5 rounded-full bg-green-500"></div>}
        </button>
    );

    return (
        <>
        <div className="min-w-0 flex-1 overflow-y-auto bg-white p-4 pb-32 transition-colors duration-300 dark:bg-black sm:p-6 lg:p-10">
             <div className="mb-8 flex flex-col items-start justify-between gap-3 sm:flex-row sm:items-center">
                <h1 className="min-w-0 truncate text-2xl font-bold text-zinc-900 dark:text-white sm:text-3xl">{t('yourLibrary')}</h1>
                <button 
                    onClick={onCreatePlaylist}
                    className="flex items-center gap-2 bg-zinc-900 dark:bg-zinc-800 hover:bg-zinc-800 dark:hover:bg-zinc-700 text-white px-4 py-2 rounded-full font-medium transition-colors shadow-lg shadow-zinc-900/10 dark:shadow-none"
                >
                    <Plus size={18} />
                    <span>{t('newPlaylist')}</span>
                </button>
             </div>

             {/* Tabs, and the order of the list they show */}
             <div role="tablist" className="mb-8 flex min-w-0 flex-wrap items-center gap-x-5 gap-y-1 border-b border-zinc-200 pb-1 dark:border-white/10">
                 {tabButton('all', t('allSongs'), allSongs.length)}
                 {tabButton('liked', t('likedTab'), likedSongs.length)}
                 {tabButton('playlists', t('playlists'), playlists.length)}
                 {tabButton('import', t('importAudioTab'))}
             </div>

             {/* Content */}
             {activeTab === 'all' && (allSongs.length === 0
                 ? <div className="text-sm text-zinc-500 dark:text-zinc-400">{t('noSongsYet')}</div>
                 : songList(allSongs, 'library', SONG_ORDERS, 'newest'))}
             {activeTab === 'liked' && likedSongs.length === 0 && (
                 <div className="flex flex-col items-center justify-center gap-3 rounded-2xl border border-dashed border-zinc-200 bg-zinc-50 px-6 py-16 text-center dark:border-white/5 dark:bg-white/2">
                     <div className="flex h-16 w-16 items-center justify-center rounded-full bg-zinc-100 dark:bg-white/5"><Heart size={30} className="text-zinc-400" /></div>
                     <p className="font-bold text-zinc-900 dark:text-white">{t('likedEmptyTitle')}</p>
                     <p className="max-w-sm text-sm text-zinc-500">{t('likedEmptyHint')}</p>
                     <button type="button" onClick={() => setActiveTab('all')} className="mt-2 rounded-full bg-zinc-900 px-4 py-2 text-sm font-bold text-white dark:bg-white dark:text-black">{t('allSongs')}</button>
                 </div>
             )}
             {activeTab === 'liked' && likedSongs.length > 0 && (
                 <div>
                    <div className="group mb-8 flex cursor-pointer flex-col items-start gap-4 rounded-xl border border-zinc-200 bg-linear-to-b from-indigo-500/10 to-zinc-50 p-4 transition-colors hover:bg-zinc-100 dark:border-white/5 dark:from-indigo-800/50 dark:to-zinc-900/50 dark:hover:bg-white/5 sm:flex-row sm:items-end sm:gap-6 sm:p-6" onClick={() => likedSongs.length > 0 && onPlaySong(likedSongs[0], likedSongs)}>
                         <div className="flex h-28 w-28 shrink-0 items-center justify-center rounded-sm bg-linear-to-br from-indigo-500 to-purple-400 shadow-2xl sm:h-40 sm:w-40">
                            <Heart fill="white" size={64} className="text-white" />
                         </div>
                         <div className="mb-2">
                             <h2 className="text-sm font-bold uppercase text-zinc-500 dark:text-white mb-2">{t('playlist')}</h2>
                             <h1 className="mb-4 text-3xl font-extrabold text-zinc-900 dark:text-white sm:text-5xl">{t('likedSongs')}</h1>
                             <div className="text-sm text-zinc-500 dark:text-zinc-300 font-medium">
                                 {songCount(likedSongs.length)}
                             </div>
                         </div>
                         <div className="ml-auto mb-2 opacity-0 group-hover:opacity-100 transition-opacity">
                             <div className="w-14 h-14 rounded-full bg-green-500 flex items-center justify-center shadow-lg hover:scale-105 transition-transform">
                                <Play fill="black" className="text-black ml-1" size={28} />
                             </div>
                         </div>
                    </div>

                    {songList(likedSongs, 'liked', LIKED_ORDERS, 'likedNewest')}
                 </div>
             )}
             {activeTab === 'playlists' && playlists.length === 0 && (
                 <div className="flex flex-col items-center justify-center gap-3 rounded-2xl border border-dashed border-zinc-200 bg-zinc-50 px-6 py-16 text-center dark:border-white/5 dark:bg-white/2">
                     <div className="flex h-16 w-16 items-center justify-center rounded-full bg-zinc-100 dark:bg-white/5"><ListMusic size={30} className="text-zinc-400" /></div>
                     <p className="font-bold text-zinc-900 dark:text-white">{t('playlistsEmptyTitle')}</p>
                     <p className="max-w-sm text-sm text-zinc-500">{t('playlistsEmptyHint')}</p>
                     <button type="button" onClick={onCreatePlaylist} className="mt-2 flex items-center gap-2 rounded-full bg-zinc-900 px-4 py-2 text-sm font-bold text-white dark:bg-white dark:text-black"><Plus size={16} />{t('createPlaylist')}</button>
                 </div>
             )}
             {activeTab === 'playlists' && playlists.length > 0 && (
                 <div className="grid grid-cols-1 gap-4 min-[440px]:grid-cols-2 sm:gap-6 md:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5">
                     {playlists.map((playlist) => {
                         const songs = playlistSongs(playlist, allSongs);
                         const ready = playable(playlist);
                         return (
                         <div
                             key={playlist.id}
                             data-mcp-context={`playlist ${playlist.id}: ${playlist.name}`}
                             className="group relative cursor-pointer rounded-lg border border-zinc-200 bg-white p-4 transition-all hover:border-zinc-300 hover:shadow-lg dark:border-white/5 dark:bg-zinc-900/40 dark:hover:border-white/10 dark:hover:bg-zinc-900"
                             onClick={() => onSelectPlaylist(playlist)}
                             onContextMenu={(event) => {
                                 event.preventDefault();
                                 setPlaylistMenu({ playlist, at: { x: event.clientX, y: event.clientY } });
                             }}
                         >
                             <div className="relative mb-4 aspect-square overflow-hidden rounded-md bg-zinc-100 dark:bg-zinc-800">
                                 <PlaylistCover playlist={playlist} songs={songs} className="h-full w-full" />
                                 {ready.length > 0 && (
                                     <button
                                         type="button"
                                         onClick={(event) => { event.stopPropagation(); onPlayPlaylist(playlist, ready); }}
                                         {...named(`${t('play')}: ${playlist.name}`)}
                                         className="absolute bottom-2 right-2 flex h-12 w-12 translate-y-2 items-center justify-center rounded-full bg-green-500 text-black opacity-0 shadow-xl transition-all hover:scale-105 group-hover:translate-y-0 group-hover:opacity-100 focus-visible:translate-y-0 focus-visible:opacity-100"
                                     >
                                         <Play size={22} fill="currentColor" className="ml-0.5" />
                                     </button>
                                 )}
                             </div>
                             <div className="flex items-start justify-between gap-2">
                                 <div className="min-w-0">
                                     <h3 className="truncate font-bold text-zinc-900 dark:text-white">{playlist.name}</h3>
                                     <p className="line-clamp-2 text-sm text-zinc-500 dark:text-zinc-400">{playlist.description || songCount(songs.length)}</p>
                                 </div>
                                 <button
                                     type="button"
                                     onClick={(event) => {
                                         event.stopPropagation();
                                         const box = event.currentTarget.getBoundingClientRect();
                                         setPlaylistMenu({ playlist, at: { x: box.right - 224, y: box.bottom + 4 } });
                                     }}
                                     aria-haspopup="menu"
                                     {...named(t('moreActions'))}
                                     className="shrink-0 rounded-full p-1 text-zinc-400 opacity-0 transition-opacity hover:text-zinc-900 group-hover:opacity-100 focus-visible:opacity-100 dark:hover:text-white"
                                 >
                                     <MoreHorizontal size={18} />
                                 </button>
                             </div>
                         </div>
                         );
                     })}
                 </div>
             )}
             {playlistMenu && (
                 <PlaylistMenu
                     at={playlistMenu.at}
                     empty={playable(playlistMenu.playlist).length === 0}
                     onClose={() => setPlaylistMenu(null)}
                     actions={{
                         play: () => { const ready = playable(playlistMenu.playlist); if (ready.length) onPlayPlaylist(playlistMenu.playlist, ready); },
                         shuffle: () => { const ready = shuffled(playable(playlistMenu.playlist)); if (ready.length) onPlayPlaylist(playlistMenu.playlist, ready); },
                         queue: (next) => songActions.queue?.(playable(playlistMenu.playlist), next),
                         edit: () => setEditing(playlistMenu.playlist),
                         remove: () => setDeleting(playlistMenu.playlist),
                     }}
                 />
             )}
             <PlaylistEditModal playlist={editing} onClose={() => setEditing(null)} onError={onError} />
             <ConfirmDialog
                 isOpen={Boolean(deleting)}
                 title={t('deletePlaylist')}
                 message={t('deletePlaylistConfirm')}
                 confirmLabel={t('deletePlaylist')}
                 onConfirm={() => deleting && void removePlaylist(deleting)}
                 onCancel={() => setDeleting(null)}
             />
             {activeTab === 'import' && (
                 <div
                     onDragOver={(event) => event.preventDefault()}
                     onDrop={(event) => { event.preventDefault(); void importAudio(event.dataTransfer.files); }}
                     className="rounded-2xl border-2 border-dashed border-zinc-300 p-10 text-center dark:border-white/15"
                 >
                     <Upload size={26} className="mx-auto text-zinc-400" />
                     <p className="mt-3 text-sm font-medium text-zinc-700 dark:text-zinc-200">{t('importAudioTitle')}</p>
                     <p className="mt-1 text-xs text-zinc-500">{t('importAudioHint')}</p>
                     <button
                         type="button"
                         onClick={() => importInput.current?.click()}
                         disabled={importing}
                         className="mt-4 inline-flex items-center gap-2 rounded-lg bg-linear-to-r from-orange-500 to-pink-600 px-4 py-2 text-sm font-bold text-white disabled:opacity-50"
                     >
                         {importing ? <Loader2 size={15} className="animate-spin" /> : <Upload size={15} />}
                         {t('chooseFiles')}
                     </button>
                     <input
                         ref={importInput}
                         type="file"
                         accept={AUDIO_ACCEPT}
                         multiple
                         className="hidden"
                         onChange={(event) => { void importAudio(event.target.files); event.target.value = ''; }}
                     />
                     {importError && <p role="alert" className="mt-3 text-xs text-rose-600 dark:text-rose-300">{importError}</p>}
                 </div>
             )}
        </div>
        </>
    );
};
