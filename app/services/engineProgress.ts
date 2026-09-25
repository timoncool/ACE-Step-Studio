/**
 * How far the engine is with the job it is rendering, read from its log.
 *
 * acestep.cpp logs the language model's code steps every 50 tokens against the
 * `max_tokens` it announced, every DiT step, and the VAE decode. The language
 * model takes the first 40% of the bar, the DiT the next 55%, the VAE the rest.
 */
export type EngineProgress = { progress: number; stage: 'stagePlanning' | 'stageRendering' | 'stageDecoding' };

const LM_SHARE = 0.4;
const DIT_SHARE = 0.55;

export const aceLogProgress = (lines: string[]): EngineProgress | null => {
  for (let index = lines.length - 1; index >= 0; index -= 1) {
    const line = lines[index];
    if (/^\[VAE\] (Decoded|Tiled decode)/.test(line)) return { progress: LM_SHARE + DIT_SHARE, stage: 'stageDecoding' };
    const dit = /^\[DiT\] Step (\d+)\/(\d+)/.exec(line);
    if (dit) return { progress: LM_SHARE + DIT_SHARE * (Number(dit[1]) / Number(dit[2])), stage: 'stageRendering' };
    const codes = /^\[LM-Phase2\] Step (\d+),/.exec(line);
    if (codes) {
      const budget = lines.slice(0, index).reverse().map(entry => /^\[LM-Phase2\] max_tokens: (\d+)/.exec(entry)).find(Boolean);
      const total = budget ? Number(budget[1]) : 0;
      return { progress: total > 0 ? LM_SHARE * Math.min(1, Number(codes[1]) / total) : 0, stage: 'stagePlanning' };
    }
    if (/^\[LM-Phase2\] max_tokens/.test(line) || /^\[LM-(Phase1|Generate)\]/.test(line)) return { progress: 0, stage: 'stagePlanning' };
  }
  return null;
};
