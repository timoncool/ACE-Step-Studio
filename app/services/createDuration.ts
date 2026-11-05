/** A max-duration loaded from a request (a reused track, an example, the
 *  writing assistant) never drops below the duration the user keeps for every
 *  new song (studio.createDuration); a longer one is honoured (#184). When
 *  nothing is saved the loaded value stands, and an absent or non-finite value
 *  keeps what is saved. The value is the string the duration field holds ('' is
 *  left to the planner). */
export function clampDurationToKept(asked: number | undefined, kept: string): string {
  const keptSeconds = Number(kept);
  const saved = kept.trim() !== '' && Number.isFinite(keptSeconds);
  if (asked !== undefined && Number.isFinite(asked) && (!saved || asked > keptSeconds)) {
    return String(Math.round(asked));
  }
  return kept;
}
