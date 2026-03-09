const LEFT_PAD = 8;
const LINE_H = 16;
const INDENT_PX = 12;

export function renderScene(doc, vw, vh, scrollY, overlayRuns, drawLayoutRects) {
  if (typeof drawLayoutRects !== 'function') return false;

  const runs = [];
  const rows = Array.isArray(doc && doc.rows) ? doc.rows : [];
  const rowY = Array.isArray(doc && doc.rowY) ? doc.rowY : [];

  for (let i = 0; i < rows.length; i++) {
    const row = rows[i];
    const x = LEFT_PAD + (Math.max(0, Number(row && row.depth || 0) | 0) * INDENT_PX);
    const y = Math.round(Number(rowY[i] || 0) - Number(scrollY || 0));
    if (y < -LINE_H) continue;
    const text = String(row && row.text || '');
    if (!text) continue;
    runs.push(x, y, text);
  }

  if (Array.isArray(overlayRuns) && overlayRuns.length > 0) {
    for (let i = 0; i < overlayRuns.length; i++) {
      runs.push(overlayRuns[i]);
    }
  }

  drawLayoutRects(doc || null, vw, vh, runs, scrollY);
  return true;
}
