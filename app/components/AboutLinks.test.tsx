import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ t: (key: string) => key, language: 'en' }) }));

const { AuthorLinks, AUTHOR_LINKS } = await import('./AuthorLinks');
const { SupportAuthor, WALLETS } = await import('./SupportAuthor');

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
let root: Root;
let host: HTMLDivElement;

afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  vi.unstubAllGlobals();
});

async function show(node: React.ReactNode) {
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root.render(<>{node}</>));
}

describe('About: the author and support', () => {
  it('signs every link of the author "Network · name", one list, no neuro-cartel', async () => {
    await show(<AuthorLinks />);
    const links = [...host.querySelectorAll('a')];
    expect(links.length).toBe(AUTHOR_LINKS.length);
    const labels = links.map(link => link.querySelector('span')?.textContent);
    for (const label of labels) expect(label).toMatch(/^.+ · .+$/);
    expect(labels).toEqual(['Telegram · @nerual_dreming', 'Telegram · @neuroport', 'X · @nerualdreming', 'GitHub · timoncool', 'aboutSite · artgeneration.me']);
    expect(host.innerHTML).not.toContain('neuro-cartel');
  });

  it('copies exactly the address of a wallet, and has no All links', async () => {
    const written: string[] = [];
    vi.stubGlobal('navigator', { ...navigator, clipboard: { writeText: async (text: string) => { written.push(text); } } });
    await show(<SupportAuthor donatePage="https://timoncool.github.io/ACE-Step-Studio/donate.html" />);
    const copies = [...host.querySelectorAll('button')].filter(button => button.textContent === 'donateCopy');
    expect(copies.length).toBe(WALLETS.length);
    await act(async () => copies[0].click());
    expect(written).toEqual([WALLETS[0].address]);
    expect(written[0]).not.toContain('USDT');
    expect(host.textContent).toContain('donateCopied');
    expect(host.textContent).not.toContain('allLinks');
    expect(host.innerHTML).not.toContain('neuro-cartel');
    expect(host.querySelector('a[href="https://timoncool.github.io/ACE-Step-Studio/donate.html"]')).not.toBeNull();
    expect(host.querySelector('a[href="https://dalink.to/nerual_dreming"]')?.textContent).toBe('donateCardButton');
  });
});
