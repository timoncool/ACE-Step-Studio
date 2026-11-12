import { beforeEach, describe, expect, it } from 'vitest';
import { FOLDED_KEY, foldedAfter, isFolded, keepFolded, keptFolded, marksAfter, SONG_PANEL_FOLDED_KEY } from './createFold';

describe('folded blocks of the create form', () => {
  beforeEach(() => window.localStorage.clear());

  it('fold and open a block without touching the others', () => {
    expect(foldedAfter([], 'lyrics', true)).toEqual(['lyrics']);
    expect(foldedAfter(['lyrics', 'quality'], 'lyrics', false)).toEqual(['quality']);
    expect(foldedAfter(['lyrics'], 'lyrics', true)).toEqual(['lyrics']);
  });

  it('are kept across a restart', () => {
    keepFolded(['song', 'lora']);
    expect(keptFolded()).toEqual(['song', 'lora']);
  });

  it('keep the song panel apart from the create form', () => {
    keepFolded(['lyrics']);
    keepFolded(['style'], SONG_PANEL_FOLDED_KEY);
    expect(keptFolded()).toEqual(['lyrics']);
    expect(keptFolded(SONG_PANEL_FOLDED_KEY)).toEqual(['style']);
  });

  it('keep a block folded by default folded until the person opens it', () => {
    expect(isFolded([], 'params', true)).toBe(true);
    const opened = marksAfter([], 'params', false, true);
    expect(opened).toEqual(['params:open']);
    expect(isFolded(opened, 'params', true)).toBe(false);
    expect(isFolded(marksAfter(opened, 'params', true, true), 'params', true)).toBe(true);
    expect(isFolded(marksAfter([], 'note', true), 'note')).toBe(true);
    expect(isFolded([], 'note')).toBe(false);
  });

  it('read nothing folded from a missing or broken record', () => {
    expect(keptFolded()).toEqual([]);
    window.localStorage.setItem(FOLDED_KEY, '{not json');
    expect(keptFolded()).toEqual([]);
    window.localStorage.setItem(FOLDED_KEY, JSON.stringify(['song', 3, null]));
    expect(keptFolded()).toEqual(['song']);
  });
});
