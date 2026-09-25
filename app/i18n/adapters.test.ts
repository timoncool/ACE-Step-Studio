import { describe, expect, it } from 'vitest';
import { adapterStrings } from './adapters';
import { processingStrings } from './processing';
import { trainingStrings } from './training';

for (const [name, catalogue] of Object.entries({ LoRA: adapterStrings, processing: processingStrings, training: trainingStrings })) {
  describe(`${name} strings`, () => {
    it('every language carries every key, none empty', () => {
      const keys = Object.keys(catalogue.en).sort();
      for (const [language, strings] of Object.entries(catalogue)) {
        expect(Object.keys(strings).sort(), language).toEqual(keys);
        for (const key of keys) {
          expect((strings as Record<string, string>)[key]?.trim().length, `${language}.${key}`).toBeGreaterThan(0);
        }
      }
    });

    it('names no model, so any engine can use them', () => {
      for (const [language, strings] of Object.entries(catalogue)) {
        for (const [key, value] of Object.entries(strings as Record<string, string>)) {
          expect(/yue|music ?3|minimax|ace-?step/i.test(value), `${language}.${key}: ${value}`).toBe(false);
        }
      }
    });
  });
}
