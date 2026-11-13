import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ t: (key: string) => key }) }));
vi.mock('../services/studioQueries', () => ({ assistantStatusChanged: vi.fn() }));

const { AssistantRules } = await import('./AssistantRules');

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
let root: Root;
let host: HTMLDivElement;

afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('AssistantRules', () => {
  it('opening the settings reads the rules and writes nothing back', async () => {
    vi.useFakeTimers();
    const puts: string[] = [];
    vi.stubGlobal('fetch', vi.fn(async (_url: string, init?: RequestInit) => {
      if (init?.method === 'PUT') puts.push(String(init.body));
      return { ok: true, status: 200, json: async () => ({ own_rules: { song: 'short lines', style: null, lyrics: null }, avoid_words: 'neon' }) } as Response;
    }));
    host = document.createElement('div');
    document.body.append(host);
    root = createRoot(host);
    await act(async () => root.render(<AssistantRules />));
    await act(async () => { await vi.advanceTimersByTimeAsync(2000); });
    expect(puts).toEqual([]);
  });
});
