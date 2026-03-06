function isChecked(srcNode) {
  if (!srcNode || !Array.isArray(srcNode.attrs)) return false;
  for (let i = 0; i < srcNode.attrs.length; i++) {
    const a = srcNode.attrs[i];
    if (!a) continue;
    if (String(a.name || '').toLowerCase() === 'checked') return true;
  }
  return false;
}

export function renderRadioWidget(rect, ctx) {
  if (!rect || String(rect.tag || '') !== 'radio') return [];
  if (!ctx || ctx.mode !== 'collect') return [];

  const x = Math.round(Number(rect.x || 0));
  const y = Math.round(Number(rect.y || 0));
  const size = Math.max(8, Math.min(14, Math.round(Math.min(Number(rect.w || 0), Number(rect.h || 0)))));
  const depth = Math.max(0, Number(rect.depth || 0) + 1);
  const cx = x + Math.floor(size / 2);
  const cy = y + Math.floor(size / 2);

  const out = [];

  // Draw an approximated ring using 1px tiles in a small radius band.
  const outerR = Math.max(3, Math.floor(size / 2) - 1);
  const innerR = Math.max(1, outerR - 2);
  const outerR2 = outerR * outerR;
  const innerR2 = innerR * innerR;
  for (let py = -outerR; py <= outerR; py++) {
    for (let px = -outerR; px <= outerR; px++) {
      const d2 = px * px + py * py;
      if (d2 > outerR2 || d2 < innerR2) continue;
      out.push(cx + px, cy + py, 1, 1, depth, 0, 6);
    }
  }

  let checked = false;
  if (typeof ctx.getSourceNodeById === 'function') {
    const srcNode = ctx.getSourceNodeById(String(rect.id || ''));
    checked = isChecked(srcNode);
  }

  if (checked) {
    const dotR = Math.max(1, outerR - 3);
    const dotR2 = dotR * dotR;
    for (let py = -dotR; py <= dotR; py++) {
      for (let px = -dotR; px <= dotR; px++) {
        const d2 = px * px + py * py;
        if (d2 > dotR2) continue;
        out.push(cx + px, cy + py, 1, 1, depth, 0, 6);
      }
    }
  }

  return out;
}
