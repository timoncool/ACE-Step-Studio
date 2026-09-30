import React, { useCallback, useEffect, useRef, useState } from 'react';
import { AlertTriangle, ExternalLink, Image as ImageIcon, Loader2, Maximize2, X } from 'lucide-react';
import { Song } from '../types';
import { AlbumCover } from './AlbumCover';
import { CoverPicker, type PickedMedia } from './CoverPicker';
import { coverSeed } from '../services/songStems';
import { ImageLightbox } from './ImageLightbox';
import { apiUrl } from '../services/apiBase';
import { openExternal } from '../services/externalLinks';
import { useI18n } from '../context/I18nContext';

/**
 * A picture for a track: its cover, stored by the studio server next to the
 * audio and written into the file, or the background or the centre of its
 * video, handed back to the video editor. One window for all three, so each
 * offers the same choices - see CoverPicker.
 *
 * Nothing is paid for until the user asks: a drawing through OpenRouter waits
 * for its button, and that button waits for a key and a catalog model.
 */

type Purpose = 'cover' | 'background' | 'center';

interface CoverRegenModalProps {
  song: Song;
  onClose: () => void;
  /** What the picture is for; a cover unless the video editor asks. */
  purpose?: Purpose;
  onCoverSaved?: (songId: string, coverUrl: string) => void;
  /** The video editor's picture, handed back instead of stored. */
  onPicked?: (choice: PickedMedia) => void;
  startOn?: 'photo' | 'video';
}

const TITLES = { cover: 'coverArt', background: 'selectBackground', center: 'selectCenterImage' } as const;

