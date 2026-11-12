import { describe, expect, it } from 'vitest';
import { aceStrings } from './ace';

describe('Add track hint', () => {
  it('says the result is the new instrument on its own, not the source with it, in every language', () => {
    expect(aceStrings.en.aceTaskHint_lego).toMatch(/on its own track, not mixed with the source/);
    expect(aceStrings.ru.aceTaskHint_lego).toMatch(/отдельной дорожкой, без исходника/);
    for (const language of ['zh', 'ja', 'ko'] as const) {
      expect(aceStrings[language].aceTaskHint_lego).not.toBe(aceStrings.en.aceTaskHint_lego);
      expect(aceStrings[language].aceTaskHint_lego.length).toBeGreaterThan(10);
    }
  });
});
