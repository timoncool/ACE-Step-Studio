import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ t: (key: string) => key, language: 'ru' }) }));

const { ProblemReportDialog, refusalText } = await import('./ProblemReportDialog');

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

function serve(answers: Record<string, { status: number; body: unknown }>) {
  vi.stubGlobal('fetch', vi.fn(async (path: string, init?: RequestInit) => {
    calls.push({ path, body: init?.body ? JSON.parse(String(init.body)) : undefined });
    const answer = answers[path] ?? { status: 404, body: {} };
    return new Response(JSON.stringify(answer.body), { status: answer.status, headers: { 'Content-Type': 'application/json' } });
  }));
}

async function open() {
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root.render(<ProblemReportDialog onClose={() => undefined} />));
}

async function type(value: string) {
  const field = host.querySelector('textarea')!;
  const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!;
  await act(async () => {
    setter.call(field, value);
    field.dispatchEvent(new Event('input', { bubbles: true }));
  });
}

const settle = () => act(async () => new Promise(resolve => setTimeout(resolve, 350)));

const sendButton =() => [...host.querySelectorAll('button')].find(button => button.textContent?.includes('problemSend'))!;

describe('ProblemReportDialog', () => {
  it('sends only with what happened written, and keeps the GitHub form beside', async () => {
    serve({ '/v1/problem/report': { status: 200, body: { number: 77, url: 'https://github.com/x/y/issues/77' } } });
    await open();
    expect(sendButton().disabled).toBe(true);
    expect(host.querySelectorAll('input[type="radio"]').length, 'no choice of way: two buttons say it').toBe(0);
    expect(host.textContent, 'the window carries no links to the author').not.toContain('problemAuthorNews');
    expect(host.querySelector('a[href="https://x.com/nerualdreming"]')).toBeNull();
    await type('short');
    expect(sendButton().disabled, 'under ten characters').toBe(true);
    await type('The song stops at decoding every time');
    expect(sendButton().disabled).toBe(false);
    await act(async () => sendButton().click());
    expect(calls.find(call => call.path === '/v1/problem/report')?.body).toMatchObject({ kind: 'bug', what: 'The song stops at decoding every time', attach_log: true });
    expect(host.textContent).toContain('problemSent');
    expect(host.querySelector('a[href="https://github.com/x/y/issues/77"]')).not.toBeNull();
  });

  it('shows exactly the request the service would send when "what will be sent" is opened', async () => {
    serve({ '/v1/problem/preview': { status: 200, body: { version: '4.0.0', models: 'quality-q8', log: 'line one' } } });
    await open();
    await type('The song stops at decoding every time');
    const details = host.querySelector('details')!;
    await act(async () => {
      details.open = true;
      details.dispatchEvent(new Event('toggle'));
    });
    await settle();
    expect(calls.some(call => call.path === '/v1/problem/preview')).toBe(true);
    expect(host.querySelector('pre')?.textContent).toContain('"models": "quality-q8"');
    expect(host.querySelector('pre')?.textContent).toContain('line one');
    expect(calls.every(call => call.path !== '/v1/problem/list'), 'the window lists no earlier messages').toBe(true);
  });

  it('asks for the preview again when the kind or the log choice changes while it is open', async () => {
    serve({ '/v1/problem/preview': { status: 200, body: { log: 'line one' } } });
    await open();
    await type('The song stops at decoding every time');
    const details = host.querySelector('details')!;
    await act(async () => {
      details.open = true;
      details.dispatchEvent(new Event('toggle'));
    });
    await settle();
    await act(async () => [...host.querySelectorAll('[role="radio"]')].find(radio => radio.textContent === 'problemKind_idea')!.dispatchEvent(new MouseEvent('click', { bubbles: true })));
    await settle();
    const previews = calls.filter(call => call.path === '/v1/problem/preview');
    expect(previews.at(-1)?.body).toMatchObject({ kind: 'idea', attach_log: false });
  });

  it('takes the fields, the log choice and the GitHub form of the kind chosen', async () => {
    serve({ '/v1/problem/report': { status: 200, body: { number: 8, url: 'https://github.com/x/y/issues/8' } } });
    await open();
    const pick = async (label: string) => act(async () => [...host.querySelectorAll('[role="radio"]')].find(radio => radio.textContent === label)!.dispatchEvent(new MouseEvent('click', { bubbles: true })));
    const log = () => (host.querySelector('input[type="checkbox"]') as HTMLInputElement).checked;
    expect(host.textContent).toContain('problemSteps');
    expect(log()).toBe(true);
    await pick('problemKind_idea');
    expect(host.textContent).toContain('problemIdeaWhat');
    expect(host.textContent).toContain('problemIdeaHow');
    expect(host.textContent).not.toContain('problemSteps');
    expect(log(), 'an idea takes no log unless ticked').toBe(false);
    await pick('problemKind_question');
    expect(host.querySelectorAll('textarea').length).toBe(1);
    await type('How do I make a song longer than four minutes?');
    await act(async () => sendButton().click());
    expect(calls.find(call => call.path === '/v1/problem/report')?.body).toMatchObject({ kind: 'question', steps: '', expected: '', attach_log: false });
  });

  it('offers two ways as two buttons named by their result, GitHub the main one, both explained in view', async () => {
    serve({});
    await open();
    const footer = () => [...host.querySelectorAll('.mt-5 a, .mt-5 button')] as HTMLElement[];
    const label = (element: HTMLElement) => element.querySelector('span')?.textContent;
    expect(host.querySelectorAll('input[type="radio"]').length).toBe(0);
    expect(footer().map(label)).toEqual(['problemSendOwn', 'problemSendAnon']);
    expect(footer()[0].tagName, 'the main one is the GitHub form, first on a phone').toBe('A');
    expect(footer()[0].className).toContain('bg-pink-600');
    expect(footer()[1].className, 'anonymous sending is the fallback').not.toContain('bg-pink-600');
    for (const element of footer()) expect(label(element)?.startsWith('problemSend')).toBe(true);
    const text = host.textContent ?? '';
    for (const line of ['problemOwnName', 'problemOwnLine', 'problemAnonName', 'problemAnonLine', 'problemAttachLog']) expect(text).toContain(line);
    expect(text.indexOf('problemOwnLine')).toBeLessThan(text.indexOf('problemAnonLine'));
    expect(text.indexOf('problemAnonLine'), 'the log belongs to the anonymous way, after its line').toBeLessThan(text.indexOf('problemAttachLog'));
    expect(footer()[0].getAttribute('aria-disabled')).toBe('true');
    await type('The song stops at decoding\nevery time');
    const link = new URL(footer()[0].getAttribute('href')!);
    expect(link.searchParams.get('template')).toBe('2-ru-1-bug.yml');
    expect(link.searchParams.get('what')).toBe('The song stops at decoding\nevery time');
    expect(link.searchParams.get('title')).toBe('The song stops at decoding');
    expect(footer()[0].getAttribute('target')).toBe('_blank');
  });

  it('says why a report did not go, each kind in its own words', () => {
    const t = (key: string) => key;
    expect(refusalText({ code: 'rate', retry_after_s: 1200 }, (key) => (key === 'problemErrRate' ? 'in {min} min' : t(key)))).toBe('in 20 min');
    for (const [code, key] of [['not_configured', 'problemErrNotConfigured'], ['busy', 'problemErrBusy'], ['unreachable', 'problemErrUnreachable'], ['field', 'problemErrField']] as const) {
      expect(refusalText({ code }, t)).toBe(key);
    }
  });
});
