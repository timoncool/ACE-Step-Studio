import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { poll } from './poll';

describe('poll', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('asks again only a pause after the answer, never twice at once', async () => {
    let inFlight = 0;
    let most = 0;
    let calls = 0;
    const answers: Array<() => void> = [];
    const stop = poll(() => {
      calls += 1;
      inFlight += 1;
      most = Math.max(most, inFlight);
      return new Promise<void>((resolve) => answers.push(() => { inFlight -= 1; resolve(); }));
    }, 1000);

    // a slow server: ten seconds without an answer start nothing new
    await vi.advanceTimersByTimeAsync(10_000);
    expect(calls).toBe(1);

    answers.shift()?.();
    await vi.advanceTimersByTimeAsync(999);
    expect(calls).toBe(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(calls).toBe(2);
    expect(most).toBe(1);

    stop();
    answers.shift()?.();
    await vi.advanceTimersByTimeAsync(5_000);
    expect(calls).toBe(2);
  });
});
