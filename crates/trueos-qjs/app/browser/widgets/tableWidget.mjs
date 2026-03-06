function isElement(node) {
  return !!node && typeof node === 'object' && typeof node.nodeName === 'string' && Array.isArray(node.childNodes);
}

function tableRowsFromNode(tableNode) {
  if (!tableNode || !Array.isArray(tableNode.childNodes)) return [];
  const rows = [];
  const kids = tableNode.childNodes;
  for (let i = 0; i < kids.length; i++) {
    const n = kids[i];
    if (!isElement(n)) continue;
    const tag = String(n.tagName || n.nodeName || '').toLowerCase();
    if (tag === 'tr') {
      rows.push(n);
      continue;
    }
    if (tag !== 'thead' && tag !== 'tbody' && tag !== 'tfoot') continue;
    const secKids = Array.isArray(n.childNodes) ? n.childNodes : [];
    for (let j = 0; j < secKids.length; j++) {
      const r = secKids[j];
      if (!isElement(r)) continue;
      if (String(r.tagName || r.nodeName || '').toLowerCase() === 'tr') rows.push(r);
    }
  }
  return rows;
}

function tableCellsFromRow(rowNode) {
  if (!rowNode || !Array.isArray(rowNode.childNodes)) return [];
  const cells = [];
  const kids = rowNode.childNodes;
  for (let i = 0; i < kids.length; i++) {
    const n = kids[i];
    if (!isElement(n)) continue;
    const tag = String(n.tagName || n.nodeName || '').toLowerCase();
    if (tag === 'th' || tag === 'td') cells.push(n);
  }
  return cells;
}

export function renderTableWidget(rect, ctx) {
  if (!rect || String(rect.tag || '') !== 'table') return [];
  if (!ctx || ctx.mode !== 'collect') return [];
  if (typeof ctx.getSourceNodeById !== 'function') return [];

  const srcNode = ctx.getSourceNodeById(String(rect.id || ''));
  const rows = tableRowsFromNode(srcNode);
  if (rows.length <= 0) return [];

  let cols = 0;
  for (let i = 0; i < rows.length; i++) cols = Math.max(cols, tableCellsFromRow(rows[i]).length);
  if (cols <= 0) return [];

  const x = Math.round(Number(rect.x || 0));
  const y = Math.round(Number(rect.y || 0));
  const w = Math.max(2, Math.round(Number(rect.w || 0)));
  const h = Math.max(2, Math.round(Number(rect.h || 0)));
  const depth = Math.max(0, Number(rect.depth || 0));

  const out = [];
  const rowH = Math.max(10, Math.floor(h / rows.length));
  const colW = Math.max(16, Math.floor(w / cols));

  // Outer frame.
  out.push(x, y, w, h, depth + 1, 0, 0);

  for (let r = 0; r < rows.length; r++) {
    const cells = tableCellsFromRow(rows[r]);
    const cy = y + r * rowH;
    const ch = (r === rows.length - 1) ? Math.max(1, (y + h) - cy) : rowH;

    for (let c = 0; c < cols; c++) {
      const cx = x + c * colW;
      const cw = (c === cols - 1) ? Math.max(1, (x + w) - cx) : colW;
      const cell = c < cells.length ? cells[c] : null;
      const tag = String(cell && (cell.tagName || cell.nodeName) || '').toLowerCase();

      // Header cells get a soft fill underneath the border.
      if (tag === 'th') {
        out.push(cx + 1, cy + 1, Math.max(1, cw - 2), Math.max(1, ch - 2), depth + 1, -1, 0xFFE9EEF7);
      }

      // Cell border.
      out.push(cx, cy, cw, ch, depth + 2, 0, 0);
    }
  }

  return out;
}
