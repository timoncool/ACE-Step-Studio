import { describe, expect, it } from 'vitest';
import { lrcToSrt } from './lrc-parser';

describe('lrcToSrt', () => {
  it('ends each line when its singing ends, leaving gaps for instrumental stretches, without section tags (#189)', () => {
    const lrc = '[00:01.50][Verse 1]\n[00:02.00]<00:02.00>First <00:02.40>line\n[00:05.25]Second line\n[01:02.10]Last line';
    expect(lrcToSrt(lrc)).toBe(
      // First line ends after its last word (2.40 + 0.40), not at the next line (5.25)
      '1\n00:00:02,000 --> 00:00:02,800\nFirst line\n\n'
      // Second line (plain) stays a few seconds, then a ~53 s instrumental gap before the last line
      + '2\n00:00:05,250 --> 00:00:09,250\nSecond line\n\n'
      + '3\n00:01:02,100 --> 00:01:05,100\nLast line\n',
    );
  });

  it('gives nothing for timings without a sung line', () => {
    expect(lrcToSrt('[00:01.00][Chorus]')).toBe('');
  });
});
