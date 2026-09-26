declare module 'butterchurn' {
  export interface ButterchurnVisualizer {
    connectAudio(node: AudioNode): void;
    disconnectAudio(node: AudioNode): void;
    loadPreset(preset: object, blendTime: number): void;
    setRendererSize(width: number, height: number): void;
    render(): void;
  }
  const butterchurn: {
    createVisualizer(context: AudioContext, canvas: HTMLCanvasElement, options: { width: number; height: number; pixelRatio?: number; textureRatio?: number }): ButterchurnVisualizer;
  };
  export default butterchurn;
}

declare module 'butterchurn-presets' {
  const presets: { getPresets(): Record<string, object> };
  export default presets;
}

declare module 'butterchurn-presets/lib/butterchurnPresetsExtra.min.js' {
  const presets: { getPresets(): Record<string, object> };
  export default presets;
}

declare module 'butterchurn-presets/lib/butterchurnPresetsExtra2.min.js' {
  const presets: { getPresets(): Record<string, object> };
  export default presets;
}

declare module 'butterchurn-presets/lib/butterchurnPresetsMD1.min.js' {
  const presets: { getPresets(): Record<string, object> };
  export default presets;
}
