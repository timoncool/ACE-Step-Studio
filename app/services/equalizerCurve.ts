/**
 * The filters behind Winamp's ten sliders, as Spotifast builds them
 * (crmne/spotifast, src/eq.rs, MIT). Each band is a peaking filter after the
 * Audio EQ Cookbook, as wide as the gap to its neighbours; adjacent filters
 * overlap, so their gains are solved together until the played response meets
 * every slider at its band's centre. One shared width made the close top bands
 * pile up and overboost presets such as Full Treble.
 */

export const EQ_BANDS = [60, 170, 310, 600, 1000, 3000, 6000, 12000, 14000, 16000] as const;
/** Winamp's range for a band and for the preamp. */
export const EQ_RANGE_DB = 12;
/** Bands closer to flat than this are left at zero. */
const FLAT = 0.05;
/** How far a solved gain may go past the sliders; a guard, not a target. */
const SOLVED_LIMIT = 36;

interface Coefficients {
  b0: number;
  b1: number;
  b2: number;
  a1: number;
  a2: number;
}

/** A band as a Web Audio peaking BiquadFilterNode takes it. */
export interface BandFilter {
  frequency: number;
  q: number;
  gain: number;
}

/** Band widths in octaves: half the gap to each neighbour; the outer bands reach three octaves down and one up, as Winamp's. */
function bandWidths(): number[] {
  const octaves = EQ_BANDS.map((hz) => Math.log2(hz));
  return octaves.map((octave, index) => {
    const below = index > 0 ? octaves[index - 1] : octave - 3;
    const above = index < octaves.length - 1 ? octaves[index + 1] : octave + 1;
    return (octave - below) / 2 + (above - octave) / 2;
  });
}

const WIDTHS = bandWidths();

/**
 * The cookbook's alpha for a width in octaves, taken the digital way with its
 * w0 / sin(w0) term: the plain analog Q comes out far too narrow near the top.
 */
function alpha(hz: number, width: number, sampleRate: number): number {
  const w0 = (2 * Math.PI * hz) / sampleRate;
  const sin = Math.sin(w0);
  return sin * Math.sinh((Math.LN2 / 2) * width * (w0 / sin));
}

function peaking(hz: number, width: number, gainDb: number, sampleRate: number): Coefficients {
  const a = Math.pow(10, gainDb / 40);
  const w0 = (2 * Math.PI * hz) / sampleRate;
  const cos = Math.cos(w0);
  const al = alpha(hz, width, sampleRate);
  const a0 = 1 + al / a;
  return { b0: (1 + al * a) / a0, b1: (-2 * cos) / a0, b2: (1 - al * a) / a0, a1: (-2 * cos) / a0, a2: (1 - al / a) / a0 };
}

/** What a filter does at a frequency, in decibels, on the unit circle: what is played. */
function gainDbAt(filter: Coefficients, hz: number, sampleRate: number): number {
  const w = (2 * Math.PI * hz) / sampleRate;
  const [sin1, cos1, sin2, cos2] = [Math.sin(w), Math.cos(w), Math.sin(2 * w), Math.cos(2 * w)];
  const numerator = (filter.b0 + filter.b1 * cos1 + filter.b2 * cos2) ** 2 + (filter.b1 * sin1 + filter.b2 * sin2) ** 2;
  const denominator = (1 + filter.a1 * cos1 + filter.a2 * cos2) ** 2 + (filter.a1 * sin1 + filter.a2 * sin2) ** 2;
  return 10 * Math.log10(numerator / denominator);
}

/** Gaussian elimination with partial pivoting, for the ten-by-ten interaction of the bands. */
function solve(matrix: number[][], rhs: number[]): number[] {
  const m = matrix.map((row) => [...row]);
  const r = [...rhs];
  const n = r.length;
  for (let column = 0; column < n; column += 1) {
    let pivot = column;
    for (let row = column + 1; row < n; row += 1) if (Math.abs(m[row][column]) > Math.abs(m[pivot][column])) pivot = row;
    [m[column], m[pivot]] = [m[pivot], m[column]];
    [r[column], r[pivot]] = [r[pivot], r[column]];
    const lead = m[column][column];
    if (Math.abs(lead) < 1e-12) continue;
    for (let row = column + 1; row < n; row += 1) {
      const factor = m[row][column] / lead;
      if (factor === 0) continue;
      for (let k = column; k < n; k += 1) m[row][k] -= factor * m[column][k];
      r[row] -= factor * r[column];
    }
  }
  const solution = new Array<number>(n).fill(0);
  for (let row = n - 1; row >= 0; row -= 1) {
    let sum = r[row];
    for (let k = row + 1; k < n; k += 1) sum -= m[row][k] * solution[k];
    const lead = m[row][row];
    solution[row] = Math.abs(lead) < 1e-12 ? 0 : sum / lead;
  }
  return solution;
}

/** The gain each filter plays, zero for a band left flat. */
function solvedGains(bandsDb: number[], sampleRate: number): number[] {
  const target = EQ_BANDS.map((_, index) => bandsDb[index] ?? 0);
  if (target.every((gain) => Math.abs(gain) <= FLAT)) return target.map(() => 0);
  // how much each band moves every centre, per decibel it is given
  const unit = EQ_BANDS.map((centre) => EQ_BANDS.map((hz, j) => gainDbAt(peaking(hz, WIDTHS[j], 1, sampleRate), centre, sampleRate)));
  const played = (gains: number[]) =>
    EQ_BANDS.map((centre) =>
      gains.reduce((sum, gain, j) => (Math.abs(gain) > FLAT ? sum + gainDbAt(peaking(EQ_BANDS[j], WIDTHS[j], gain, sampleRate), centre, sampleRate) : sum), 0),
    );
  let gains = [...target];
  for (let pass = 0; pass < 6; pass += 1) {
    const residual = played(gains).map((db, index) => db - target[index]);
    if (residual.every((value) => Math.abs(value) < 0.01)) break;
    const correction = solve(unit, residual);
    gains = gains.map((gain, index) => Math.max(-SOLVED_LIMIT, Math.min(SOLVED_LIMIT, gain - correction[index])));
  }
  return gains.map((gain) => (Math.abs(gain) > FLAT ? gain : 0));
}

/** The ten filters for the sliders, as the graph's BiquadFilterNodes take them. */
export function bandFilters(bandsDb: number[], sampleRate: number): BandFilter[] {
  const gains = solvedGains(bandsDb, sampleRate);
  return EQ_BANDS.map((hz, index) => ({
    frequency: hz,
    // Web Audio's peaking alpha is sin(w0) / (2Q)
    q: Math.sin((2 * Math.PI * hz) / sampleRate) / (2 * alpha(hz, WIDTHS[index], sampleRate)),
    gain: gains[index],
  }));
}

/** The whole response in decibels, preamp included, at the given frequencies. */
export function responseDb(frequencies: ArrayLike<number>, bandsDb: number[], preampDb: number, sampleRate: number): number[] {
  const filters = solvedGains(bandsDb, sampleRate)
    .map((gain, index) => (gain ? peaking(EQ_BANDS[index], WIDTHS[index], gain, sampleRate) : null))
    .filter((filter): filter is Coefficients => filter !== null);
  return Array.from(frequencies, (hz) => filters.reduce((sum, filter) => sum + gainDbAt(filter, hz, sampleRate), preampDb));
}
