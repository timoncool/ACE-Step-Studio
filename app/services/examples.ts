/**
 * The official ACE-Step 1.5 examples (ace-step/ACE-Step-1.5, MIT): one-line
 * song ideas for the simple mode and complete text2music requests, and the
 * genre list of ACE-Step Studio's quick tags.
 */
import simple from '../examples/simple.json';
import text2music from '../examples/text2music.json';
import genreText from '../examples/genres.txt?raw';

export interface SongIdea {
  description: string;
  instrumental?: boolean;
  vocal_language?: string;
}

export interface SongExample {
  caption: string;
  lyrics?: string;
  bpm?: number;
  keyscale?: string;
  timesignature?: string;
  duration?: number;
  vocal_language?: string;
}

const pick = <T,>(items: T[]): T => items[Math.floor(Math.random() * items.length)];

export const randomIdea = (): SongIdea => pick(simple as SongIdea[]);

export const randomExample = (): SongExample => pick(text2music as SongExample[]);

export const GENRES = genreText.split('\n').map(line => line.trim()).filter(Boolean);

/** A few genres to offer as quick tags, different each time. */
export const someGenres = (count = 6): string[] => [...GENRES].sort(() => Math.random() - 0.5).slice(0, count);

/** The style line a track shows under its title: the caption's first sentence. */
export function captionSummary(caption: string): string {
  const text = caption.replace(/\s+/g, ' ').trim();
  const sentence = text.split(/(?<=[.!?])\s/)[0] ?? text;
  return sentence.length > 140 ? `${sentence.slice(0, 137)}…` : sentence;
}
