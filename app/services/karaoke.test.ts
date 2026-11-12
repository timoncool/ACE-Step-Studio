import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { karaokeReason, recogniseLyrics } from './karaoke';

describe('karaokeReason', () => {
  it('puts the refusal of a song in the bin in the window language, with its title', () => {
    // the sentence the service writes (library::in_the_bin), so the two cannot drift apart
    const source = readFileSync(resolve(__dirname, '../../crates/music-server/src/library.rs'), 'utf8');
    const sentence = /format!\("\\"\{\}\\"( is in the library's bin; restore it in Library - Bin[^"]*)"/.exec(source)?.[1];
    expect(sentence).toBeTruthy();
    const t = (key: string) => (key === 'songInTheBin' ? 'IN BIN: {title}' : key);
    expect(karaokeReason(t, `"Song, "quoted""${sentence}`)).toBe('IN BIN: Song, "quoted"');
  });
});

afterEach(() => {
  vi.unstubAllGlobals();
});

function answering(status: number, body: unknown) {
  const fetch = vi.fn(async () => new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } }));
  vi.stubGlobal('fetch', fetch);
  return fetch;
}

const sent = (fetch: ReturnType<typeof answering>) => JSON.parse((fetch.mock.calls[0] as unknown as [string, RequestInit])[1].body as string);

describe('recogniseLyrics', () => {
  it('names a library song by its id', async () => {
    const fetch = answering(200, { text: 'hello there.\nagain now' });
    expect(await recogniseLyrics({ songId: 'song-1' }, 'en')).toBe('hello there.\nagain now');
    expect((fetch.mock.calls[0] as unknown as [string])[0]).toBe('/v1/lyrics/recognise');
    expect(sent(fetch)).toEqual({ song_id: 'song-1', language: 'en' });
  });

  it('sends a file with its format and name', async () => {
    const fetch = answering(200, { text: 'la la' });
    const file = new File([new Uint8Array([1, 2, 3])], 'demo take.wav', { type: 'audio/wav' });
    expect(await recogniseLyrics({ file })).toBe('la la');
    const body = sent(fetch);
    expect(body.audio_base64).toBe('AQID');
    expect(body.audio_format).toBe('wav');
    expect(body.audio_name).toBe('demo take.wav');
    expect(body.song_id).toBeUndefined();
  });

  it('passes the server reason on', async () => {
    answering(409, { error: 'karaoke.model-missing' });
    await expect(recogniseLyrics({ songId: 'song-1' })).rejects.toThrow('karaoke.model-missing');
  });
});
