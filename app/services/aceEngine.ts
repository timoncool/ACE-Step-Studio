/** The samplers acestep.cpp takes by name, as the create form and the re-render offer them. */
export const SOLVERS = ['euler', 'heun', 'sde', 'dpm2m', 'dpm2m_ada', 'dpm3m', 'stork2', 'stork4', 'unipc', 'unipc_p', 'aflops', 'aflops2', 'jkass_fast', 'jkass_quality', 'rfsolver', 'rk4', 'rk5', 'gl2s', 'dopri5', 'dop853'];
export const SCHEDULERS = ['linear', 'linear_quadratic', 'ddim_uniform', 'sgm_uniform', 'bong_tangent', 'cosine', 'power', 'beta57'];

/** The last index of ACE-Step 1.5's FSQ codebook (levels 8·8·8·5·5·5). */
export const MAX_AUDIO_CODE = 63999;

const AUDIO_CODE_TOKEN = /<\|audio_code_([^|]*)\|>/g;

/** Audio codes as written, read the way the service reads them: numbers separated by
 *  commas or spaces, or ACE-Step's `<|audio_code_N|>` tokens. */
export type AudioCodes =
  | { ok: true; codes: string }
  | { ok: false; problem: 'notCodes'; piece: string }
  | { ok: false; problem: 'foreign'; code: number };

export function readAudioCodes(text: string): AudioCodes {
  const trimmed = text.trim();
  const tokens = trimmed.includes('<|audio_code_');
  const pieces: string[] = [];
  if (tokens) {
    const between = trimmed.replace(AUDIO_CODE_TOKEN, ' ').replace(/[,\s]+/g, '');
    if (between) return { ok: false, problem: 'notCodes', piece: between.slice(0, 40) };
    for (const match of trimmed.matchAll(AUDIO_CODE_TOKEN)) pieces.push(match[1]);
  } else {
    pieces.push(...trimmed.split(/[,\s]+/).filter(Boolean));
  }
  const codes: number[] = [];
  for (const piece of pieces) {
    if (!/^\d+$/.test(piece)) return { ok: false, problem: 'notCodes', piece: piece.slice(0, 40) };
    const code = Number(piece);
    if (code > MAX_AUDIO_CODE) return { ok: false, problem: 'foreign', code };
    codes.push(code);
  }
  return { ok: true, codes: codes.join(',') };
}
