/**
 * The vocal language a lyric's script names. ACE-Step forces the language it
 * is given into its plan, and left unset it guesses, so Cyrillic words sung as
 * English, or as a language nobody asked for, come from a language the lyric
 * never had. Latin script names no language, so it gives none.
 */
export function scriptLanguage(lyrics: string): string | null {
  const words = lyrics.replace(/\[[^\]]*\]/g, ' ');
  const count = (pattern: RegExp) => (words.match(pattern) ?? []).length;
  const kana = count(/[぀-ヿ]/g);
  const han = count(/[一-鿿]/g);
  const tallies: [string, number][] = [
    ['cyrillic', count(/[Ѐ-ӿ]/g)],
    ['ko', count(/[가-힯ᄀ-ᇿ]/g)],
    // kana among Chinese characters is Japanese
    kana > 0 ? ['ja', kana + han] : ['zh', han],
    ['ar', count(/[؀-ۿ]/g)],
    ['hi', count(/[ऀ-ॿ]/g)],
    ['th', count(/[฀-๿]/g)],
    ['latin', count(/[A-Za-zÀ-ɏ]/g)],
  ];
  const [script, letters] = tallies.reduce((best, entry) => (entry[1] > best[1] ? entry : best));
  if (letters < 8 || script === 'latin') return null;
  if (script === 'cyrillic') return count(/[іїєґІЇЄҐ]/g) > 0 ? 'uk' : 'ru';
  return script;
}
