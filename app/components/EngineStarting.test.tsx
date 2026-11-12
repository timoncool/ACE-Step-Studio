import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { EngineStarting } from './EngineStarting';

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ t: (key: string) => key }) }));
vi.mock('../services/studioQueries', () => ({ SELF_READ: { 'x-hub-skip': '1' } }));

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
let root: Root;
let host: HTMLDivElement;

afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('engine start poll', () => {
  it('a page drawn again with a new onReady asks no sooner than the pause', async () => {
    vi.useFakeTimers();
    let asked = 0;
    vi.stubGlobal('fetch', vi.fn(async (url: string) => {
      if (url === '/health') asked += 1;
      return { ok: true, status: 200, json: async () => ({ music_engine: { reachable: false }, engine_bundle_present: true }) } as Response;
    }));
    host = document.createElement('div');
    document.body.append(host);
    root = createRoot(host);
    const draw = () => root.render(<EngineStarting onReady={() => undefined} />);
    await act(async () => draw());
    await act(async () => { await vi.advanceTimersByTimeAsync(100); });
    const first = asked;
    expect(first).toBe(1);
    for (let again = 0; again < 5; again++) await act(async () => draw());
    await act(async () => { await vi.advanceTimersByTimeAsync(500); });
    expect(asked).toBe(first);
    await act(async () => { await vi.advanceTimersByTimeAsync(600); });
    expect(asked).toBe(first + 1);
  });
});
