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

/**
 * GitHub's choice of forms (bug, idea, question) with what the studio knows
 * passed along. The keys are the `id`s of the text fields in
 * `.github/ISSUE_TEMPLATE/*.yml`; the chosen form fills a field from the query
 * parameter of the same name.
 */
export function issueReportUrl(repositoryUrl: string, version: string, facts?: ReportFacts): string {
  const query = new URLSearchParams({ version });
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
  return `${repositoryUrl}/issues/new/choose?${query.toString()}`;
}
