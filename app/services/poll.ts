/**
 * Runs `step` again `pauseMs` after the previous run has settled, so a slow server never has two of the same
 * request in flight and the window's connections stay free for what the person does. Returns the stop.
 *
 * `step` handles its own errors; a rejection still ends that run and the next one follows after the pause.
 */
export function poll(step: () => Promise<unknown>, pauseMs: number): () => void {
  let stopped = false;
  let timer: number | undefined;
  const run = async () => {
    try {
      await step();
    } finally {
      if (!stopped) timer = window.setTimeout(() => void run(), pauseMs);
    }
  };
  void run();
  return () => {
    stopped = true;
    if (timer !== undefined) window.clearTimeout(timer);
  };
}
