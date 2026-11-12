import { describe, expect, it } from 'vitest';
import { songErrorText } from './songErrors';

describe('songErrorText', () => {
  it('fills the fields into the template of the window language', () => {
    const failure = { message: 'English text', code: 'vae_not_finite', params: { count: 12, total: 3840000 } };
    const ru = songErrorText(failure, 'ru');
    expect(ru).toContain('12 из 3840000');
    expect(ru).toContain('VAE');
    expect(songErrorText(failure, 'ja')).toContain('3840000 個中 12 個');
    expect(songErrorText({ message: 'x', code: 'track_too_long', params: { length: '12:34' } }, 'ko')).toContain('12:34');
  });

  it('shows the service text as it is without a code or a template', () => {
    expect(songErrorText({ message: 'raw words' }, 'ru')).toBe('raw words');
    expect(songErrorText({ message: 'raw words', code: 'no_such_code' }, 'ko')).toBe('raw words');
    expect(songErrorText(null, 'en')).toBe('');
  });

  it('keeps a placeholder whose field did not come', () => {
    expect(songErrorText({ message: 'x', code: 'engine_fatal', params: {} }, 'en')).toBe('The engine stopped this song: {line}');
  });
});
