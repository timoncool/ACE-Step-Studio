import { describe, expect, it } from 'vitest';
import { bandFilters, EQ_BANDS, EQ_RANGE_DB, responseDb } from './equalizerCurve';
import { SITUATION_PRESETS, WINAMP_PRESETS } from './equalizerPresets';

const RATE = 44100;
const flat = () => EQ_BANDS.map(() => 0);
const at = (hz: number, bands: number[], preamp = 0) => responseDb([hz], bands, preamp, RATE)[0];

describe('equalizer curve', () => {
  it('peaks at the band and adds the preamp', () => {
    const bands = flat();
    bands[7] = 6;
    expect(Math.abs(at(12000, bands, -3) - 3)).toBeLessThan(0.1);
    expect(Math.abs(at(100, bands, -3) + 3)).toBeLessThan(0.2);
  });

  it('meets the sliders at every band for every preset', () => {
    for (const preset of [...WINAMP_PRESETS, ...SITUATION_PRESETS]) {
      EQ_BANDS.forEach((hz, index) => {
        expect(Math.abs(at(hz, preset.gains) - preset.gains[index]), `${preset.name} at ${hz} Hz`).toBeLessThan(0.3);
      });
    }
  });

  it('leaves the neighbours centres alone, even at the top', () => {
    const bands = flat();
    bands[8] = 12;
    expect(Math.abs(at(14000, bands) - 12)).toBeLessThan(0.3);
    expect(Math.abs(at(12000, bands))).toBeLessThan(0.3);
    expect(Math.abs(at(16000, bands))).toBeLessThan(0.3);
    expect(Math.abs(at(1000, bands))).toBeLessThan(0.1);
    bands[7] = 12;
    expect(at(13000, bands)).toBeGreaterThan(9);
  });

  it('lets the outer sliders reach the ends', () => {
    const low = flat();
    low[0] = 12;
    expect(at(30, low)).toBeGreaterThan(6);
    expect(Math.abs(at(170, low))).toBeLessThan(0.3);
    const high = flat();
    high[9] = 12;
    expect(at(19000, high)).toBeGreaterThan(5);
    expect(Math.abs(at(14000, high))).toBeLessThan(0.3);
  });

  it('keeps every preset within range, Flat first', () => {
    expect(WINAMP_PRESETS[0].name).toBe('Flat');
    for (const preset of [...WINAMP_PRESETS, ...SITUATION_PRESETS]) {
      expect(preset.gains).toHaveLength(EQ_BANDS.length);
      expect(preset.gains.every((gain) => Math.abs(gain) <= EQ_RANGE_DB)).toBe(true);
    }
  });

  it('gives Web Audio a positive Q per band and zero gain when flat', () => {
    const filters = bandFilters(flat(), RATE);
    expect(filters.every((filter) => filter.q > 0 && filter.gain === 0)).toBe(true);
  });
});
