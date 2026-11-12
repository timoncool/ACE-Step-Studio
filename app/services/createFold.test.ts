import { beforeEach, describe, expect, it } from 'vitest';
import { FOLDED_KEY, foldedAfter, keepFolded, keptFolded } from './createFold';

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

  it('read nothing folded from a missing or broken record', () => {
    expect(keptFolded()).toEqual([]);
    window.localStorage.setItem(FOLDED_KEY, '{not json');
    expect(keptFolded()).toEqual([]);
    window.localStorage.setItem(FOLDED_KEY, JSON.stringify(['song', 3, null]));
    expect(keptFolded()).toEqual(['song']);
  });
});
