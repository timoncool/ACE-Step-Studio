import { describe, expect, it } from 'vitest';
import { en } from '../i18n/en';
import { keepLoadedNow } from './EngineSettings';

const t = (key: string) => (en as Record<string, string>)[key] ?? key;
const base = { max_seq: 8192, max_seq_reason: 'full' as const, flash_attention: true };

describe('keepLoadedNow', () => {
  it('says whether the running engine keeps its models and why, in numbers when Auto weighed them', () => {
    expect(keepLoadedNow({ ...base, keep_loaded: true, keep_loaded_reason: 'fits', keep_loaded_need_gb: 18.4, keep_loaded_free_gb: 22 }, t))
      .toBe('Now: kept loaded - 18.4 GB needed, 22.0 GB free');
    expect(keepLoadedNow({ ...base, keep_loaded: false, keep_loaded_reason: 'does_not_fit', keep_loaded_need_gb: 18.4, keep_loaded_free_gb: 11 }, t))
      .toBe('Now: loaded for each song - 18.4 GB needed, only 11.0 GB free');
    expect(keepLoadedNow({ ...base, keep_loaded: false, keep_loaded_reason: 'not_on_card' }, t)).toContain('NVIDIA');
    expect(keepLoadedNow({ ...base, keep_loaded: true, keep_loaded_reason: 'set' }, t)).toBe('Now: kept loaded - as chosen');
  });
});
