import React, { useEffect, useMemo, useState } from 'react';
import { apiUrl } from '../services/apiBase';
import { DEFAULT_COVER_PATTERN, isCoverPattern, patternArt } from '../services/coverArt';
import { coverSeed } from '../services/songStems';
import { useCoverLook } from '../services/studioQueries';
import type { Song } from '../types';

interface AlbumCoverProps {
  seed: string;
  size?: 'xs' | 'sm' | 'md' | 'lg' | 'xl' | 'full';
  className?: string;
  children?: React.ReactNode;
  /**
   * A stored cover image for this track. When absent — or when the file cannot
   * be loaded — the track's placeholder shows, so a track always has a cover and
   * never a broken image.
   */
  coverUrl?: string;
  /** The look's photograph for a track whose placeholder is not written into it. */
  placeholderUrl?: string;
}

const SIZES: Record<NonNullable<AlbumCoverProps['size']>, string> = {
  xs: 'w-8 h-8',
  sm: 'w-10 h-10',
  md: 'w-12 h-12',
  lg: 'w-14 h-14',
  xl: 'w-48 h-48',
  full: 'w-full h-full',
};

/** A cover: the track's pattern underneath, a photograph or its own picture over it. */
export const AlbumCover: React.FC<AlbumCoverProps> = ({ seed, size = 'md', className = '', children, coverUrl, placeholderUrl }) => {
  const look = useCoverLook().data;
  const pattern = isCoverPattern(look?.pattern) ? look.pattern : DEFAULT_COVER_PATTERN;
  const art = useMemo(() => patternArt(seed, pattern), [seed, pattern]);
  // a written placeholder arrives as the track's cover; until then its pattern shows
  const source = coverUrl || (look?.photo && !look.keep ? placeholderUrl ?? '' : '');
  const [imageFailed, setImageFailed] = useState(false);
  useEffect(() => { setImageFailed(false); }, [source]);

  return (
    <div
      className={`${SIZES[size]} rounded-md shadow-lg shrink-0 overflow-hidden relative bg-zinc-200 bg-cover bg-center dark:bg-zinc-800 ${className}`}
      style={{ backgroundImage: `url("${art}")` }}
    >
      {source && !imageFailed && (
        <img
          src={source}
          alt=""
          loading="lazy"
          onError={() => setImageFailed(true)}
          className="absolute inset-0 h-full w-full object-cover"
        />
      )}
      {children}
    </div>
  );
};

/** A song's cover wherever a song is shown: its own picture, else its placeholder,
 *  drawn from the song a stem was separated from so a stem wears its song's. */
export const SongCover: React.FC<Omit<AlbumCoverProps, 'seed' | 'coverUrl' | 'placeholderUrl'> & { song: Pick<Song, 'id' | 'title' | 'derived' | 'coverUrl'> }> = ({ song, ...cover }) => (
  <AlbumCover
    seed={coverSeed(song)}
    coverUrl={song.coverUrl}
    placeholderUrl={apiUrl(`/v1/library/songs/${encodeURIComponent(song.id)}/cover/placeholder`)}
    {...cover}
  />
);

export default AlbumCover;
