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

function collapseInlineWhitespace(s) {
  return String(s || '').replace(/\s+/g, ' ').trim();
}

function collectNodeText(node, out) {
  if (!node) return;
  if (node.nodeName === '#text' && typeof node.value === 'string') {
    if (node.value) out.push(node.value);
    return;
  }
  if (!isElement(node)) return;
  const kids = Array.isArray(node.childNodes) ? node.childNodes : [];
  for (let i = 0; i < kids.length; i++) collectNodeText(kids[i], out);
}

function nodeText(node) {
  const parts = [];
  collectNodeText(node, parts);
  return collapseInlineWhitespace(parts.join(''));
}

function preferredColumnWidths(rows, cols) {
  const out = new Array(cols).fill(16);
  for (let r = 0; r < rows.length; r++) {
    const cells = tableCellsFromRow(rows[r]);
    for (let c = 0; c < cols; c++) {
      const cell = c < cells.length ? cells[c] : null;
      const txt = nodeText(cell);
      if (!txt) continue;
      out[c] = Math.max(out[c], txt.length * 8 + 10);
    }
  }
  return out;
}

function fitColumnWidths(totalW, preferred) {
  const cols = preferred.length;
  if (cols <= 0) return [];
  const safeW = Math.max(cols, Math.round(Number(totalW || cols)));
  const pref = preferred.map((w) => Math.max(1, Math.round(Number(w || 1))));
  const prefSum = pref.reduce((a, b) => a + b, 0) || cols;
  const widths = pref.map((w) => Math.max(1, Math.floor((safeW * w) / prefSum)));
  let used = widths.reduce((a, b) => a + b, 0);
  let rem = safeW - used;
  for (let i = 0; rem > 0 && i < cols; i++, rem--) widths[i % cols] += 1;
  return widths;
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
  const prefCols = preferredColumnWidths(rows, cols);
  const colWidths = fitColumnWidths(w, prefCols);
  const colStarts = new Array(cols + 1).fill(0);
  for (let c = 0; c < cols; c++) colStarts[c + 1] = colStarts[c] + colWidths[c];
  const rowStarts = new Array(rows.length + 1).fill(0);
  for (let r = 0; r < rows.length; r++) rowStarts[r + 1] = (r === rows.length - 1) ? h : (r + 1) * rowH;

  // Outer frame.
  out.push(x, y, w, h, depth + 1, 0, 0);

  for (let r = 0; r < rows.length; r++) {
    const cells = tableCellsFromRow(rows[r]);
    const cy = y + rowStarts[r];
    const ch = Math.max(1, rowStarts[r + 1] - rowStarts[r]);
    for (let c = 0; c < cols; c++) {
      const cx = x + colStarts[c];
      const cw = Math.max(1, colStarts[c + 1] - colStarts[c]);
      const cell = c < cells.length ? cells[c] : null;
      const tag = String(cell && (cell.tagName || cell.nodeName) || '').toLowerCase();
      if (tag === 'th') {
        out.push(cx + 1, cy + 1, Math.max(1, cw - 2), Math.max(1, ch - 2), depth + 1, -1, 0xFFE9EEF7);
      }
    }
  }

  // Internal grid lines: draw each shared separator once (1px), avoiding doubled borders.
  const lineColor = 0xFF000000;
  for (let c = 1; c < cols; c++) {
    const lx = x + colStarts[c];
    out.push(lx, y + 1, 1, Math.max(1, h - 2), depth + 2, -1, lineColor);
  }
  for (let r = 1; r < rows.length; r++) {
    const ly = y + rowStarts[r];
    out.push(x + 1, ly, Math.max(1, w - 2), 1, depth + 2, -1, lineColor);
  }

  return out;
}
