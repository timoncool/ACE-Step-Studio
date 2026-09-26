import { apiUrl } from './apiBase';

/**
 * The Winamp mode: the studio's window becomes a classic Winamp 2 player
 * (Webamp) wearing .wsz skins. Its settings, the skins the studio carries and
 * the ones the user added, and the running player for the agent's commands.
 */

export interface WinampSkin {
  name: string;
  url: string;
  /** Carried by the studio, rather than added by the user. */
  builtIn: boolean;
}

export interface WinampSettings {
  /** The skin by name; null is Winamp's own base skin. */
  skin: string | null;
  /** A different skin each time the mode opens, never the same twice running. */
  random: boolean;
  doubleSize: boolean;
  alwaysOnTop: boolean;
  /** How large the whole player is drawn, 1 = Winamp's own pixels. */
  scale: number;
  /** The button starts the mode at once; its menu opens on a long press. */
  skipMenu: boolean;
}

export const SCALE_RANGE = { min: 1, max: 3, step: 0.1 };

/**
 * Skins that come with the studio: the ones Webamp's own demo carries, and the
 * Classified trio from the top of the Winamp Skin Museum (skins.webamp.org).
 */
const BUILT_IN: [string, string][] = [
  ['Green Dimension V2', 'Green-Dimension-V2.wsz'],
  ['Internet Archive', 'Internet-Archive.wsz'],
  ['Mac OS X v1.5 (Aqua)', 'MacOSXAqua1-5.wsz'],
  ['TopazAmp', 'TopazAmp1-2.wsz'],
  ['Vizor', 'Vizor1-01.wsz'],
  ['XMMS Turquoise', 'XMMS-Turquoise.wsz'],
  ['Zaxon Remake', 'ZaxonRemake1-0.wsz'],
  ['Winamp5 Classified', 'Winamp5-Classified.wsz'],
  ['Winamp3 Classified', 'Winamp3-Classified.wsz'],
  ['Bento Classified', 'Bento-Classified.wsz'],
];

export const SKIN_MUSEUM = 'https://skins.webamp.org';

const STORE = 'studio:winamp';
const DEFAULT: WinampSettings = { skin: 'Winamp5 Classified', random: false, doubleSize: false, alwaysOnTop: false, scale: 1, skipMenu: false };

function load(): WinampSettings {
  try {
    return { ...DEFAULT, ...(JSON.parse(localStorage.getItem(STORE) ?? 'null') ?? {}) };
  } catch {
    return DEFAULT;
  }
}

let settings = load();
let on = false;
const listeners = new Set<() => void>();

const notify = () => listeners.forEach((listener) => listener());

export function winampSettings(): WinampSettings {
  return settings;
}

export function setWinampSettings(change: Partial<WinampSettings>): void {
  settings = { ...settings, ...change };
  settings.scale = Math.round(Math.max(SCALE_RANGE.min, Math.min(SCALE_RANGE.max, Number(settings.scale) || 1)) * 100) / 100;
  try {
    localStorage.setItem(STORE, JSON.stringify(settings));
  } catch {
    // kept for this run only
  }
  notify();
}

export function winampOn(): boolean {
  return on;
}

export function setWinampOn(value: boolean): void {
  if (on === value) return;
  on = value;
  notify();
}

export function onWinamp(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** Every skin there is: the studio's, then the user's, by name. */
export async function winampSkins(): Promise<WinampSkin[]> {
  const builtIn = BUILT_IN.map(([name, file]) => ({ name, url: `/skins/${file}`, builtIn: true }));
  const response = await fetch(apiUrl('/v1/skins'));
  if (!response.ok) throw new Error(`the skins folder: ${response.status} ${await response.text()}`);
  const own = ((await response.json()) as { skins: { name: string; url: string }[] }).skins.map((skin) => ({ name: skin.name, url: apiUrl(skin.url), builtIn: false }));
  return [...builtIn, ...own.filter((skin) => !BUILT_IN.some(([name]) => name === skin.name))];
}

/** Keeps a skin file in the studio's skins folder; the added skin comes back. */
export async function addWinampSkin(file: Blob, name: string): Promise<WinampSkin> {
  const response = await fetch(apiUrl('/v1/skins'), {
    method: 'POST',
    headers: { 'X-File-Name': encodeURIComponent(name) },
    body: file,
  });
  if (!response.ok) throw new Error(((await response.json().catch(() => null)) as { error?: string } | null)?.error ?? response.statusText);
  const added = (await response.json()) as { name: string; url: string };
  return { name: added.name, url: apiUrl(added.url), builtIn: false };
}

/** The skin to open with: the chosen one, or with random on a different one than last time. */
export function pickSkin(skins: WinampSkin[]): WinampSkin | null {
  if (settings.random && skins.length > 1) {
    const others = skins.filter((skin) => skin.name !== settings.skin);
    const chosen = others[Math.floor(Math.random() * others.length)];
    setWinampSettings({ skin: chosen.name });
    return chosen;
  }
  return skins.find((skin) => skin.name === settings.skin) ?? null;
}

/** What the running player answers the agent with, and does for it. */
export interface WinampControl {
  state: () => Record<string, unknown>;
  set: (change: Record<string, unknown>) => Promise<string>;
  /** Back to the studio, handing it the song, position, volume and equalizer. */
  leave: () => void;
}

let control: WinampControl | null = null;

export function registerWinampControl(next: WinampControl | null): void {
  control = next;
}

export function winampControl(): WinampControl | null {
  return control;
}
