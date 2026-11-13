import React, { useCallback, useEffect, useRef, useState } from 'react';
import { Loader2, RotateCcw, Trash2 } from 'lucide-react';
import { useI18n } from '../context/I18nContext';
import { deleteTrashedSong, emptyTrash, listTrash, restoreTrashedSong, type TrashedSong } from '../services/nativeLibrary';
import { libraryChanged } from '../services/studioQueries';
import { ConfirmDialog } from './ConfirmDialog';

/** The library's bin: songs deleted in the last days, to bring back or delete for good. */
export const LibraryTrash: React.FC<{ onToast: (message: string, type?: 'success' | 'error' | 'info') => void }> = ({ onToast }) => {
  const { t, language } = useI18n();
  const [songs, setSongs] = useState<TrashedSong[] | null>(null);
  const [days, setDays] = useState(30);
  const [busy, setBusy] = useState<string | null>(null);
  const [asking, setAsking] = useState<{ kind: 'one'; song: TrashedSong } | { kind: 'all' } | null>(null);

  // the latest onToast, outside the read's keys: the page passes a new function on every render, and a read keyed on
  // it would ask for the bin again each time
  const toast = useRef(onToast);
  useEffect(() => {
    toast.current = onToast;
  }, [onToast]);
  const load = useCallback(async () => {
    try {
      const bin = await listTrash();
      setSongs(bin.songs);
      setDays(bin.days);
    } catch (error) {
      setSongs([]);
      toast.current(error instanceof Error ? error.message : String(error), 'error');
    }
  }, []);

  useEffect(() => { void load(); }, [load]);

  const act = async (key: string, work: () => Promise<void>, done: string) => {
    setBusy(key);
    try {
      await work();
      onToast(done, 'success');
      libraryChanged();
    } catch (error) {
      onToast(error instanceof Error ? error.message : String(error), 'error');
    } finally {
      setBusy(null);
      void load();
    }
  };

  const date = (at: Date) => at.toLocaleDateString(language, { day: 'numeric', month: 'long' });

  return (
    <div>
      <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
        <p className="max-w-xl text-sm text-zinc-500 dark:text-zinc-400">{t('trashIntro').replace('{days}', String(days))}</p>
        {songs && songs.length > 0 && (
          <button
            type="button"
            onClick={() => setAsking({ kind: 'all' })}
            disabled={busy !== null}
            className="inline-flex items-center gap-2 rounded-full border border-red-500/40 px-4 py-2 text-sm font-bold text-red-600 hover:bg-red-500/10 disabled:opacity-50 dark:text-red-400"
          >
            {busy === 'all' ? <Loader2 size={15} className="animate-spin" /> : <Trash2 size={15} />} {t('trashEmpty')}
          </button>
        )}
      </div>

      {songs === null && <Loader2 size={18} className="animate-spin text-zinc-400" />}
      {songs?.length === 0 && <div className="text-sm text-zinc-500 dark:text-zinc-400">{t('trashNothing')}</div>}

      <ul className="divide-y divide-zinc-200 dark:divide-white/5">
        {songs?.map(entry => (
          <li key={entry.song.id} className="flex flex-wrap items-center gap-3 py-3">
            <div className="h-12 w-12 shrink-0 overflow-hidden rounded-md bg-zinc-200 dark:bg-white/5">
              {entry.song.coverUrl && <img src={entry.song.coverUrl} alt="" className="h-full w-full object-cover opacity-70" />}
            </div>
            <div className="min-w-0 flex-1">
              <p className="truncate text-sm font-bold text-zinc-900 dark:text-white">{entry.song.title}</p>
              <p className="text-xs text-zinc-500">
                {t('trashDeletedOn').replace('{date}', date(entry.trashedAt))} · {t('trashGoneOn').replace('{date}', date(entry.goneAt))}
                {entry.with > 0 && ` · ${t('trashWith').replace('{count}', String(entry.with))}`}
              </p>
            </div>
            <button
              type="button"
              onClick={() => void act(entry.song.id, () => restoreTrashedSong(entry.song.id), t('trashRestored').replace('{title}', entry.song.title))}
              disabled={busy !== null}
              className="inline-flex items-center gap-1.5 rounded-full bg-zinc-900 px-3 py-1.5 text-xs font-bold text-white disabled:opacity-50 dark:bg-white dark:text-black"
            >
              {busy === entry.song.id ? <Loader2 size={13} className="animate-spin" /> : <RotateCcw size={13} />} {t('trashRestore')}
            </button>
            <button
              type="button"
              onClick={() => setAsking({ kind: 'one', song: entry })}
              disabled={busy !== null}
              className="inline-flex items-center gap-1.5 rounded-full px-3 py-1.5 text-xs font-bold text-red-600 hover:bg-red-500/10 disabled:opacity-50 dark:text-red-400"
            >
              <Trash2 size={13} /> {t('trashDeleteForGood')}
            </button>
          </li>
        ))}
      </ul>

      <ConfirmDialog
        isOpen={asking !== null}
        danger
        title={asking?.kind === 'all' ? t('trashEmpty') : t('trashDeleteForGood')}
        message={asking?.kind === 'one' ? t('trashDeleteForGoodConfirm').replace('{title}', asking.song.song.title) : t('trashEmptyConfirm')}
        confirmLabel={asking?.kind === 'all' ? t('trashEmpty') : t('trashDeleteForGood')}
        onCancel={() => setAsking(null)}
        onConfirm={() => {
          const chosen = asking;
          setAsking(null);
          if (chosen?.kind === 'one') void act(chosen.song.song.id, () => deleteTrashedSong(chosen.song.song.id), t('trashDeletedForGood'));
          if (chosen?.kind === 'all') void act('all', emptyTrash, t('trashEmptied'));
        }}
      />
    </div>
  );
};
