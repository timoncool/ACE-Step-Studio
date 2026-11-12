import { describe, expect, it } from 'vitest';
import { clampUiScale, scaleForKey } from './uiScale';

describe('interface scale', () => {
  it('keeps to 90-150 per cent in steps of ten', () => {
    expect(clampUiScale(80)).toBe(90);
    expect(clampUiScale(155)).toBe(150);
    expect(clampUiScale(114)).toBe(110);
    expect(clampUiScale(Number.NaN)).toBe(100);
  });

  it('answers Ctrl and plus, minus and nought', () => {
    expect(scaleForKey('=', 100)).toBe(110);
    expect(scaleForKey('+', 150)).toBe(150);
    expect(scaleForKey('-', 100)).toBe(90);
    expect(scaleForKey('0', 130)).toBe(100);
    expect(scaleForKey('a', 100)).toBeNull();
  });
});
