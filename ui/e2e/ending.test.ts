import { spawn } from 'node:child_process';
import { describe, expect, it } from 'vitest';
import { ending, stopProgram } from './ending';

const forever = () => spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], { stdio: 'ignore' });

describe('ending', () => {
  it('a stop after the program was killed and has ended returns at once', async () => {
    const child = forever();
    const end = ending(child);
    child.kill();
    await end.ended;
    // Ended by a signal: exitCode stays null, the case the old check waited forever on.
    expect(child.exitCode).toBeNull();
    const t0 = Date.now();
    await stopProgram(child, end);
    expect(Date.now() - t0).toBeLessThan(1000);
  });

  it('a stop of a running program ends it', async () => {
    const child = forever();
    const end = ending(child);
    expect(end.done()).toBe(false);
    await stopProgram(child, end);
    expect(end.done()).toBe(true);
  });
});
