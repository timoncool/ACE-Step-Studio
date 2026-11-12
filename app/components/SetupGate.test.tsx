import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { OptionalGroup, WHERE_IT_RUNS } from './SetupGate';

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ t: (key: string) => key }) }));
vi.mock('../services/studioQueries', () => ({ openRouterSettingsChanged: vi.fn(), SELF_READ: { 'x-hub-skip': '1' } }));
vi.mock('./AssistantSettings', () => ({ AssistantExtras: () => null }));
vi.mock('./AssistantRules', () => ({ AssistantRules: () => null }));
vi.mock('./KaraokeSettings', () => ({ KaraokeExtras: () => null }));
vi.mock('./HubTelemetryPreview', () => ({ HubTelemetryPreview: () => null }));
vi.mock('../services/studioHub', () => ({ setTelemetry: vi.fn() }));

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
let root: Root;
let host: HTMLDivElement;

const engines = [{ id: 'managed', label: 'Text engine' }];
const response = (body: unknown): Response => ({ ok: true, status: 200, json: async () => body }) as Response;

afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('optional capability download poll', () => {
  it('asks once a pause while a download runs, though every answer brings new figures', async () => {
    vi.useFakeTimers();
    let asked = 0;
    let downloaded = 0;
    vi.stubGlobal('fetch', vi.fn(async (url: string) => {
      if (url !== '/status') return response({});
      asked += 1;
      downloaded += 1;
      return response({
        provider: 'managed',
        runtime: 'cpu',
        assets: [{ id: 'native-runtime', label: 'Native runtime', kind: 'runtime', bytes: 100, installed: false }],
        active_download: { asset_id: 'native-runtime', downloaded_bytes: downloaded, total_bytes: 100, done: false },
      });
    }));
    host = document.createElement('div');
    document.body.append(host);
    root = createRoot(host);
    await act(async () => root.render(<OptionalGroup title="Writing assistant" purpose="Write songs" statusUrl="/status" installUrl="/install" cancelUrl="/cancel" engines={engines} embedded />));
    await act(async () => { await vi.advanceTimersByTimeAsync(100); });
    const settled = asked;
    await act(async () => { await vi.advanceTimersByTimeAsync(5000); });
    expect(asked - settled).toBeGreaterThanOrEqual(4);
    expect(asked - settled).toBeLessThanOrEqual(6);
  });

  it('a page drawn again with the same engines does not ask the group again', async () => {
    let asked = 0;
    vi.stubGlobal('fetch', vi.fn(async (url: string) => {
      if (url === '/status') asked += 1;
      return response({ provider: 'managed', runtime: 'cpu', assets: [] });
    }));
    host = document.createElement('div');
    document.body.append(host);
    root = createRoot(host);
    const draw = () => root.render(<OptionalGroup title="Writing assistant" purpose="Write songs" statusUrl="/status" installUrl="/install" engines={[{ id: 'managed', label: 'Text engine' }]} embedded />);
    await act(async () => draw());
    const first = asked;
    expect(first).toBeGreaterThanOrEqual(1);
    for (let again = 0; again < 5; again++) await act(async () => draw());
    expect(asked).toBe(first);
  });
});

describe('where each part runs', () => {
  it('says HT-Demucs and Mel-Band RoFormer compute on AMD and Intel cards through Vulkan', () => {
    const row = (name: string) => WHERE_IT_RUNS.find(entry => entry.name === name);
    expect(row('HT-Demucs')?.other).toBe('vulkan');
    expect(row('Mel-Band RoFormer')?.other).toBe('vulkan');
  });
});
