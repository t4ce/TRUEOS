export function renderFormWidget(rect, ctx) {
  if (!rect || String(rect.tag || '') !== 'form') return [];
  if (!ctx || ctx.mode !== 'collect') return [];
  // Keep forms visually neutral in this HTML-default renderer.
  return [];
}
