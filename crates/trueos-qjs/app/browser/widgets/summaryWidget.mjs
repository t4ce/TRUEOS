export function renderSummaryWidget(rect, ctx) {
  if (!rect || String(rect.tag || '') !== 'summary') return [];
  if (!ctx || ctx.mode !== 'collect') return [];

  const x = Math.round(Number(rect.x || 0));
  const y = Math.round(Number(rect.y || 0));
  const size = Math.max(8, Math.min(14, Math.round(Math.min(Number(rect.h || 0), 14))));
  const depth = Math.max(0, Number(rect.depth || 0) + 1);

  // Tiny disclosure box left to summary text (iconless for now).
  return [x + 2, y + 2, Math.max(1, size - 4), Math.max(1, size - 4), depth, 0, 2];
}
