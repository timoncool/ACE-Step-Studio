import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ t: (key: string) => key, language: 'en' }) }));
vi.mock('../services/studioQueries', () => ({ SELF_READ: { 'x-hub-skip': '1' } }));

const { UpdatePlanView } = await import('./UpdateScreen');

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
let root: Root;
let host: HTMLDivElement;

afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('UpdatePlanView', () => {
  it('asks for the plan again only after the last answer, not on a timer over a slow one', async () => {
    vi.useFakeTimers();
    const plan = { version: '3.9.0', updated: [], added: [], retired: [], download: 0, freed: 0, show: true };
    const running = { status: 200, ok: true, json: async () => ({ plan, job: { status: 'running', error: null, updated: [], added: [] } }) } as Response;
    let asked = 0;
    vi.stubGlobal('fetch', vi.fn(async (url: string) => {
      if (url !== '/setup/update') return running;
      asked += 1;
      // the first answer comes; the service then answers no more within the test
      return asked === 1 ? running : new Promise<Response>(() => undefined);
    }));
    host = document.createElement('div');
    document.body.append(host);
    root = createRoot(host);
    await act(async () => root.render(<UpdatePlanView />));
    await act(async () => { await vi.advanceTimersByTimeAsync(5000); });
    expect(asked).toBe(2);
  });
});
