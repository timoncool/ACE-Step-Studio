import React, { useRef, useState } from 'react';
import { Song, Playlist } from '../types';
import { Heart, Plus, Play, Upload, Loader2 } from 'lucide-react';
import { AlbumCover } from './AlbumCover';
import { useI18n } from '../context/I18nContext';
import { SongList } from './SongList';
import { SONG_ORDERS, type SongOrder } from '../services/songOrder';

/** The liked songs are read from the latest like first. */
const LIKED_ORDERS: SongOrder[] = ['likedNewest', ...SONG_ORDERS];

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
  onImported?: () => void;
  likedSongIds: Set<string>;
  onToggleLike: (songId: string) => void;
  onAddToPlaylist: (songs: Song | Song[]) => void;
  onDeleteMany: (songs: Song[]) => void;
  onMidiMany: (songs: Song[]) => void;
  /** Opens a song's details over the page, or closes them when they are its. */
  onShowDetails: (song: Song) => void;
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
    onImported,
    likedSongIds,
    onToggleLike,
    onAddToPlaylist,
    onDeleteMany,
    onMidiMany,
    onShowDetails,
    detailsSongId,
}) => {
    const { t, songCount } = useI18n();
    const [activeTab, setActiveTab] = useState<'all' | 'playlists' | 'liked' | 'import'>('all');
    const [importing, setImporting] = useState(false);
    const [importError, setImportError] = useState<string | null>(null);
    const importInput = useRef<HTMLInputElement | null>(null);

    /// Imports an existing MP3, WAV, FLAC or OGG into the local library. The service
    /// stores the media and the row atomically, rolling both back on failure.
    const importAudio = async (files: FileList | null) => {
        if (!files || files.length === 0) return;
        setImporting(true);
        setImportError(null);
        try {
            for (const file of Array.from(files)) {
                const form = new FormData();
                form.append('audio', file, file.name);
                form.append('title', file.name.replace(/\.[^.]+$/, ''));
                const response = await fetch('/v1/library/import', { method: 'POST', body: form });
                if (!response.ok) {
                    const body = await response.json().catch(() => null);
                    throw new Error(body?.error || `${file.name}: import failed (${response.status})`);
                }
            }
            onImported?.();
        } catch (reason) {
            setImportError(reason instanceof Error ? reason.message : 'Import failed.');
        } finally {
            setImporting(false);
        }
    };

    // the list of a tab: the same rows, search, filters and selection as the create page's
    const songList = (songs: Song[], orderKey: string, orders: SongOrder[], defaultOrder: SongOrder) => (
        <SongList
            embedded
            songs={songs}
            librarySongs={allSongs}
            currentSong={currentSong}
            selectedSong={null}
            likedSongIds={likedSongIds}
            isPlaying={isPlaying}
            onPlay={onPlaySong}
            onSelect={song => onPlaySong(song, songs)}
            onToggleLike={onToggleLike}
            onAddToPlaylist={song => onAddToPlaylist(song)}
            onAddManyToPlaylist={chosen => onAddToPlaylist(chosen)}
            onShowDetails={onShowDetails}
            detailsSongId={detailsSongId}
            onDeleteMany={onDeleteMany}
            onMidiMany={onMidiMany}
            orderKey={orderKey}
            orders={orders}
            defaultOrder={defaultOrder}
        />
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
             <div className="mb-8 flex items-center gap-3">
             <div className="flex min-w-0 flex-1 items-center gap-4 overflow-x-auto border-b border-zinc-200 pb-1 dark:border-white/10">
                 <button 
                    onClick={() => setActiveTab('all')}
                    className={`pb-3 text-sm font-bold transition-colors relative ${activeTab === 'all' ? 'text-zinc-900 dark:text-white' : 'text-zinc-500 hover:text-zinc-900 dark:hover:text-white'}`}
                 >
                    {t('allSongs')}
                    {activeTab === 'all' && <div className="absolute bottom-0 left-0 right-0 h-0.5 bg-green-500 rounded-full"></div>}
                 </button>
                 <button 
                    onClick={() => setActiveTab('liked')}
                    className={`pb-3 text-sm font-bold transition-colors relative ${activeTab === 'liked' ? 'text-zinc-900 dark:text-white' : 'text-zinc-500 hover:text-zinc-900 dark:hover:text-white'}`}
                 >
                    {t('likedSongs')}
                    {activeTab === 'liked' && <div className="absolute bottom-0 left-0 right-0 h-0.5 bg-green-500 rounded-full"></div>}
                 </button>
                 <button 
                    onClick={() => setActiveTab('playlists')}
                    className={`pb-3 text-sm font-bold transition-colors relative ${activeTab === 'playlists' ? 'text-zinc-900 dark:text-white' : 'text-zinc-500 hover:text-zinc-900 dark:hover:text-white'}`}
                 >
                    {t('playlists')}
                    {activeTab === 'playlists' && <div className="absolute bottom-0 left-0 right-0 h-0.5 bg-green-500 rounded-full"></div>}
                 </button>
                 <button 
                    onClick={() => setActiveTab('import')}
                    className={`pb-3 text-sm font-bold transition-colors relative ${activeTab === 'import' ? 'text-zinc-900 dark:text-white' : 'text-zinc-500 hover:text-zinc-900 dark:hover:text-white'}`}
                 >
                    {t('importAudioTab')}
                    {activeTab === 'import' && <div className="absolute bottom-0 left-0 right-0 h-0.5 bg-green-500 rounded-full"></div>}
                 </button>
             </div>
             </div>

             {/* Content */}
             {activeTab === 'all' && (allSongs.length === 0
                 ? <div className="text-sm text-zinc-500 dark:text-zinc-400">{t('noSongsYet')}</div>
                 : songList(allSongs, 'library', SONG_ORDERS, 'newest'))}
             {activeTab === 'liked' && (
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
             {activeTab === 'playlists' && (
                 <div className="grid grid-cols-1 gap-4 min-[440px]:grid-cols-2 sm:gap-6 md:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5">
                     {playlists.map((playlist) => (
                         <div key={playlist.id} className="bg-white dark:bg-zinc-900/40 p-4 rounded-lg border border-zinc-200 dark:border-white/5 hover:border-zinc-300 dark:hover:border-white/10 hover:shadow-lg dark:hover:bg-zinc-900 transition-all group cursor-pointer" onClick={() => onSelectPlaylist(playlist)}>
                             <div className="relative aspect-square mb-4 rounded-md overflow-hidden bg-zinc-100 dark:bg-zinc-800 flex items-center justify-center">
                                 {playlist.coverUrl ? (
                                     <img src={playlist.coverUrl} className="w-full h-full object-cover" alt={playlist.name} onError={(e) => { e.currentTarget.style.display = 'none'; }} />
                                 ) : (
                                     <AlbumCover seed={playlist.id || playlist.name} size="full" className="w-full h-full" />
                                 )}
                             </div>
                             <h3 className="font-bold text-zinc-900 dark:text-white truncate">{playlist.name}</h3>
                             <p className="text-sm text-zinc-500 dark:text-zinc-400 line-clamp-2">{playlist.description || t('byYou')}</p>
                         </div>
                     ))}
                 </div>
             )}
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
                         accept="audio/mpeg,audio/wav,audio/flac,audio/ogg,.mp3,.wav,.flac,.ogg"
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
