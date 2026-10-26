import { describe, expect, it } from 'vitest';
import { GENRES } from './examples';
import { GENRE_GLOSSES, genreGloss } from './styleSuggestions';

describe('genre glosses', () => {
  it('gloss every genre of the quick tags in each language that has a file, and nothing else', () => {
    expect(GENRE_GLOSSES.zh).toBeDefined();
    for (const [language, glosses] of Object.entries(GENRE_GLOSSES)) {
      expect([...glosses!.keys()].sort(), language).toEqual([...GENRES].sort());
      for (const [genre, gloss] of glosses!) expect(gloss, `${language}: ${genre}`).not.toBe('');
    }
  });

  it('show the gloss beside the English genre the model reads', () => {
    expect(genreGloss('dubstep', 'zh')).toBe('回响贝斯');
    expect(genreGloss('dubstep', 'en')).toBeUndefined();
  });
});
