/** What the bug form on GitHub asks about this computer, as far as the studio knows it. */
export interface ReportFacts {
  hardware?: { gpuName?: string; totalVramGb?: number; totalRamGb?: number };
  selected_profile_id?: string | null;
  portable?: boolean;
}

function system(portable: boolean | undefined): string {
  const agent = navigator.userAgent;
  const name = /Windows/.test(agent) ? 'Windows' : /Mac OS X|Macintosh/.test(agent) ? 'macOS' : /Linux/.test(agent) ? 'Linux' : '';
  return portable ? `${name} (portable)`.trim() : name;
}

/** The bug form of each window language; the file names keep English first in GitHub's list. */
const BUG_FORMS: Record<string, string> = { en: '1-en-1-bug.yml', ru: '2-ru-1-bug.yml', zh: '3-zh-1-bug.yml', ja: '4-ja-1-bug.yml', ko: '5-ko-1-bug.yml' };

/** The bug form in the window's language, the English one for a language the forms do not have. */
export function bugTemplate(language: string): string {
  return BUG_FORMS[language] ?? BUG_FORMS.en;
}

/**
 * The bug form in the window's language with what the studio knows passed
 * along. The keys are the `id`s of the text fields in
 * `.github/ISSUE_TEMPLATE/*.yml`, the same in every language; the form fills a
 * field from the query parameter of the same name.
 */
export function issueReportUrl(repositoryUrl: string, version: string, language: string, facts?: ReportFacts): string {
  const query = new URLSearchParams({ template: bugTemplate(language), version });
  const os = system(facts?.portable);
  if (os) query.set('os', os);
  const hardware = facts?.hardware;
  if (hardware?.gpuName) {
    const memory = [
      hardware.totalVramGb ? `${Math.round(hardware.totalVramGb)} GB` : '',
      hardware.totalRamGb ? `RAM ${Math.round(hardware.totalRamGb)} GB` : '',
    ].filter(Boolean).join(', ');
    query.set('gpu', memory ? `${hardware.gpuName}, ${memory}` : hardware.gpuName);
  }
  if (facts?.selected_profile_id) query.set('models', facts.selected_profile_id);
  return `${repositoryUrl}/issues/new?${query.toString()}`;
}
