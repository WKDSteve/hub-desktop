// The lines of a log that another process appends to while a test reads it (fake-app.log, browser.log). A line counts
// once its newline is written: a line that is half written when the file is read is left for the next look, instead of
// being parsed half (`Unexpected end of JSON input`).

export function completeLines(text: string): string[] {
  const end = text.lastIndexOf('\n');
  if (end < 0) return [];
  return text
    .slice(0, end)
    .split('\n')
    .map((l) => l.replace(/\r$/, ''))
    .filter((l) => l !== '');
}
