export interface LrcWord {
  time: number;
  text: string;
}

export interface LrcLine {
  time: number; // seconds
  text: string;
  isSection: boolean; // [Verse], [Chorus], etc.
  /** Present when the file carries per-word times (enhanced LRC, A2). */
  words?: LrcWord[];
}

/**
 * Parse LRC formatted text into timed lines.
 * Format: [mm:ss.xx] text
 */
export function parseLrc(lrcContent: string): LrcLine[] {
  if (!lrcContent) return [];

  const lines: LrcLine[] = [];
  const regex = /\[(\d{2}):(\d{2})\.(\d{2})\](.*)/;
  // Enhanced LRC (the A2 form) puts a time before every word:
  // [00:12.41]<00:12.41>Neon <00:12.90>trembles. Without those a karaoke
  // highlight can only sweep the line linearly, which drifts off the singing.
  const wordRegex = /<(\d{2}):(\d{2})\.(\d{2})>([^<]*)/g;

  for (const raw of lrcContent.split('\n')) {
    const match = raw.match(regex);
    if (!match) continue;

    const minutes = parseInt(match[1], 10);
    const seconds = parseInt(match[2], 10);
    const centiseconds = parseInt(match[3], 10);
    const body = match[4];

    const words: LrcWord[] = [];
    for (const word of body.matchAll(wordRegex)) {
      const wordText = word[4].trim();
      if (!wordText) continue;
      words.push({
        time: parseInt(word[1], 10) * 60 + parseInt(word[2], 10) + parseInt(word[3], 10) / 100,
        text: wordText,
      });
    }

    const text = words.length > 0 ? words.map((word) => word.text).join(' ') : body.trim();
    if (!text) continue;

    const time = minutes * 60 + seconds + centiseconds / 100;
    const isSection = /^\[.*\]$/.test(text);

    lines.push({ time, text, isSection, words: words.length > 0 ? words : undefined });
  }

  return lines.sort((a, b) => a.time - b.time);
}


/** How long the last line stays on screen: an SRT cue needs an end, and nothing follows it. */
const LAST_SRT_LINE_SECONDS = 3;
/** How long a sung line stays when its own end is unknown (plain LRC, no per-word times). */
const PLAIN_LINE_SECONDS = 4;
/** The tail a single-word line keeps after its word begins, so a subtitle is not cut on the syllable. */
const WORD_TAIL_SECONDS = 0.8;

function srtTime(seconds: number): string {
  const total = Math.max(0, Math.round(seconds * 1000));
  const pad = (value: number, width = 2) => String(value).padStart(width, '0');
  return `${pad(Math.floor(total / 3_600_000))}:${pad(Math.floor(total / 60_000) % 60)}:${pad(Math.floor(total / 1000) % 60)},${pad(total % 1000, 3)}`;
}

/** When a sung line's subtitle ends: after its last word (enhanced LRC), or a
 *  few seconds in (plain LRC). This is what leaves gaps between the lines where
 *  the song plays without words, instead of one line lingering until the next. */
function lineEndSeconds(line: LrcLine): number {
  if (line.words && line.words.length > 0) {
    const last = line.words[line.words.length - 1].time;
    const span = last - line.words[0].time;
    const tail = line.words.length > 1 ? span / (line.words.length - 1) : WORD_TAIL_SECONDS;
    return last + Math.min(Math.max(tail, 0.3), 1.5);
  }
  return line.time + PLAIN_LINE_SECONDS;
}

/**
 * The karaoke timings as SubRip subtitles, which video editors import where
 * they do not take LRC: each sung line runs until its singing ends (never past
 * the next line), so an instrumental stretch between lines stays empty on the
 * timeline instead of holding the last line. Section tags are not subtitles.
 */
export function lrcToSrt(lrcContent: string): string {
  const sung = parseLrc(lrcContent).filter((line) => !line.isSection);
  return sung
    .map((line, index) => {
      const next = sung.slice(index + 1).find((later) => later.time > line.time);
      const ceiling = next ? next.time : line.time + LAST_SRT_LINE_SECONDS;
      const end = Math.min(ceiling, Math.max(line.time + 0.3, Math.min(lineEndSeconds(line), ceiling)));
      return `${index + 1}\n${srtTime(line.time)} --> ${srtTime(end)}\n${line.text}\n`;
    })
    .join('\n');
}

/**
 * Find the current lyric line based on playback time.
 * Returns index of current line, or -1 if before first line.
 */
export function getCurrentLrcIndex(lines: LrcLine[], currentTime: number): number {
  if (lines.length === 0) return -1;

  for (let i = lines.length - 1; i >= 0; i--) {
    if (currentTime >= lines[i].time) return i;
  }

  return -1;
}

/**
 * How much of a line has been sung at this moment, from 0 to 1.
 *
 * With per-word times the highlight advances word by word, each word filling
 * in proportion to its own length - what a karaoke player is supposed to do.
 * With only a line time there is nothing to do but sweep linearly, and this
 * says so rather than pretending to know more.
 */
export function lineProgress(line: LrcLine, currentTime: number, nextLineTime?: number): number {
  const end = nextLineTime ?? line.time + 4;
  if (!line.words || line.words.length === 0) {
    return Math.max(0, Math.min(1, (currentTime - line.time) / Math.max(0.1, end - line.time)));
  }
  const lengths = line.words.map((word) => word.text.length);
  const total = lengths.reduce((sum, length) => sum + length, 0) || 1;
  let consumed = 0;
  for (let index = 0; index < line.words.length; index++) {
    const start = line.words[index].time;
    const finish = line.words[index + 1]?.time ?? end;
    if (currentTime < start) break;
    if (currentTime >= finish) {
      consumed += lengths[index];
      continue;
    }
    consumed += lengths[index] * ((currentTime - start) / Math.max(0.05, finish - start));
    break;
  }
  return Math.max(0, Math.min(1, consumed / total));
}

/** The word being sung right now, or -1 before the line starts. */
export function currentWordIndex(line: LrcLine, currentTime: number): number {
  if (!line.words || line.words.length === 0) return -1;
  let index = -1;
  for (let position = 0; position < line.words.length; position++) {
    if (currentTime >= line.words[position].time) index = position;
    else break;
  }
  return index;
}
