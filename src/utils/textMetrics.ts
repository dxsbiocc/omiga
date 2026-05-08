export function countTextLines(value: string): number {
  if (value.length === 0) return 1;
  let lines = 1;
  for (let i = 0; i < value.length; i += 1) {
    if (value.charCodeAt(i) === 10) lines += 1;
  }
  return lines;
}
