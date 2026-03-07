function isChecked(srcNode) {
  if (!srcNode || !Array.isArray(srcNode.attrs)) return false;
  for (let i = 0; i < srcNode.attrs.length; i++) {
    const a = srcNode.attrs[i];
    if (!a) continue;
    if (String(a.name || '').toLowerCase() === 'checked') return true;
  }
  return false;
}

export function renderCheckboxWidget(rect, ctx) {
  if (!rect || String(rect.tag || '') !== 'checkbox') return [];
  if (!ctx || ctx.mode !== 'collect') return [];

  const x = Math.round(Number(rect.x || 0) - 2);
  const y = Math.round(Number(rect.y || 0));
  const size = Math.max(16, Math.round(Math.min(Number(rect.w || 0), Number(rect.h || 0))));
  const depth = Math.max(0, Number(rect.depth || 0) + 1);
  const out = [x + 2, y + 2, Math.max(1, size - 4), Math.max(1, size - 4), depth, 0, 2];

  let checked = false;
  if (typeof ctx.getSourceNodeById === 'function') {
    const srcNode = ctx.getSourceNodeById(String(rect.id || ''));
    checked = isChecked(srcNode);
  }
  if (!checked) return out;

  const cx = x + Math.floor(size * 0.30);
  const cy = y + Math.floor(size * 0.55);
  const mx = x + Math.floor(size * 0.45);
  const my = y + Math.floor(size * 0.72);
  const ex = x + Math.floor(size * 0.78);
  const ey = y + Math.floor(size * 0.30);

  const pts = [
    [cx, cy], [mx, my], [ex, ey],
  ];
  for (let i = 0; i + 1 < pts.length; i++) {
    const [x0, y0] = pts[i];
    const [x1, y1] = pts[i + 1];
    const dx = Math.abs(x1 - x0);
    const dy = Math.abs(y1 - y0);
    const steps = Math.max(1, dx > dy ? dx : dy);
    for (let s = 0; s <= steps; s++) {
      const t = s / steps;
      const px = Math.round(x0 + (x1 - x0) * t);
      const py = Math.round(y0 + (y1 - y0) * t);
      out.push(px, py, 1, 1, depth + 1, 0, 6);
    }
  }
  return out;
}