export const CoverRegenModal: React.FC<CoverRegenModalProps> = ({ song, onClose, purpose = 'cover', onCoverSaved, onPicked, startOn }) => {
  const { t } = useI18n();
  const [choice, setChoice] = useState<PickedMedia | null>(null);
  const [busy, setBusy] = useState(false);
  // The picture, as big as the screen allows.
  const [zoomed, setZoomed] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const wide = purpose === 'background';

  // A clip read from disk plays from a blob address; one not handed on is given back.
  const handedOn = useRef<string | null>(null);
  useEffect(() => {
    const blob = choice?.kind === 'video' && choice.url.startsWith('blob:') ? choice.url : null;
    return () => {
      if (blob && handedOn.current !== blob) URL.revokeObjectURL(blob);
    };
  }, [choice]);

  const save = useCallback(async () => {
    if (!choice || busy) return;
    if (purpose !== 'cover') {
      handedOn.current = choice.url;
      onPicked?.(choice);
      onClose();
      return;
    }
    const cover = `/v1/library/songs/${encodeURIComponent(song.id)}/cover`;
    const request: [string, string, Record<string, string>] | null =
      choice.kind === 'image' ? [cover, 'PUT', { image_base64: choice.base64, media_type: choice.mediaType }]
        : choice.kind === 'photo' ? [`${cover}/photo`, 'POST', { image: choice.photo.image, page: choice.photo.page }]
          : choice.kind === 'pattern' ? [`${cover}/pattern`, 'POST', { pattern: choice.pattern, seed: choice.seed }]
            : null;
    if (!request) return;
    setBusy(true);
    setError(null);
    try {
      const [path, method, body] = request;
      const response = await fetch(path, { method, headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
      const answer = await response.json().catch(() => null);
      if (!response.ok) throw new Error(answer?.error || `Storing the cover failed (${response.status})`);
      onCoverSaved?.(song.id, apiUrl(cover));
      onClose();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'Storing the cover failed.');
    } finally {
      setBusy(false);
    }
  }, [busy, choice, onClose, onCoverSaved, onPicked, purpose, song.id]);

  // What the preview shows: the choice, or what the track has now.
  const still = choice && choice.kind !== 'video' ? choice.url : purpose === 'cover' ? song.coverUrl : '';
  const source = choice?.kind === 'photo' ? choice.photo.page
    : choice?.kind === 'video' ? choice.clip?.page
      : !choice && purpose === 'cover' ? song.coverSource : undefined;

  return (
    <div className="fixed inset-0 z-70 flex items-center justify-center bg-black/60 p-4" onClick={onClose}>
      <div className="flex max-h-[92vh] w-full max-w-2xl flex-col overflow-hidden rounded-2xl bg-white shadow-2xl dark:bg-zinc-900" onClick={event => event.stopPropagation()}>
        <div className="flex items-center justify-between border-b border-zinc-200 px-5 py-4 dark:border-white/10">
          <h3 className="flex items-center gap-2 text-base font-bold text-zinc-900 dark:text-white">
            <ImageIcon size={18} className="text-pink-500" /> {t(TITLES[purpose])}
          </h3>
          <button type="button" onClick={onClose} className="text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-200">
            <X size={18} />
          </button>
        </div>

        <div className="min-h-0 flex-1 space-y-4 overflow-y-auto p-5">
          {/* The result is the point of this window, so it gets the middle of
              it: a large picture, and a click to see it at full size. */}
          <div className="flex flex-col items-center">
            <button
              type="button"
              onClick={() => { if (still) setZoomed(true); }}
              className={`group relative w-full overflow-hidden rounded-2xl border border-zinc-200 bg-zinc-100 dark:border-white/10 dark:bg-zinc-800 ${wide ? 'aspect-video max-w-[520px]' : 'aspect-square max-w-[360px]'}`}
              title={t('coverOpenLarge')}
            >
              {choice?.kind === 'video' ? (
                <video src={choice.url} poster={choice.clip?.poster || undefined} muted loop autoPlay playsInline className="h-full w-full object-cover" />
              ) : choice ? (
                <img src={choice.url} alt={song.title} className="h-full w-full object-cover" />
              ) : purpose === 'background' ? (
                <span className="flex h-full w-full items-center justify-center px-6 text-center text-xs text-zinc-500">{t('pickerNothingChosen')}</span>
              ) : (
                <AlbumCover seed={coverSeed(song)} size="full" coverUrl={song.coverUrl} />
              )}
              {still && (
                <span className="pointer-events-none absolute bottom-2 right-2 inline-flex items-center gap-1 rounded-lg bg-black/60 px-2 py-1 text-[11px] text-white opacity-0 transition-opacity group-hover:opacity-100">
                  <Maximize2 size={12} /> {t('coverOpenLarge')}
                </span>
              )}
            </button>

            <p className="mt-3 w-full truncate text-center text-sm font-semibold text-zinc-900 dark:text-white">{song.title}</p>
            {source && (
              <button
                type="button"
                onClick={() => void openExternal(source)}
                className="mt-1 inline-flex items-center gap-1 text-[11px] text-zinc-500 hover:text-pink-600"
              >
                <ExternalLink size={11} /> {t('coverSourceLabel')}
              </button>
            )}
          </div>

          <CoverPicker song={song} choice={choice} onChoice={setChoice} forBackground={wide} startOn={startOn} />

          {error && (
            <p role="alert" className="flex items-center gap-2 rounded-lg bg-rose-500/10 px-3 py-2 text-xs text-rose-700 dark:text-rose-300">
              <AlertTriangle size={14} /> {error}
            </p>
          )}
        </div>

        <div className="flex justify-end gap-2 border-t border-zinc-200 px-5 py-4 dark:border-white/10">
          <button type="button" onClick={onClose} className="rounded-lg px-4 py-2 text-sm font-medium text-zinc-500 hover:text-zinc-800 dark:hover:text-zinc-200">
            {t('cancel')}
          </button>
          <button
            type="button"
            onClick={() => void save()}
            disabled={!choice || busy}
            className="inline-flex items-center gap-2 rounded-lg bg-zinc-900 px-4 py-2 text-sm font-bold text-white disabled:opacity-50 dark:bg-white dark:text-zinc-900"
          >
            {busy ? <Loader2 size={14} className="animate-spin" /> : null} {purpose === 'cover' ? t('saveCover') : t('pickerApply')}
          </button>
        </div>
      </div>
      {zoomed && still && (
        <ImageLightbox src={still} alt={song.title} onClose={() => setZoomed(false)} />
      )}
    </div>
  );
};
