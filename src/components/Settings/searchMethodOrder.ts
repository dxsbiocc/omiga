export function moveItemToIndex<T>(items: readonly T[], item: T, targetIndex: number): T[] {
  const fromIndex = items.indexOf(item);
  if (fromIndex < 0 || items.length === 0) return [...items];

  const boundedTarget = Math.min(Math.max(0, targetIndex), items.length - 1);
  if (fromIndex === boundedTarget) return [...items];

  const next = [...items];
  const [moved] = next.splice(fromIndex, 1);
  next.splice(boundedTarget, 0, moved);
  return next;
}
