function getAttr(node, name) {
  if (!node || !Array.isArray(node.attrs)) return '';
  const n = String(name || '').toLowerCase();
  for (let i = 0; i < node.attrs.length; i++) {
    const a = node.attrs[i];
    if (!a || String(a.name || '').toLowerCase() !== n) continue;
    return String(a.value || '');
  }
  return '';
}

function isElement(node) {
  return !!node && typeof node === 'object' && typeof node.nodeName === 'string' && Array.isArray(node.childNodes);
}

function isTextNode(node) {
  return !!node && typeof node === 'object' && node.nodeName === '#text' && typeof node.value === 'string';
}

function collapseInlineWhitespace(s) {
  return String(s || '').replace(/\s+/g, ' ').trim();
}

function collectNodeText(node, out) {
  if (!node) return;
  if (isTextNode(node)) {
    out.push(node.value || '');
    return;
  }
  if (!isElement(node)) return;
  const kids = Array.isArray(node.childNodes) ? node.childNodes : [];
  for (let i = 0; i < kids.length; i++) collectNodeText(kids[i], out);
}

function optionText(optionNode) {
  const parts = [];
  collectNodeText(optionNode, parts);
  return collapseInlineWhitespace(parts.join(''));
}

function hasSelectedAttr(optionNode) {
  if (!optionNode || !Array.isArray(optionNode.attrs)) return false;
  for (let i = 0; i < optionNode.attrs.length; i++) {
    const a = optionNode.attrs[i];
    if (!a) continue;
    if (String(a.name || '').toLowerCase() === 'selected') return true;
  }
  return false;
}

export function selectOptionTexts(selectNode) {
  if (!selectNode || !Array.isArray(selectNode.childNodes)) return [];
  const out = [];
  const kids = selectNode.childNodes;
  for (let i = 0; i < kids.length; i++) {
    const k = kids[i];
    if (!isElement(k)) continue;
    const tag = String(k.tagName || k.nodeName || '').toLowerCase();
    if (tag !== 'option') continue;
    const txt = optionText(k);
    out.push(txt || `Option ${out.length + 1}`);
  }
  return out;
}

export function selectSelectedIndex(selectNode) {
  if (!selectNode || !Array.isArray(selectNode.childNodes)) return 0;
  const kids = selectNode.childNodes;
  let first = -1;
  let idx = 0;
  for (let i = 0; i < kids.length; i++) {
    const k = kids[i];
    if (!isElement(k)) continue;
    const tag = String(k.tagName || k.nodeName || '').toLowerCase();
    if (tag !== 'option') continue;
    if (first < 0) first = idx;
    if (hasSelectedAttr(k)) return idx;
    idx += 1;
  }
  return first >= 0 ? first : 0;
}

export function selectDisplayText(selectNode) {
  if (!selectNode || !Array.isArray(selectNode.childNodes)) return 'Select';
  const kids = selectNode.childNodes;
  let firstOption = null;
  for (let i = 0; i < kids.length; i++) {
    const k = kids[i];
    if (!isElement(k)) continue;
    const tag = String(k.tagName || k.nodeName || '').toLowerCase();
    if (tag !== 'option') continue;
    if (!firstOption) firstOption = k;
    if (!hasSelectedAttr(k)) continue;
    const txt = optionText(k);
    if (txt) return txt;
  }
  if (firstOption) {
    const txt = optionText(firstOption);
    if (txt) return txt;
  }
  const title = collapseInlineWhitespace(getAttr(selectNode, 'title'));
  if (title) return title;
  return 'Select';
}

