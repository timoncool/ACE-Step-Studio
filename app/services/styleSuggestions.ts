/**
 * The quick genre tags under the style field, with a gloss in the window's
 * language. The genres stay in English, the language ACE-Step reads its
 * caption in; a window in another language shows each one's gloss beside it,
 * from `examples/genres.<language>.txt` (one `genre<TAB>gloss` a line). A
 * language without a file shows the genres alone.
 */

import type { Language } from '../i18n/translations';

const glossTexts = import.meta.glob('../examples/genres.*.txt', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;

const lines = (text: string) => text.split('\n').map(line => line.replace(/\r$/, '')).filter(line => line.trim().length > 0);

/** Each language's glosses by genre, from the files beside the list. */
export const GENRE_GLOSSES: Partial<Record<Language, Map<string, string>>> = Object.fromEntries(
  Object.entries(glossTexts).map(([path, text]) => {
    const language = path.match(/genres\.([a-z]+)\.txt$/)?.[1] ?? path;
    const glosses = new Map(lines(text).map(line => {
      const [genre, gloss = ''] = line.split('\t');
      return [genre.trim(), gloss.trim()] as [string, string];
    }));
    return [language, glosses];
  }),
);

/** A genre's gloss in `language`, when the language has one for it. */
export function genreGloss(genre: string, language: Language): string | undefined {
  return GENRE_GLOSSES[language]?.get(genre) || undefined;
}
