import { creator, parser, type EqfPreset } from 'winamp-eqf';
import { EQ_BANDS, EQ_RANGE_DB } from './equalizerCurve';

/**
 * Equalizer presets in decibels: Winamp's own, in its order and with its
 * values, then presets for common situations (Spotifast's list, MIT), then
 * the user's. Winamp's .EQF files come in and go out through winamp-eqf, whose
 * bands are 1..64 with 33 at zero.
 */

export interface EqPreset {
  name: string;
  preamp: number;
  gains: number[];
}

const preset = (name: string, gains: number[]): EqPreset => ({ name, preamp: 0, gains });

export const WINAMP_PRESETS: EqPreset[] = [
  preset('Flat', [0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
  preset('Classical', [0, 0, 0, 0, 0, 0, -7.2, -7.2, -7.2, -9.6]),
  preset('Club', [0, 0, 8, 5.6, 5.6, 5.6, 3.2, 0, 0, 0]),
  preset('Dance', [9.6, 7.2, 2.4, 0, 0, -5.6, -7.2, -7.2, 0, 0]),
  preset('Full Bass', [-8, 9.6, 9.6, 5.6, 1.6, -4, -8, -10.4, -11.2, -11.2]),
  preset('Full Bass & Treble', [7.2, 5.6, 0, -7.2, -4.8, 1.6, 8, 11.2, 12, 12]),
  preset('Full Treble', [-9.6, -9.6, -9.6, -4, 2.4, 11.2, 12, 12, 12, 12]),
  preset('Laptop Speakers / Headphones', [4.8, 11.2, 5.6, -3.2, -2.4, 1.6, 4.8, 9.6, 12, 12]),
  preset('Large Hall', [10.4, 10.4, 5.6, 5.6, 0, -4.8, -4.8, -4.8, 0, 0]),
  preset('Live', [-4.8, 0, 4, 5.6, 5.6, 5.6, 4, 2.4, 2.4, 2.4]),
  preset('Party', [7.2, 7.2, 0, 0, 0, 0, 0, 0, 7.2, 7.2]),
  preset('Pop', [-1.6, 4.8, 7.2, 8, 5.6, 0, -2.4, -2.4, -1.6, -1.6]),
  preset('Reggae', [0, 0, 0, -5.6, 0, 6.4, 6.4, 0, 0, 0]),
  preset('Rock', [8, 4.8, -5.6, -8, -3.2, 4, 8.8, 11.2, 11.2, 11.2]),
  preset('Ska', [-2.4, -4.8, -4, 0, 4, 5.6, 8.8, 9.6, 11.2, 9.6]),
  preset('Soft', [4.8, 1.6, 0, -2.4, 0, 4, 8, 9.6, 11.2, 12]),
  preset('Soft Rock', [4, 4, 2.4, 0, -4, -5.6, -3.2, 0, 2.4, 8.8]),
  preset('Techno', [8, 5.6, 0, -5.6, -4.8, 0, 8, 9.6, 9.6, 8.8]),
];

export const SITUATION_PRESETS: EqPreset[] = [
  preset('Bass Booster', [8.8, 7.2, 5.6, 3.2, 0.8, 0, 0, 0, 0, 0]),
  preset('Bass Reducer', [-8.8, -7.2, -5.6, -3.2, -0.8, 0, 0, 0, 0, 0]),
  preset('Treble Booster', [0, 0, 0, 0, 0, 0.8, 3.2, 5.6, 7.2, 8.8]),
  preset('Vocal Booster', [-2.4, -4.8, -4.8, 1.6, 5.6, 5.6, 4, 1.6, 0, -2.4]),
  preset('Small Speakers', [-8, -6.4, -4, -1.6, 1.6, 3.2, 4.8, 5.6, 5.6, 5.6]),
  preset('Spoken Word', [-3.2, -0.8, 0, 0.8, 4, 5.6, 4.8, 2.4, 0.8, 0]),
  preset('Loudness', [9.6, 6.4, 0, 0, -2.4, 0, -1.6, 0, 8, 1.6]),
  preset('Night Listening', [-4.8, -3.2, -1.6, 0.8, 2.4, 3.2, 2.4, 0.8, -1.6, -3.2]),
];

const KEYS = ['hz60', 'hz170', 'hz310', 'hz600', 'hz1000', 'hz3000', 'hz6000', 'hz12000', 'hz14000', 'hz16000'] as const;

const toDb = (value: number) => Math.round(((value - 1) / 31.5 - 1) * EQ_RANGE_DB * 10) / 10;
const fromDb = (db: number) => Math.max(1, Math.min(64, Math.round((db / EQ_RANGE_DB + 1) * 31.5 + 1)));

/** The presets inside an .EQF file. */
export function readEqf(bytes: ArrayBuffer): EqPreset[] {
  return parser(bytes).presets.map((entry) => ({ name: entry.name, preamp: toDb(entry.preamp), gains: KEYS.map((key) => toDb(entry[key])) }));
}

export function writeEqf(presets: EqPreset[]): Blob {
  const eqf: EqfPreset[] = presets.map((entry) => ({
    name: entry.name,
    ...(Object.fromEntries(KEYS.map((key, index) => [key, fromDb(entry.gains[index] ?? 0)])) as Record<(typeof KEYS)[number], number>),
    preamp: fromDb(entry.preamp),
  }));
  return new Blob([creator({ presets: eqf })], { type: 'application/octet-stream' });
}

/** Presets the user saved or imported. */
const OWN = 'studio:equalizer-own-presets';

export function ownPresets(): EqPreset[] {
  try {
    const list = JSON.parse(localStorage.getItem(OWN) ?? '[]') as EqPreset[];
    return Array.isArray(list)
      ? list.filter((entry) => typeof entry?.name === 'string' && Array.isArray(entry.gains) && entry.gains.length === EQ_BANDS.length)
      : [];
  } catch {
    return [];
  }
}

export function storeOwnPresets(presets: EqPreset[]): void {
  localStorage.setItem(OWN, JSON.stringify(presets));
  window.dispatchEvent(new CustomEvent(OWN_CHANGED));
}

/** Sent when the user's presets change, from the panel or the agent. */
export const OWN_CHANGED = 'studio:equalizer-own-presets';

export const BAND_LABELS = EQ_BANDS.map((frequency) => (frequency >= 1000 ? `${frequency / 1000}K` : String(frequency)));
