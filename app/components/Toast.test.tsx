import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ language: 'en', t: (key: string) => key }) }));

const { Toast } = await import('./Toast');

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
let root: Root;
let host: HTMLDivElement;

beforeEach(() => {
  vi.useFakeTimers();
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.useRealTimers();
});

describe('a message (YuE2 Studio #230)', () => {
  it('an error stays until it is closed; any other message leaves after its time', () => {
    const closed = { error: 0, info: 0 };
    act(() => root.render(<Toast message="failed" type="error" isVisible onClose={() => { closed.error += 1; }} />));
    act(() => { vi.advanceTimersByTime(60_000); });
    expect(closed.error).toBe(0);
    act(() => root.render(<Toast message="saved" type="info" isVisible onClose={() => { closed.info += 1; }} />));
    act(() => { vi.advanceTimersByTime(3_100); });
    expect(closed.info).toBe(1);
  });
});
