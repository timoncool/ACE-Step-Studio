import { describe, expect, it } from 'vitest';
import { completeCustomComponentIds, componentPrecision, componentVariant, componentsByKind, type ModelComponent } from './modelCatalog';

const components: ModelComponent[] = [
  { id: 'dit-xl-turbo-q8', kind: 'dit', filename: 'acestep-v15-xl-turbo-Q8_0.gguf', bytes: 1, sha256: 'a' },
  { id: 'lm-4b-q8', kind: 'lm', filename: 'acestep-5Hz-lm-4B-Q8_0.gguf', bytes: 1, sha256: 'b' },
  { id: 'te-qwen3-0.6b-q8', kind: 'text_encoder', filename: 'Qwen3-Embedding-0.6B-Q8_0.gguf', bytes: 1, sha256: 'c' },
  { id: 'vae-standard-bf16', kind: 'vae', filename: 'vae-BF16.gguf', bytes: 1, sha256: 'd' },
];

describe('model catalog helpers', () => {
  it('groups components by role and names their precision and model', () => {
    expect(componentsByKind(components).map((group) => group.components.length)).toEqual([1, 1, 1, 1]);
    expect(componentPrecision(components[0])).toBe('Q8_0');
    expect(componentVariant(components[0])).toBe('xl-turbo');
  });

  it('accepts only a complete set with one file per role', () => {
    expect(completeCustomComponentIds(components, { dit: 'dit-xl-turbo-q8', lm: 'lm-4b-q8', text_encoder: 'te-qwen3-0.6b-q8', vae: 'vae-standard-bf16' })).toHaveLength(4);
    expect(completeCustomComponentIds(components, { dit: 'dit-xl-turbo-q8', lm: 'lm-4b-q8' })).toBeNull();
  });
});
