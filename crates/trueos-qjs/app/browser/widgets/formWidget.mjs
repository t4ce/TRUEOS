export function renderFormWidget(rect, ctx) {
  if (!rect || String(rect.tag || '') !== 'form') return [];
  if (!ctx || ctx.mode !== 'collect') return [];

  const x = Math.round(Number(rect.x || 0));
  const y = Math.round(Number(rect.y || 0));
  const w = Math.max(4, Math.round(Number(rect.w || 0)));
  const h = Math.max(4, Math.round(Number(rect.h || 0)));
  const depth = Math.max(0, Number(rect.depth || 0));

  // Gentle surface tint to separate form groups from plain flow content.
  return [
    x + 1, y + 1, Math.max(1, w - 2), Math.max(1, h - 2), depth, -1, 0x14DFE8F7,
    x, y, w, h, depth + 1, 0, 0,
  ];
}
