import { describe, expect, it, vi } from 'vitest';
import { en } from '../i18n/en';
import { deleteQuestion } from './deleteQuestion';

const t = (key: 'deleteSongConfirm' | 'deleteSongsConfirm') => en[key];

describe('deleteQuestion', () => {
  it('asks with the days of the setup status and reads nothing', () => {
    const fetch = vi.spyOn(globalThis, 'fetch');
    const one = deleteQuestion(t, [{ title: 'Song $& one' }], 30);
    expect(one).toContain('"Song $& one"');
    expect(one).toContain('30 days');
    const many = deleteQuestion(t, [{ title: 'a' }, { title: 'b' }], 14);
    expect(many).toContain('2 songs');
    expect(many).toContain('14 days');
    expect(many).not.toContain('{');
    expect(fetch).not.toHaveBeenCalled();
    fetch.mockRestore();
  });
});
