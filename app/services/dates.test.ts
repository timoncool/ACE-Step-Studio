import { describe, expect, it } from 'vitest';
import { relativeMoment } from './dates';

describe('when something happened, as a list shows it', () => {
  const now = new Date(2026, 8, 30, 12, 0, 0);
  const ago = (seconds: number) => new Date(now.getTime() - seconds * 1000);

  it('says how long ago for today and yesterday', () => {
    expect(relativeMoment(ago(10), 'en', now)).toBe('now');
    expect(relativeMoment(ago(5 * 60), 'en', now)).toBe('5 min. ago');
    expect(relativeMoment(ago(3 * 3600), 'en', now)).toBe('3 hr. ago');
    expect(relativeMoment(ago(30 * 3600), 'en', now)).toBe('yesterday');
  });

  it('gives a date for anything older, with the year when it is another one', () => {
    expect(relativeMoment(new Date(2026, 8, 20), 'en', now)).toBe('Sep 20');
    expect(relativeMoment(new Date(2025, 11, 31), 'en', now)).toBe('Dec 31, 2025');
  });
});
