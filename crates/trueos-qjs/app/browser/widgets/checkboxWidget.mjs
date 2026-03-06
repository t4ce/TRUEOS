export function renderCheckboxWidget(rect, ctx) {
  if (!rect || String(rect.tag || '') !== 'checkbox') return [];
  if (!ctx || ctx.mode !== 'collect') return [];

  const x = Math.round(Number(rect.x || 0) - 2);
  const y = Math.round(Number(rect.y || 0));
  const size = Math.max(8, Math.min(14, Math.round(Math.min(Number(rect.w || 0), Number(rect.h || 0)))));
  const depth = Math.max(0, Number(rect.depth || 0) + 1);
  // Small fill inside the base border so the checkbox is clearly visible.
  return [x + 2, y + 2, Math.max(1, size - 4), Math.max(1, size - 4), depth, 0, 2];
}
