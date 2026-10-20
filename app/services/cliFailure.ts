/** What each agent CLI runs once in a terminal to sign in. */
export const CLI_SIGNIN: Record<string, string> = {
  claude: 'claude auth login',
  codex: 'codex login',
  opencode: 'opencode auth login',
  antigravity: 'agy',
  muse: 'muse',
};

type Key = 'cliNotSignedIn' | 'cliNoCredit' | 'cliThrottled' | 'cliSigninHow';

/** The machine tags the server appends to classified CLI failures. */
const TAGS: Array<{ tag: string; key: Exclude<Key, 'cliSigninHow'> }> = [
  { tag: '[cli_not_signed_in]', key: 'cliNotSignedIn' },
  { tag: '[cli_no_credit]', key: 'cliNoCredit' },
  { tag: '[cli_throttled]', key: 'cliThrottled' },
];

/**
 * A failure of the agent CLI assistant in the interface language. A sign-in
 * failure names the command of the CLI the server quoted, or of `fallback`.
 */
export function explainCliFailure(raw: string, t: (key: Key) => string, fallback = ''): string {
  if (raw.includes('[cli_not_signed_in]')) {
    const id = Object.keys(CLI_SIGNIN).find((candidate) => raw.includes(`\`${CLI_SIGNIN[candidate]}\``)) ?? fallback;
    const how = t('cliSigninHow').replace('{command}', CLI_SIGNIN[id] ?? id);
    return `${t('cliNotSignedIn')} ${how.charAt(0).toUpperCase()}${how.slice(1)}`;
  }
  for (const { tag, key } of TAGS) {
    if (raw.includes(tag)) return `${t(key)} ${raw.replace(tag, '').trim()}`;
  }
  return raw;
}
