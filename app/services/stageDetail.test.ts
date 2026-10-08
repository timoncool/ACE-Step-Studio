import { describe, expect, it } from 'vitest';
import { stageDetail } from './engineProgress';

describe('stageDetail', () => {
  it('shows the counter, then the time the rest of the stage takes at its pace', () => {
    const clock = { current: null };
    expect(stageDetail('10/64', 'acoustic', clock, 0)).toBe('10/64');
    expect(stageDetail('12/64', 'acoustic', clock, 50_000)).toBe('12/64 · ~21:40');
    expect(stageDetail('64/64', 'acoustic', clock, 60_000)).toBe('64/64');
  });

  it('starts its clock again on a new stage and shows a bare count as it is', () => {
    const clock = { current: null };
    stageDetail('300/9000', 'semantic', clock, 0);
    expect(stageDetail('1/32', 'acoustic', clock, 10_000)).toBe('1/32');
    expect(stageDetail('412', 'score', clock, 11_000)).toBe('412');
    expect(stageDetail('', 'decode', clock, 12_000)).toBe('');
  });
});
