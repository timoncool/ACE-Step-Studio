/** A run of a tool as the service reports it: the song it works on and whether it has ended. */
export interface ToolRun {
  song_id: string | null;
  done: boolean;
}

/**
 * The songs the Tools list draws: the first `limit` matches, with the chosen one always among them - first, when it
 * would otherwise be past the limit - so the song the tools work on is never out of sight.
 */
export function shownSongs<T extends { id: string }>(matches: T[], chosen: string, limit: number): T[] {
  const first = matches.slice(0, limit);
  if (!chosen || first.some(song => song.id === chosen)) return first;
  const picked = matches.find(song => song.id === chosen);
  return picked ? [picked, ...first.slice(0, limit - 1)] : first;
}

/**
 * The song the Tools tab opens on when nothing chose one: the song a stems or MIDI run is still working on, so its
 * progress is in view at once, else the first song of the library.
 */
export function toolsSongOnOpen(playable: string[], runs: (ToolRun | null | undefined)[]): string {
  const working = runs.find(run => run && !run.done && run.song_id && playable.includes(run.song_id));
  return working?.song_id ?? playable[0] ?? '';
}

/** Why the library did not answer: its status, and its body only when it said something. */
export function libraryFailure(status: number, body: string): string {
  const said = body.trim();
  return said ? `${status} ${said}` : String(status);
}
