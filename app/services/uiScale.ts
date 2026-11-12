import { getCurrentWebview } from '@tauri-apps/api/webview';
import { isDesktop } from './externalLinks';

/** The interface scale, in per cent of the studio's own size. */
export const UI_SCALE_MIN = 90;
export const UI_SCALE_MAX = 150;
export const UI_SCALE_STEP = 10;
export const UI_SCALE_DEFAULT = 100;

const STORAGE_KEY = 'uiScale';
/** Sent when the scale changes, so the setting and the shortcuts agree. */
export const UI_SCALE_EVENT = 'studio:ui-scale';

export function clampUiScale(percent: number): number {
  if (!Number.isFinite(percent)) return UI_SCALE_DEFAULT;
  const stepped = Math.round(percent / UI_SCALE_STEP) * UI_SCALE_STEP;
  return Math.min(UI_SCALE_MAX, Math.max(UI_SCALE_MIN, stepped));
}

/** The scale this window keeps; the studio's own size when none is kept or the storage cannot be read. */
export function storedUiScale(): number {
  try {
    const kept = localStorage.getItem(STORAGE_KEY);
    return kept === null ? UI_SCALE_DEFAULT : clampUiScale(Number(kept));
  } catch (error) {
    console.error('[ERROR] the interface scale could not be read:', error);
    return UI_SCALE_DEFAULT;
  }
}

/**
 * Scales the whole interface: the window's own page zoom in the desktop studio, which lays the page out again at
 * the new size as a browser's zoom does, and the page's zoom in a browser.
 */
export async function applyUiScale(percent: number): Promise<void> {
  const factor = clampUiScale(percent) / 100;
  if (isDesktop()) {
    await getCurrentWebview().setZoom(factor);
  } else {
    document.documentElement.style.zoom = String(factor);
  }
}

/** Keeps and applies a scale, and tells the rest of the window. */
export async function setUiScale(percent: number): Promise<number> {
  const next = clampUiScale(percent);
  try {
    localStorage.setItem(STORAGE_KEY, String(next));
  } catch (error) {
    console.error('[ERROR] the interface scale could not be kept:', error);
  }
  await applyUiScale(next);
  window.dispatchEvent(new CustomEvent(UI_SCALE_EVENT, { detail: next }));
  return next;
}

/** What Ctrl with a key does to the scale: larger, smaller, back to the studio's size; null for other keys. */
export function scaleForKey(key: string, current: number): number | null {
  if (key === '=' || key === '+') return clampUiScale(current + UI_SCALE_STEP);
  if (key === '-' || key === '_') return clampUiScale(current - UI_SCALE_STEP);
  if (key === '0') return UI_SCALE_DEFAULT;
  return null;
}
