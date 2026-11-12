import { describe, expect, it } from 'vitest';
import { clampLyricsOffset, LYRICS_OFFSET_LIMIT } from './lyricsOffset';

describe('the karaoke offset (YuE2 Studio #231)', () => {
  it('reaches 30 seconds either way in hundredths', () => {
    expect(LYRICS_OFFSET_LIMIT).toBe(30);
    expect(clampLyricsOffset(-12.34)).toBe(-12.34);
    expect(clampLyricsOffset(0.016)).toBe(0.02);
    expect(clampLyricsOffset(45)).toBe(30);
    expect(clampLyricsOffset(-30.5)).toBe(-30);
    expect(clampLyricsOffset(Number.NaN)).toBe(0);
  });
});
