import { describe, expect, it } from 'vitest';
import { readAudioCodes } from './aceEngine';

describe('audio codes', () => {
  it('reads the studio list and ACE-Step tokens as one comma list', () => {
    expect(readAudioCodes('12, 7  63999\n0')).toEqual({ ok: true, codes: '12,7,63999,0' });
    expect(readAudioCodes('<|audio_code_12|><|audio_code_7|>\n<|audio_code_0|>')).toEqual({ ok: true, codes: '12,7,0' });
    expect(readAudioCodes('  ')).toEqual({ ok: true, codes: '' });
  });

  it('names what is not a code of this model', () => {
    expect(readAudioCodes('12, x')).toEqual({ ok: false, problem: 'notCodes', piece: 'x' });
    expect(readAudioCodes('-3')).toEqual({ ok: false, problem: 'notCodes', piece: '-3' });
    expect(readAudioCodes('64000')).toEqual({ ok: false, problem: 'foreign', code: 64000 });
    expect(readAudioCodes('<|audio_code_70000|>')).toEqual({ ok: false, problem: 'foreign', code: 70000 });
    expect(readAudioCodes('<|audio_code_1|> noise <|audio_code_2|>')).toEqual({ ok: false, problem: 'notCodes', piece: 'noise' });
    expect(readAudioCodes('<|audio_code_12')).toEqual({ ok: false, problem: 'notCodes', piece: '<|audio_code_12' });
  });
});
