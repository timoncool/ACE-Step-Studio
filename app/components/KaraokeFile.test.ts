import { describe, expect, it } from 'vitest';
import { karaokeFromRows, karaokeRows } from './KaraokeFile';

const LRC = '[00:12.41]<00:12.41>Neon <00:12.90>trembles\n[00:20.50]<00:20.50>on <00:21.00>glass';

describe('editing a song\'s karaoke (YuE2 Studio #218)', () => {
  it('shows each line as its start and its text, the word times aside', () => {
    expect(karaokeRows(LRC).map(row => [row.time, row.text])).toEqual([['00:12.41', 'Neon trembles'], ['00:20.50', 'on glass']]);
  });

  it('keeps a line left alone with its word times, and writes an edited one plain', () => {
    const rows = karaokeRows(LRC);
    rows[1] = { ...rows[1], time: '00:20.80', text: 'on the glass' };
    expect(karaokeFromRows(rows)).toBe('[00:12.41]<00:12.41>Neon <00:12.90>trembles\n[00:20.80]on the glass');
  });
});
