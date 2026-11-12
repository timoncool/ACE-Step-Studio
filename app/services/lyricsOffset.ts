/** How far the karaoke may be moved against the song, in seconds either way. */
export const LYRICS_OFFSET_LIMIT = 30;

/** An offset kept to the limit and to a hundredth of a second. */
export const clampLyricsOffset = (seconds: number): number =>
  Number.isFinite(seconds) ? Math.round(Math.min(LYRICS_OFFSET_LIMIT, Math.max(-LYRICS_OFFSET_LIMIT, seconds)) * 100) / 100 : 0;
