import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { CliAgentPanel } from './CliAgentPanel';

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ t: (key: string) => key }) }));
vi.mock('../services/apiBase', () => ({ apiUrl: (path: string) => path }));
vi.mock('../services/studioQueries', () => ({ assistantStatusChanged: () => undefined }));

globalThis.IS_REACT_ACT_ENVIRONMENT = true;

let root: Root;
let host: HTMLDivElement;
let calls: Array<{ url: string; init?: RequestInit }>;
let writeAnswer: { ok: boolean; status: number; body: unknown };

const AGENTS = [
  { id: 'claude', display_name: 'Claude Code', installed: true, path: 'C:/tools/claude.exe', version: '2.1.0', default_model: 'opus', features: [], suggested_models: ['opus'], install: 'npm install -g @anthropic-ai/claude-code', signin: 'run `claude auth login`', docs: 'https://example.invalid/claude' },
  { id: 'opencode', display_name: 'opencode', installed: false, path: null, version: null, default_model: null, features: [], suggested_models: [], install: 'npm install -g opencode-ai', signin: 'run `opencode auth login`', docs: 'https://example.invalid/opencode' },
];

function json(body: unknown, status = 200): Response {
  return { ok: status >= 200 && status < 300, status, json: () => Promise.resolve(body) } as Response;
}

async function mount(status: object = { provider: 'none', cli_agent: null, cli_model: null, cli_path: null }) {
  calls = [];
  writeAnswer = { ok: true, status: 200, body: { style: 'English, warm piano pop, 88 BPM' } };
  globalThis.fetch = vi.fn((url: string, init?: RequestInit) => {
    calls.push({ url, init });
    if (url.endsWith('/v1/assistant/cli-agents')) return Promise.resolve(json({ agents: AGENTS }));
    if (url.endsWith('/v1/assistant/status') && (!init || init.method === undefined || init.method === 'GET')) return Promise.resolve(json(status));
    if (url.endsWith('/v1/assistant/status')) return Promise.resolve(json({ available: true, provider: 'cli' }));
    if (url.endsWith('/v1/assistant/write')) return Promise.resolve(json(writeAnswer.body, writeAnswer.status));
    return Promise.resolve(json({}, 404));
  }) as typeof fetch;
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
  await act(async () => {
    root.render(<CliAgentPanel />);
  });
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 20));
  });
}

afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  vi.unstubAllGlobals();
});

function buttonWith(text: string): HTMLButtonElement {
  const found = Array.from(host.querySelectorAll('button')).find((button) => button.textContent === text);
  if (!found) throw new Error(`no button reading ${text}`);
  return found as HTMLButtonElement;
}

describe('CLI agent panel', () => {
  it('lists an installed CLI with its version and a missing one with its install command', async () => {
    await mount();
    expect(host.textContent).toContain('Claude Code');
    expect(host.textContent).toContain('2.1.0');
    expect(host.textContent).toContain('C:/tools/claude.exe');
    expect(host.textContent).toContain('claude auth login');
    expect(host.textContent).toContain('opencode');
    expect(host.textContent).toContain('npm install -g opencode-ai');
    expect(host.querySelector('a[href="https://example.invalid/opencode"]')).not.toBeNull();
  });

  it('choosing a CLI saves it as the assistant provider', async () => {
    await mount();
    act(() => (host.querySelector('button[aria-label="Claude Code"]') as HTMLButtonElement).click());
    const put = calls.find((call) => call.url.endsWith('/v1/assistant/status') && call.init?.method === 'PUT');
    expect(put).toBeDefined();
    expect(JSON.parse(put!.init!.body as string)).toMatchObject({ provider: 'cli', cli_agent: 'claude' });
    expect(host.textContent).toContain('cliSelected');
  });

  it('tests with a short draft through the write address', async () => {
    await mount({ provider: 'cli', cli_agent: 'claude', cli_model: null, cli_path: null });
    await act(async () => {
      buttonWith('cliTest').click();
      await new Promise((resolve) => setTimeout(resolve, 20));
    });
    const posted = calls.find((call) => call.url.endsWith('/v1/assistant/write'));
    expect(posted).toBeDefined();
    expect(JSON.parse(posted!.init!.body as string)).toMatchObject({ target: 'style' });
    expect(host.textContent).toContain('warm piano pop');
  });

  it('tests the CLI it shows as chosen when none was picked', async () => {
    await mount({ provider: 'cli', cli_agent: null, cli_model: null, cli_path: null });
    expect(host.textContent).toContain('cliSelected');
    await act(async () => {
      buttonWith('cliTest').click();
      await new Promise((resolve) => setTimeout(resolve, 20));
    });
    expect(host.textContent).not.toContain('cliTestNeedsChoice');
    const put = calls.filter((call) => call.url.endsWith('/v1/assistant/status') && call.init?.method === 'PUT').pop();
    expect(JSON.parse(put!.init!.body as string)).toMatchObject({ provider: 'cli', cli_agent: 'claude' });
    expect(host.textContent).toContain('warm piano pop');
  });

  it('answers a classified failure in the interface language', async () => {
    await mount({ provider: 'cli', cli_agent: 'claude', cli_model: null, cli_path: null });
    writeAnswer = { ok: false, status: 502, body: { error: 'Claude Code is not signed in: run `claude auth login` [cli_not_signed_in]' } };
    await act(async () => {
      buttonWith('cliTest').click();
      await new Promise((resolve) => setTimeout(resolve, 20));
    });
    expect(host.textContent).toContain('cliNotSignedIn');
    expect(host.textContent).not.toContain('[cli_not_signed_in]');
  });
});
