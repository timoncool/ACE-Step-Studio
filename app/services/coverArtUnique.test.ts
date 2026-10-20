import { describe, expect, it } from 'vitest';
import { uniqueCover } from './coverArt';

describe('unique cover pattern', () => {
  it('draws the colours the service writes into the track', () => {
    // cover_art.rs a_unique_pattern_has_the_colours_the_window_draws holds the same
    expect(uniqueCover('waves', 'dark synthwave, male vocal', 'neon on the glass', 'Night')).toEqual({ seed: 'neon on the glass', colors: { backgroundColor: ['be185d'] } });
  });

  it('takes the shapes from the words and the colours from the style', () => {
    const night = uniqueCover('squircles', 'dark synthwave, male vocal', '[Verse 1]\nneon on the glass', 'Night');
    const restyled = uniqueCover('squircles', 'indie pop, warm female vocal', '[Verse 1]\nneon on the glass', 'Night');
    const rewritten = uniqueCover('squircles', 'dark synthwave, male vocal', '[Verse 1]\nriver in the morning', 'Night');
    expect(restyled.seed).toBe(night.seed);
    expect(restyled.colors).not.toEqual(night.colors);
    expect(rewritten.colors).toEqual(night.colors);
    expect(rewritten.seed).not.toBe(night.seed);
    expect(night.colors.shapeColor).toBeUndefined();
    expect(uniqueCover('waves', 'pop', '  ', 'Untitled').seed).toBe('Untitled');
  });
});
