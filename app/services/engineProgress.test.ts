import { describe, expect, it } from 'vitest';
import { aceLogProgress } from './engineProgress';

describe('aceLogProgress', () => {
  it('counts code steps against the announced budget', () => {
    const state = aceLogProgress(['[LM-Phase2] max_tokens: 1000, CFG: 2.00, N=1, prompts=1', '[LM-Phase2] Step 500, 1 active, 500 total codes, 80.0 tok/s']);
    expect(state).toEqual({ progress: 0.2, stage: 'stagePlanning' });
  });

  it('puts the DiT after the language model', () => {
    const state = aceLogProgress(['[LM-Phase2] Decode 900ms', '[DiT] Step 4/8 t=0.500']);
    expect(state?.stage).toBe('stageRendering');
    expect(state?.progress).toBeCloseTo(0.675);
  });

  it('reads the latest line only', () => {
    expect(aceLogProgress(['[DiT] Step 8/8 t=0.100', '[VAE] Tiled decode: 4 tiles (chunk=256, overlap=64, stride=192)'])?.stage).toBe('stageDecoding');
    expect(aceLogProgress(['[Server] ready'])).toBeNull();
  });
});
