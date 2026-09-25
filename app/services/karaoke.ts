/**
 * What the studio can and cannot time, and how it says so.
 *
 * The server names a cause rather than a sentence - "karaoke.instrumental" -
 * so the interface can put it in the language the user is reading.
 */

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
    'karaoke.downloading': 'karaokeDownloading',
    'karaoke.model-missing': 'karaokeModelMissing',
};

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
