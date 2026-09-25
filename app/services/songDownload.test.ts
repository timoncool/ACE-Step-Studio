import { describe, expect, it } from 'vitest';
import type { Song } from '../types';
import { songFileName } from './songDownload';

const song = (extra: Partial<Song> = {}) => ({ id: 's', title: 'Северный ветер', ...extra }) as Song;

describe('songFileName', () => {
  it('names the original by its title and stored format', () => {
    expect(songFileName(song(), 'audio/wav')).toBe('Северный ветер.wav');
    expect(songFileName(song({ activeVersion: 'original', audioVersions: [{ id: 'v', label: 'x', createdAt: '' }] }), 'audio/mpeg')).toBe('Северный ветер.mp3');
  });

  it('carries the processing of the version that plays', () => {
    const processed = song({ activeVersion: 'v', audioVersions: [{ id: 'v', label: 'Шумоподавление 0.40 + Мастеринг · Big Pineapple', createdAt: '' }] });
    expect(songFileName(processed, 'audio/wav')).toBe('Северный ветер (Шумоподавление 0.40 + Мастеринг · Big Pineapple).wav');
  });

  it('keeps the name legal on Windows', () => {
    expect(songFileName(song({ title: 'A/B: "test"?' }), 'audio/flac')).toBe('A_B_ _test__.flac');
  });
});
