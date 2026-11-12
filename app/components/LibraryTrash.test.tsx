import React, { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';

const listTrash = vi.hoisted(() => vi.fn(async () => ({ songs: [], days: 30 })));

vi.mock('../context/I18nContext', () => ({ useI18n: () => ({ t: (key: string) => key, language: 'en' }) }));
vi.mock('../services/nativeLibrary', () => ({
  listTrash,
  deleteTrashedSong: vi.fn(),
  emptyTrash: vi.fn(),
  restoreTrashedSong: vi.fn(),
}));
vi.mock('../services/studioQueries', () => ({ libraryChanged: vi.fn() }));

const { LibraryTrash } = await import('./LibraryTrash');

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
let root: Root;
let host: HTMLDivElement;

afterEach(() => {
  act(() => root?.unmount());
  host?.remove();
  listTrash.mockClear();
});

describe('LibraryTrash', () => {
  it('a page drawn again with a new onToast does not ask for the bin again', async () => {
    host = document.createElement('div');
    document.body.append(host);
    root = createRoot(host);
    const draw = () => root.render(<LibraryTrash onToast={() => undefined} />);
    await act(async () => draw());
    expect(listTrash).toHaveBeenCalledTimes(1);
    for (let again = 0; again < 5; again++) await act(async () => draw());
    expect(listTrash).toHaveBeenCalledTimes(1);
  });
});
