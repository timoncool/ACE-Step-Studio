import { Avatar, Style } from '@dicebear/core/lite';
import blobs from '@dicebear/styles/blobs.json';
import constellation from '@dicebear/styles/constellation.json';
import disco from '@dicebear/styles/disco.json';
import glass from '@dicebear/styles/glass.json';
import identicon from '@dicebear/styles/identicon.json';
import landscape from '@dicebear/styles/landscape.json';
import loops from '@dicebear/styles/loops.json';
import marbles from '@dicebear/styles/marbles.json';
import patchwork from '@dicebear/styles/patchwork.json';
import planets from '@dicebear/styles/planets.json';
import rings from '@dicebear/styles/rings.json';
import shadows from '@dicebear/styles/shadows.json';
import shapeGrid from '@dicebear/styles/shape-grid.json';
import shapes from '@dicebear/styles/shapes.json';
import slice from '@dicebear/styles/slice.json';
import squircles from '@dicebear/styles/squircles.json';
import stack from '@dicebear/styles/stack.json';
import stripes from '@dicebear/styles/stripes.json';
import triangles from '@dicebear/styles/triangles.json';
import waves from '@dicebear/styles/waves.json';
import weave from '@dicebear/styles/weave.json';

/**
 * The pattern a track without a cover of its own wears: a DiceBear style (CC0),
 * drawn from the track's seed, so a track always gets the same one and a stem
 * its song's. The service draws the same styles with the same DiceBear version
 * when it writes the pattern into the track.
 */
const DEFINITIONS = {
  waves, squircles, blobs, constellation, disco, glass, identicon, landscape, loops, marbles, patchwork,
  planets, rings, shadows, 'shape-grid': shapeGrid, shapes, slice, stack, stripes, triangles, weave,
};

export type CoverPattern = keyof typeof DEFINITIONS;

/** The styles on offer, the default first. */
export const COVER_PATTERNS = Object.keys(DEFINITIONS) as CoverPattern[];
export const DEFAULT_COVER_PATTERN: CoverPattern = 'waves';

export function isCoverPattern(name: string | undefined | null): name is CoverPattern {
  return typeof name === 'string' && name in DEFINITIONS;
}

/** The styles that draw nothing behind their shapes, and the backgrounds they
 *  get instead, picked by the seed: a cover written into a file is never
 *  see-through. The service's `cover_art.rs` passes the same. */
const SEE_THROUGH: ReadonlySet<CoverPattern> = new Set(['identicon', 'rings']);
const BACKDROPS = ['b6e3f4', 'c0aede', 'd1d4f9', 'ffd5dc', 'ffdfbf'];

const styles = new Map<CoverPattern, Style>();
const drawn = new Map<string, string>();

/** The pattern for a seed, as a data URI an <img>, a background or a tag can take;
 *  a video's background asks for it as wide as its frame, so it stays sharp. */
export function patternArt(seed: string, pattern: CoverPattern = DEFAULT_COVER_PATTERN, size = 256): string {
  const key = `${pattern}\u0000${size}\u0000${seed}`;
  const known = drawn.get(key);
  if (known) return known;
  let style = styles.get(pattern);
  if (!style) {
    style = new Style(DEFINITIONS[pattern]);
    styles.set(pattern, style);
  }
  const art = new Avatar(style, { seed, size, ...(SEE_THROUGH.has(pattern) ? { backgroundColor: BACKDROPS } : {}) }).toDataUri();
  drawn.set(key, art);
  return art;
}
