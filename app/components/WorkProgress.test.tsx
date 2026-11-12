import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { WorkProgress, elapsedClock } from './WorkProgress';

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ language: 'en', t: (key: string) => ({ workElapsed: 'running {time}' }[key] ?? key) }) }));

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
let root: Root;
let host: HTMLDivElement;

function mount(node: React.ReactElement) {
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
  act(() => root.render(node));
}

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.useRealTimers();
});

describe('WorkProgress', () => {
  it('shows the percent and a bar filled to it when the server reports one', () => {
    mount(<WorkProgress label="Splitting" percent={40} startedMs={Date.now()} />);
    expect(host.textContent).toContain('40%');
    expect(host.textContent).not.toContain('running');
    expect(host.querySelector('[role="progressbar"]')?.getAttribute('aria-valuenow')).toBe('40');
  });

  it('shows how long the job has run and no figure when the server reports none', () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date('2026-11-12T12:00:00Z'));
    mount(<WorkProgress label="Transcribing" percent={null} startedMs={Date.now() - 65_000} />);
    expect(host.textContent).toContain('running 1:05');
    expect(host.textContent).not.toContain('%');
    expect(host.querySelector('[role="progressbar"]')?.hasAttribute('aria-valuenow')).toBe(false);
    act(() => { vi.advanceTimersByTime(2_000); });
    expect(host.textContent).toContain('running 1:07');
  });

  it('writes hours once a job passes an hour', () => {
    expect(elapsedClock(3_725)).toBe('1:02:05');
    expect(elapsedClock(-3)).toBe('0:00');
  });
});
