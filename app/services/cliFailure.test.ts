import { describe, expect, it } from 'vitest';
import { explainCliFailure } from './cliFailure';

const t = (key: string) => (key === 'cliSigninHow' ? 'run {command} once' : key);

describe('agent CLI failures', () => {
  it('names the sign-in command of the CLI the server quoted', () => {
    expect(explainCliFailure('Codex CLI is not signed in: run `codex login` once in a terminal [cli_not_signed_in]', t)).toBe('cliNotSignedIn Run codex login once');
  });

  it('keeps the CLI reason after a translated quota failure and leaves other errors alone', () => {
    expect(explainCliFailure('Claude Code: usage limit reached [cli_no_credit]', t)).toBe('cliNoCredit Claude Code: usage limit reached');
    expect(explainCliFailure('HTTP 502', t)).toBe('HTTP 502');
  });
});
