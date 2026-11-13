import { describe, expect, it } from 'vitest';
import { applyUiScale, clampUiScale, pageZoom, scaleForKey } from './uiScale';

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

  it('gives the page scale that turns screen pixels into layout pixels', () => {
    const root = document.documentElement;
    root.style.zoom = '';
    expect(pageZoom()).toBe(1);
    root.style.zoom = '1.5';
    expect(pageZoom()).toBe(1.5);
    expect(300 / pageZoom()).toBe(200);
    root.style.zoom = '0.9';
    expect(pageZoom()).toBe(0.9);
    root.style.zoom = '';
  });

  it('keeps --vh and --vw one per cent of the visible window in a scaled browser tab', async () => {
    const root = document.documentElement;
    await applyUiScale(150);
    expect(root.style.zoom).toBe('1.5');
    expect(root.style.getPropertyValue('--vh')).toBe('calc(1vh / 1.5)');
    expect(root.style.getPropertyValue('--vw')).toBe('calc(1vw / 1.5)');
    await applyUiScale(100);
    expect(root.style.getPropertyValue('--vh')).toBe('calc(1vh / 1)');
    root.style.zoom = '';
  });
});
