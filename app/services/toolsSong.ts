/** A run of a tool as the service reports it: the song it works on and whether it has ended. */
export interface ToolRun {
  song_id: string | null;
  done: boolean;
}

/**
 * The song the Tools tab opens on when nothing chose one: the song a stems or MIDI run is still working on, so its
 * progress is in view at once, else the first song of the library.
 */
export function toolsSongOnOpen(playable: string[], runs: (ToolRun | null | undefined)[]): string {
  const working = runs.find(run => run && !run.done && run.song_id && playable.includes(run.song_id));
  return working?.song_id ?? playable[0] ?? '';
}
