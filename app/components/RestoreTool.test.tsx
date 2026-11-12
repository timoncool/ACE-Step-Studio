import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ t: (key: string) => key, language: 'en' }) }));

const { RestoreTool } = await import('./RestoreTool');

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
let root: Root;
let host: HTMLDivElement;
const calls: { path: string; body: unknown }[] = [];

afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  calls.length = 0;
  vi.unstubAllGlobals();
});

const model = { id: 'apollo', label: 'Apollo', bytes: 66_398_592, note: 'CC BY-SA 4.0', installed: true };
const ready = { model, runtime_installed: true, ready: true, missing_bytes: 0, error: null, download: null, run: null };

function serve(status: unknown, answers: Record<string, { status: number; body: unknown }> = {}) {
  vi.stubGlobal('fetch', vi.fn(async (path: string, init?: RequestInit) => {
    calls.push({ path, body: init?.body ? JSON.parse(String(init.body)) : undefined });
    const answer = path === '/v1/restore' ? { status: 200, body: status } : answers[path] ?? { status: 200, body: { started: true } };
    return new Response(JSON.stringify(answer.body), { status: answer.status, headers: { 'Content-Type': 'application/json' } });
  }));
}

async function open(props: Partial<React.ComponentProps<typeof RestoreTool>> = {}) {
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root.render(<RestoreTool songId="s1" songTitle="Song" restoredId={null} chosenIsRestored={false} card="" onDone={() => undefined} {...props} />));
  await act(async () => { await new Promise(resolve => setTimeout(resolve, 0)); });
}

const button = (label: string) => [...document.querySelectorAll('button')].find(entry => entry.textContent?.includes(label));
const click = async (element: Element | undefined) => {
  expect(element).toBeDefined();
  await act(async () => { (element as HTMLElement).click(); });
};

describe('RestoreTool', () => {
  it('names the size before downloading, and restores nothing until Apollo is here', async () => {
    serve({ ...ready, ready: false, model: { ...model, installed: false }, missing_bytes: 66_398_592 });
    await open();
    expect(host.textContent).toContain('Apollo · 63 MB');
    expect(button('restoreStart')!.disabled).toBe(true);
    await click(button('restoreInstall'));
    expect(calls.some(call => call.path === '/v1/restore/install')).toBe(true);
  });

  it('restores the chosen track, and asks first when that replaces an earlier restoration', async () => {
    serve(ready);
    await open({ restoredId: 'old' });
    await click(button('restoreAgain'));
    expect(calls.some(call => call.path === '/v1/restore/start'), 'nothing starts before the answer').toBe(false);
    expect(document.body.textContent).toContain('restoreReplaceMessage');
    await click([...document.querySelectorAll('button')].filter(entry => entry.textContent?.includes('restoreReplaceConfirm')).pop());
    expect(calls.find(call => call.path === '/v1/restore/start')?.body).toEqual({ song_id: 's1' });
  });

  it('does not restore a restored track again', async () => {
    serve(ready);
    await open({ chosenIsRestored: true });
    expect(button('restoreStart')!.disabled).toBe(true);
    expect(host.textContent).toContain('restoreOfRestored');
  });

  it('shows the run with its time and a stop, then the result to play and save', async () => {
    serve({ ...ready, run: { song_id: 's1', title: 'Song', stage: 'restoring', started_ms: Date.now(), done: false, error: null, used_gpu: null, library_song: null } });
    await open();
    expect(host.textContent).toContain('restoreRunning');
    await click(button('restoreStop'));
    expect(calls.some(call => call.path === '/v1/restore/cancel')).toBe(true);

    act(() => root.unmount());
    serve({ ...ready, run: { song_id: 's1', title: 'Song', stage: 'done', started_ms: 1, done: true, error: null, used_gpu: true, library_song: 'r1' } });
    await open();
    expect(host.textContent).toContain('restoredLabel');
    expect(host.querySelector('button[title="download"]')).not.toBeNull();
  });

  it('says a stopped run stopped, and a failure in words', async () => {
    serve({ ...ready, run: { song_id: 's1', title: 'Song', stage: 'stopped', started_ms: 1, done: true, error: null, used_gpu: null, library_song: null } });
    await open();
    expect(host.textContent).toContain('restoreStopped');
    act(() => root.unmount());
    serve({ ...ready, run: { song_id: 's1', title: 'Song', stage: 'restoring', started_ms: 1, done: true, error: 'Apollo returned no audio', used_gpu: null, library_song: null } });
    await open();
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('Apollo returned no audio');
  });
});
