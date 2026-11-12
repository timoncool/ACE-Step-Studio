/**
 * What the studio can and cannot time, and how it says so.
 *
 * The server names a cause rather than a sentence - "karaoke.instrumental" -
 * so the interface can put it in the language the user is reading.
 */

import { audioFormatFor, base64For } from './nativeOpenRouter';

/** Whether anything is actually sung: an instrumental carries only its markers. */
export const hasSungLines = (lyrics?: string | null): boolean =>
    (lyrics ?? '')
        .split('\n')
        .map(line => line.trim())
        .some(line => line.length > 0 && !(line.startsWith('[') && line.endsWith(']')));

const REASONS: Record<string, string> = {
    'karaoke.off': 'karaokeOff',
    'karaoke.no-recogniser': 'karaokeNoRecogniser',
    'karaoke.instrumental': 'karaokeInstrumental',
    'karaoke.no-match': 'karaokeNoMatch',
    'karaoke.no-words': 'karaokeNoWords',
    'karaoke.no-timings': 'karaokeNoTimings',
    'karaoke.downloading': 'karaokeDownloading',
    'karaoke.model-missing': 'karaokeModelMissing',
    'karaoke.no-lines': 'karaokeNoLines',
};

/** The lyrics a library song or an audio file sings, heard by the recogniser chosen for karaoke. */
export async function recogniseLyrics(source: { songId: string } | { file: File }, language?: string): Promise<string> {
    const body = 'songId' in source
        ? { song_id: source.songId, language: language || undefined }
        : { audio_base64: await base64For(source.file), audio_format: audioFormatFor(source.file), audio_name: source.file.name, language: language || undefined };
    const response = await fetch('/v1/lyrics/recognise', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
    });
    const payload = await response.json().catch(() => null) as { text?: unknown; error?: unknown } | null;
    if (!response.ok) throw new Error(typeof payload?.error === 'string' ? payload.error : `the lyrics could not be recognised (${response.status})`);
    if (typeof payload?.text !== 'string') throw new Error('the recogniser returned no lyrics');
    return payload.text;
}

/** The reason in words, or whatever the server said if it is not one of ours. */
export const karaokeReason = (t: (key: string) => string, reason?: string | null): string | undefined => {
    if (!reason) return undefined;
    // A reason can carry a number - "karaoke.downloading 37%" - so the strip
    // shows progress instead of a word that never moves.
    const [code, ...rest] = reason.trim().split(' ');
    const key = REASONS[code];
    if (!key) return reason;
    return rest.length > 0 ? `${t(key)} ${rest.join(' ')}` : t(key);
};