function readIconCmds(kind) {
  const fn = globalThis.__trueosReadWindowSvgCmds;
  if (typeof fn !== 'function') return null;
  let raw = null;
  try {
    raw = fn(kind);
  } catch (_) {
    raw = null;
  }
  if (!Array.isArray(raw) || raw.length < 3) return null;

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
  return { w, h, cmds };
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

function paintFallbackRadio(out, x, y, size, depth, selected) {
  const s = Math.max(8, Number(size || 16) | 0);
  const cx = x + Math.floor(s / 2);
  const cy = y + Math.floor(s / 2);
  const outer = Math.max(3, Math.floor(s / 2) - 1);
  const inner = Math.max(1, outer - 3);
  const outer2 = outer * outer;
  const inner2 = inner * inner;

  for (let oy = -outer; oy <= outer; oy++) {
    for (let ox = -outer; ox <= outer; ox++) {
      const d2 = ox * ox + oy * oy;
      if (d2 > outer2 || d2 < inner2) continue;
      out.push(cx + ox, cy + oy, 1, 1, depth, 0, 6);
    }
  }

  if (!selected) return;

  const dot = Math.max(1, outer - 5);
  const dot2 = dot * dot;
  for (let oy = -dot; oy <= dot; oy++) {
    for (let ox = -dot; ox <= dot; ox++) {
      const d2 = ox * ox + oy * oy;
      if (d2 > dot2) continue;
      out.push(cx + ox, cy + oy, 1, 1, depth, 0, 6);
    }
  }
}

export function applyYogaDefaultsSelectWidget(yogaNode, Yoga, tag, srcNode) {
  if (!yogaNode || !Yoga || String(tag || '').toLowerCase() !== 'select') return;
  yogaNode.setPadding(Yoga.EDGE_LEFT, 0);
  yogaNode.setPadding(Yoga.EDGE_RIGHT, 0);
  yogaNode.setPadding(Yoga.EDGE_TOP, 0);
  yogaNode.setPadding(Yoga.EDGE_BOTTOM, 0);

  const label = selectDisplayText(srcNode);
  const options = selectOptionTexts(srcNode);
  let longest = Math.max(0, label.length);
  for (let i = 0; i < options.length; i++) longest = Math.max(longest, String(options[i] || '').length);
  const targetW = Math.max(84, longest * 8 + 30);
  // Reserve room for 1px border + icon/text breathing space.
  const headerH = 18;
  const rowH = 16;
  const rows = Math.max(1, options.length);
  const targetH = headerH + rows * rowH;

  if (typeof yogaNode.setAlignSelf === 'function') yogaNode.setAlignSelf(Yoga.ALIGN_FLEX_START);
  if (typeof yogaNode.setMeasureFunc === 'function') {
    yogaNode.setMeasureFunc(() => ({ width: targetW, height: targetH }));
  }
  if (typeof yogaNode.setWidth === 'function') yogaNode.setWidth(targetW);
  if (typeof yogaNode.setMinWidth === 'function') yogaNode.setMinWidth(targetW);
  if (typeof yogaNode.setHeight === 'function') yogaNode.setHeight(targetH);
  if (typeof yogaNode.setMinHeight === 'function') yogaNode.setMinHeight(targetH);
}

export function renderSelectWidget(rect, ctx) {
  if (!rect || String(rect.tag || '').toLowerCase() !== 'select') return [];
  if (!ctx || ctx.mode !== 'collect') return [];

  const x = Math.round(Number(rect.x || 0));
  const y = Math.round(Number(rect.y || 0));
  const w = Math.max(1, Math.round(Number(rect.w || 0)));
  const h = Math.max(8, Math.round(Number(rect.h || 0)));
  const size = 16;
  const headerH = 18;
  const rowH = 16;
  const depth = Math.max(0, Number(rect.depth || 0) + 1);

  const out = [];
  let options = [];
  let selectedIndex = 0;
  if (typeof ctx.getSourceNodeById === 'function') {
    const srcNode = ctx.getSourceNodeById(String(rect.id || ''));
    options = selectOptionTexts(srcNode);
    selectedIndex = Math.max(0, selectSelectedIndex(srcNode));
  }

  const rows = Math.max(1, options.length);
  const listY = y + headerH;
  const listH = Math.max(0, h - headerH);

  if (listH > 0) {
    out.push(x, listY, w, listH, depth, 0, 0);
    const fillRow = Math.max(0, Math.min(rows - 1, selectedIndex));
    const fillY = listY + fillRow * rowH;
    const fillH = Math.max(1, Math.min(rowH, listY + listH - fillY));
    if (fillY < listY + listH) out.push(x + 1, fillY + 1, Math.max(1, w - 2), Math.max(1, fillH - 1), depth, 0, 2);
    for (let i = 1; i < rows; i++) {
      const ry = listY + i * rowH;
      if (ry >= listY + listH) break;
      out.push(x + 1, ry, Math.max(1, w - 2), 1, depth, 0, 6);
    }
  }

  // Kind id 7 is the selected radio-circle icon emitted from svg.rs.
  const iconSelected = readIconCmds(7);
  for (let i = 0; i < rows; i++) {
    const ry = listY + i * rowH;
    if (ry >= listY + listH) break;
    const iconX = x + 1;
    const iconY = ry + Math.max(0, Math.round((Math.min(rowH, h) - size) / 2));
    const isSelected = i === Math.max(0, Math.min(rows - 1, selectedIndex));
    if (isSelected && iconSelected && Array.isArray(iconSelected.cmds) && iconSelected.cmds.length > 0) {
      paintIconPixels(out, iconSelected, iconX, iconY, size, depth);
    } else {
      paintFallbackRadio(out, iconX, iconY, size, depth, isSelected);
    }
  }
  return out;
}
