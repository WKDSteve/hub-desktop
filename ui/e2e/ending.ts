// The end of a program a test started. A program ended by a signal (`kill()`) keeps `exitCode` at null and sets only
// `signalCode`, so "exitCode !== null" never sees that end, and an `exit` listener added after the event waits forever.
// `ending` listens from the start, so a stop after the end returns at once.

import type { ChildProcess } from 'node:child_process';

export interface Ending {
  /** Resolves once the program has ended, however it ended, also when it ended before this was awaited. */
  ended: Promise<void>;
  /** Whether the program has ended (by its own exit or by a signal). */
  done(): boolean;
}

/** Call right after `spawn`, before anything can end the program. */
export function ending(child: ChildProcess): Ending {
  let over = child.exitCode !== null || child.signalCode !== null;
  const ended = over
    ? Promise.resolve()
    : new Promise<void>((r) =>
        child.once('exit', () => {
          over = true;
          r();
        }),
      );
  return { ended, done: () => over };
}

/** End the program (if it still runs) and wait for its end. */
export async function stopProgram(child: ChildProcess, end: Ending): Promise<void> {
  if (!end.done()) child.kill();
  await end.ended;
}
