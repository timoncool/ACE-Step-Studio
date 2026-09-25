import { describe, expect, it } from 'vitest';
import { modelsForCapability, type NativeOpenRouterModel } from './nativeOpenRouter';

describe('modelsForCapability', () => {
  it('uses only capability metadata returned by the native OpenRouter catalog', () => {
    const models: NativeOpenRouterModel[] = [
      { id: 'dynamic/asr', name: 'ASR', capabilities: ['speech_to_text'] },
      { id: 'dynamic/image', name: 'Image', capabilities: ['cover_art'] },
      { id: 'dynamic/text', name: 'Text', capabilities: [] },
    ];

    expect(modelsForCapability(models, 'speech_to_text').map((model) => model.id)).toEqual(['dynamic/asr']);
    expect(modelsForCapability(models, 'cover_art').map((model) => model.id)).toEqual(['dynamic/image']);
  });
});
