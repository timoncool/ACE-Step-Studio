import { describe, expect, it } from 'vitest';
import { translations, type Language } from './translations';

describe('the bin length in the texts', () => {
  it('comes from the service in every language, not written in', () => {
    for (const language of Object.keys(translations) as Language[]) {
      for (const key of ['deleteSongConfirm', 'deleteSongsConfirm', 'trashIntro'] as const) {
        const text = translations[language][key] ?? '';
        expect(text, `${language} ${key}`).toContain('{days}');
        expect(text, `${language} ${key}`).not.toMatch(/\b30\b/);
      }
    }
  });

  it('counts the songs that went with one, not stems alone', () => {
    for (const language of Object.keys(translations) as Language[]) {
      expect(translations[language].trashWith ?? '', language).toMatch(/[:：] ?\{count\}$/);
    }
  });
});
