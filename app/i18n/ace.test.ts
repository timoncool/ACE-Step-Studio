import { describe, expect, it } from 'vitest';
import { aceStrings } from './ace';

describe('ACE-Step strings', () => {
  it('every language carries every key, none empty', () => {
    const keys = Object.keys(aceStrings.en).sort();
    for (const [language, strings] of Object.entries(aceStrings)) {
      expect(Object.keys(strings).sort(), language).toEqual(keys);
      for (const key of keys) {
        expect((strings as Record<string, string>)[key]?.trim().length, `${language}.${key}`).toBeGreaterThan(0);
      }
    }
  });
});

describe('merged interface strings', () => {
  it('nothing the interface shows names the models this studio grew out of', async () => {
    const { translations } = await import('./translations');
    const leaks: string[] = [];
    for (const [language, strings] of Object.entries(translations)) {
      for (const [key, value] of Object.entries(strings)) {
        if (typeof value === 'string' && /music ?3|minimax|yue ?2/i.test(value)) leaks.push(`${language}.${key}: ${value.slice(0, 80)}`);
      }
    }
    expect(leaks).toEqual([]);
  });
});
