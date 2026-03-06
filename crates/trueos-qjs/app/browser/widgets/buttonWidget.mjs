export function renderButtonWidget(rect, ctx) {
  if (!rect || String(rect.tag || '') !== 'button') return [];
  if (!ctx || ctx.mode !== 'collect') return [];

  const x = Math.round(Number(rect.x || 0));
  const y = Math.round(Number(rect.y || 0));
  const w = Math.max(8, Math.round(Number(rect.w || 0)));
  const h = Math.max(8, Math.round(Number(rect.h || 0)));
  const depth = Math.max(0, Number(rect.depth || 0));

  // Custom layout style=5 applies a subtle diagonal two-value tint.
  return [x + 1, y + 1, Math.max(1, w - 2), Math.max(1, h - 2), depth + 1, 0, 5];
}
