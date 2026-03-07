function chipWidth(label) {
  const txt = String(label || '');
  return Math.max(36, Math.min(220, txt.length * 8 + 12));
}

export function layoutStatusbarItems(rect, items) {
  const x = Math.round(Number(rect && rect.x || 0));
  const y = Math.round(Number(rect && rect.y || 0));
  const w = Math.max(1, Math.round(Number(rect && rect.w || 0)));
  const baseH = Math.max(12, Number(globalThis.__trueosThemeNodeH || 16));
  const chipH = Math.max(10, baseH);
  const gapX = 4;
  const gapY = 2;
  const padX = 4;
  const padY = 2;

  let cx = x + padX;
  let cy = y + padY;
  let rowBottom = cy + chipH;

  const list = Array.isArray(items) ? items : [];
  const out = [];
  const right = x + w - padX;

  for (let i = 0; i < list.length; i++) {
    const it = list[i] || {};
    const label = String(it.label || '').trim();
    if (!label) continue;
    const cw = chipWidth(label);
    if (cx + cw > right && cx > x + padX) {
      cx = x + padX;
      cy = rowBottom + gapY;
      rowBottom = cy + chipH;
    }
    out.push({
      id: String(it.id || ''),
      label,
      x: cx,
      y: cy,
      w: Math.max(20, Math.min(cw, Math.max(20, right - cx))),
      h: chipH,
    });
    cx += cw + gapX;
  }

  const barH = Math.max(baseH + 4, rowBottom - y + padY);
  return { items: out, barH };
}

export function renderStatusbarWidget(rect, ctx) {
  if (!rect || String(rect.tag || '') !== 'statusbar') return [];
  if (!ctx || ctx.mode !== 'collect') return [];

  const x = Math.round(Number(rect.x || 0));
  const y = Math.round(Number(rect.y || 0));
  const w = Math.max(2, Math.round(Number(rect.w || 0)));
  const h = Math.max(2, Math.round(Number(rect.h || 0)));
  const depth = Math.max(0, Number(rect.depth || 0));
  const items = typeof ctx.getStatusbarItems === 'function' ? ctx.getStatusbarItems() : [];
  const chips = layoutStatusbarItems(rect, items);

  const out = [];
  // Border-only lane: visual separation should come from true layout bounds.
  out.push(x, y, w, h, depth + 1, 0, 1);

  if (!chips || !Array.isArray(chips.items)) return out;
  for (let i = 0; i < chips.items.length; i++) {
    const c = chips.items[i];
    out.push(c.x, c.y, c.w, c.h, depth + 2, 0, 5);
  }
  return out;
}
