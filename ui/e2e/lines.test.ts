import { describe, expect, it } from 'vitest';
import { completeLines } from './lines';

describe('completeLines', () => {
  it('leaves a line that is still being written for the next look', () => {
    const whole = '{"argv":["a"]}\n';
    const half = '{"argv":["b"';
    expect(completeLines(whole + half)).toEqual(['{"argv":["a"]}']);
    // Each line it gives is one the writer finished: parsing them never fails on a half line.
    expect(() => completeLines(whole + half).map((l) => JSON.parse(l) as unknown)).not.toThrow();
    expect(completeLines(`${whole}${half}]}\n`)).toEqual(['{"argv":["a"]}', '{"argv":["b"]}']);
  });

  it('gives nothing for an empty log or one whose first line is not finished', () => {
    expect(completeLines('')).toEqual([]);
    expect(completeLines('{"argv"')).toEqual([]);
  });

  it('skips empty lines and the carriage return of a line', () => {
    expect(completeLines('x\r\n\ny\n')).toEqual(['x', 'y']);
  });
});
