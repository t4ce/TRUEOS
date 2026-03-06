const iconCmdCache = new Map();

function readIconCmds(kind) {
  if (iconCmdCache.has(kind)) return iconCmdCache.get(kind);
  const fn = globalThis.__trueosReadWindowSvgCmds;
  if (typeof fn !== 'function') {
    iconCmdCache.set(kind, null);
    return null;
  }
  let raw = null;
  try {
    raw = fn(kind);
  } catch (_) {
    raw = null;
  }
  if (!Array.isArray(raw) || raw.length < 3) {
    iconCmdCache.set(kind, null);
    return null;
  }

  const w = Math.max(1, Number(raw[0] || 32));
  const h = Math.max(1, Number(raw[1] || 32));
  const cmdCount = Math.max(0, Number(raw[2] || 0) | 0);
  const cmds = [];
  let p = 3;
  for (let i = 0; i < cmdCount; i++) {
    if (p + 5 >= raw.length) break;
    cmds.push({
      x0: Number(raw[p + 0] || 0),
      y0: Number(raw[p + 1] || 0),
      x1: Number(raw[p + 2] || 0),
      y1: Number(raw[p + 3] || 0),
      thick: Math.max(1, Number(raw[p + 4] || 1) | 0),
    });
    p += 6;
  }
  const out = { w, h, cmds };
  iconCmdCache.set(kind, out);
  return out;
}

function paintIconPixels(out, icon, x, y, size, depth) {
  if (!icon || !Array.isArray(icon.cmds) || icon.cmds.length === 0) return;
  const pxSet = new Set();
  const maxX = Math.max(1, icon.w - 1);
  const maxY = Math.max(1, icon.h - 1);
  for (let i = 0; i < icon.cmds.length; i++) {
    const c = icon.cmds[i];
    const x0 = c.x0 * maxX;
    const y0 = c.y0 * maxY;
    const x1 = c.x1 * maxX;
    const y1 = c.y1 * maxY;
    const dx = Math.abs(x1 - x0);
    const dy = Math.abs(y1 - y0);
    const steps = Math.max(1, (dx > dy ? dx : dy) | 0);
    const radius = Math.max(0, ((c.thick | 0) - 1) >> 1);
    for (let s = 0; s <= steps; s++) {
      const t = steps <= 0 ? 0 : (s / steps);
      const cx = Math.round(x0 + (x1 - x0) * t);
      const cy = Math.round(y0 + (y1 - y0) * t);
      for (let oy = -radius; oy <= radius; oy++) {
        for (let ox = -radius; ox <= radius; ox++) {
          const px = cx + ox;
          const py = cy + oy;
          if (px < 0 || py < 0 || px > maxX || py > maxY) continue;
          pxSet.add(`${px},${py}`);
        }
      }
    }
  }

  const scaleX = size / icon.w;
  const scaleY = size / icon.h;
  for (const k of pxSet) {
    const comma = k.indexOf(',');
    const px = Number(k.slice(0, comma));
    const py = Number(k.slice(comma + 1));
    const rx = Math.round(x + px * scaleX);
    const ry = Math.round(y + py * scaleY);
    const rw = Math.max(1, Math.ceil(scaleX));
    const rh = Math.max(1, Math.ceil(scaleY));
    out.push(rx, ry, rw, rh, depth, 0, 6);
  }
}

export function renderHtmlAppWindowWidget(rect, ctx) {
  if (!rect || String(rect.tag || '') !== 'html_app_window') return [];
  if (!ctx || ctx.mode !== 'collect') return [];

  const x = Math.round(Number(rect.x || 0));
  const y = Math.round(Number(rect.y || 0));
  const w = Math.max(16, Math.round(Number(rect.w || 0)));
  const h = Math.max(16, Math.round(Number(rect.h || 0)));
  const depth = Math.max(0, Number(rect.depth || 0));

  const btnSize = 12;
  const gap = 2;
  const margin = 2;
  const topY = y + margin;
  const totalW = btnSize * 3 + gap * 2;
  const startX = x + Math.max(0, w - totalW - margin);
  if (h < btnSize + margin * 2) return [];

  const out = [];
  for (let i = 0; i < 3; i++) {
    const bx = startX + i * (btnSize + gap);
    // Button surface tint.
    out.push(bx, topY, btnSize, btnSize, depth + 2, 0, 5);
    // Icon pixels from svg.rs commands.
    const icon = readIconCmds(i);
    paintIconPixels(out, icon, bx + 1, topY + 1, btnSize - 2, depth + 3);
  }
  return out;
}
