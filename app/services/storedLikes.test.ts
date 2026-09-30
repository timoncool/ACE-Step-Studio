import { afterEach, describe, expect, it, vi } from 'vitest';
import { moveStoredLikes } from './nativeLibrary';

describe('likes kept in the window move into the library', () => {
  afterEach(() => vi.unstubAllGlobals());

  it('keeps for later only what the service could not take', async () => {
    const answers: Record<string, number | 'offline'> = { taken: 200, gone: 404, broken: 500, away: 'offline' };
    const asked: string[] = [];
    vi.stubGlobal('fetch', vi.fn(async (url: string, init: RequestInit) => {
      const id = decodeURIComponent(url.split('/')[4]);
      asked.push(`${init.method} ${id} ${init.body}`);
      const answer = answers[id];
      if (answer === 'offline') throw new Error('the service is not answering');
      return new Response('{}', { status: answer });
    }));
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const left = await moveStoredLikes(['taken', 'gone', 'broken', 'away']);
    expect(left).toEqual(['broken', 'away']);
    expect(asked[0]).toBe('PUT taken {"liked":true}');
  });
});
