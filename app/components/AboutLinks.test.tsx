import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

const windowLanguage = vi.hoisted(() => ({ current: 'en' }));
vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ t: (key: string) => key, language: windowLanguage.current }) }));

const { AuthorLinks, AUTHOR_LINKS, ABOUT_LINK } = await import('./AuthorLinks');
const { en } = await import('../i18n/en');
const { ru } = await import('../i18n/ru');
const { zh } = await import('../i18n/zh');
const { ja } = await import('../i18n/ja');
const { ko } = await import('../i18n/ko');
const { SupportAuthor } = await import('./SupportAuthor');

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
let root: Root;
let host: HTMLDivElement;

afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
});

async function show(node: React.ReactNode) {
  host = document.createElement('div');
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root.render(<>{node}</>));
}

const WALLET_ADDRESSES = ['TQST9Lp2TjK6FiVkn4fwfGUee7NmkxEE7C', '1E7dHL22RpyhJGVpcvKdbyZgksSYkYeEBC', '0xb5db65adf478983186d4897ba92fe2c25c594a0c'];

describe('About: the author and support', () => {
  it('shows the author\'s links as buttons signed "Network · name", no neuro-cartel and no other studios', async () => {
    await show(<AuthorLinks />);
    const links = [...host.querySelectorAll('a')];
    expect(links.length).toBe(AUTHOR_LINKS.length);
    expect(host.querySelectorAll('li').length).toBe(0);
    for (const link of links) expect(link.className).toBe(ABOUT_LINK);
    expect(links.map(link => link.querySelector('span')?.textContent)).toEqual(['Telegram · @nerual_dreming', 'Telegram · @neuroport', 'X · @nerualdreming', 'GitHub · timoncool']);
    expect(host.innerHTML).not.toContain('artgeneration');
    expect(host.textContent).not.toContain('aboutLink');
    expect(host.innerHTML).not.toContain('neuro-cartel');
    expect(host.innerHTML).not.toMatch(/MiniMax|ACE-Step|studioFamily/);
    for (const icon of host.querySelectorAll('svg')) expect(icon.getAttribute('aria-hidden')).toBe('true');
  });

  it('has no explanation lines: no line on where what is, none on sending reports, none per link', () => {
    for (const strings of [en, ru, zh, ja, ko]) {
      expect(Object.keys(strings).filter(key => key.startsWith('aboutLink') || key === 'aboutReportLine' || key === 'aboutAuthorWhere')).toEqual([]);
    }
  });

  it('says "free" once on the whole page in every language and names no page in the text', () => {
    expect(ru.aboutAuthorLede).toBe('Я делаю приложения, которые работают на вашем компьютере: музыка, озвучка, голос и не только. Весь исходный код открыт.');
    expect(ru.supportLine).toBe('Всё здесь бесплатно. Донаты оплачивают время, которое на это уходит.');
    expect(ru.supportPageButton).toBe('Все способы');
    const free = [[en, /\bfree\b/gi], [ru, /бесплатн/gi], [zh, /免费/g], [ja, /無料/g], [ko, /무료/g]] as const;
    for (const [strings, word] of free) {
      const page = [strings.localAIMusicGenerator, strings.problemTitle, strings.aboutAuthor, strings.aboutAuthorLede, strings.supportTheAuthor,
        strings.supportLine, strings.donateCardButton, strings.donateMonthlyButton, strings.supportPageButton].join('\n');
      expect(page.match(word)?.length).toBe(1);
      expect(strings.supportLine).not.toMatch(/Apple Pay|[«“「]/);
    }
  });

  it('supports the author with a card, Boosty and the donation page for the rest, no wallets in the program', async () => {
    await show(<SupportAuthor donatePage="https://timoncool.github.io/ACE-Step-Studio/donate.html" />);
    const buttons = [...host.querySelectorAll('a')];
    expect(buttons.map(button => button.getAttribute('href'))).toEqual(['https://dalink.to/nerual_dreming', 'https://boosty.to/neuro_art', 'https://timoncool.github.io/ACE-Step-Studio/donate.html?from=app&lang=en']);
    const page = new URL(buttons[2].getAttribute('href')!);
    expect(page.searchParams.get('from')).toBe('app');
    expect(page.searchParams.get('lang')).toBe('en');
    act(() => root.unmount());
    host.remove();
    windowLanguage.current = 'ko';
    await show(<SupportAuthor donatePage="https://timoncool.github.io/ACE-Step-Studio/donate.html" />);
    windowLanguage.current = 'en';
    expect(new URL(host.querySelectorAll('a')[2].getAttribute('href')!).searchParams.get('lang')).toBe('ko');
    expect(buttons[2].textContent).toContain('supportPageButton');
    expect(buttons[2].querySelector('svg')).not.toBeNull();
    for (const address of WALLET_ADDRESSES) expect(host.innerHTML).not.toContain(address);
    expect(host.querySelectorAll('code, [aria-expanded]').length).toBe(0);
    expect(host.textContent).not.toContain('allLinks');
  });
});
