const TEMPORAL_TAGS = new Set(['timeinput', 'dateinput', 'monthinput', 'weekinput', 'datetimelocalinput']);

function clampInt(n, lo, hi) {
  const x = Number.isFinite(Number(n)) ? (Number(n) | 0) : 0;
  return Math.max(lo, Math.min(hi, x));
}

function pad2(n) {
  const x = clampInt(n, 0, 99);
  return x < 10 ? `0${x}` : String(x);
}

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

function parseYear2FromYYYY(yyyy) {
  if (!/^\d{4}$/.test(String(yyyy || ''))) return null;
  const y = Number(yyyy);
  if (!Number.isFinite(y)) return null;
  const y2 = y - 2000;
  if (y2 < 0 || y2 > 99) return null;
  return y2;
}

function parseTimeValue(v) {
  const m = /^\s*(\d{2}):(\d{2})(?::(\d{2}))?\s*$/.exec(String(v || ''));
  if (!m) return null;
  const h = clampInt(Number(m[1]), 0, 23);
  const mm = clampInt(Number(m[2]), 0, 59);
  const s = m[3] != null ? clampInt(Number(m[3]), 0, 59) : 0;
  return { hour: h, minute: mm, second: s };
}

function parseMonthValue(v) {
  const m = /^\s*(\d{4})-(\d{2})\s*$/.exec(String(v || ''));
  if (!m) return null;
  const y2 = parseYear2FromYYYY(m[1]);
  const mo = clampInt(Number(m[2]), 1, 12);
  if (y2 == null) return null;
  return { year2: y2, month: mo };
}

function parseDateValue(v) {
  const m = /^\s*(\d{4})-(\d{2})-(\d{2})\s*$/.exec(String(v || ''));
  if (!m) return null;
  const y2 = parseYear2FromYYYY(m[1]);
  const mo = clampInt(Number(m[2]), 1, 12);
  const dd = clampInt(Number(m[3]), 1, 31);
  if (y2 == null) return null;
  const wi = clampInt(Math.floor((dd - 1) / 7) + 1, 1, 4);
  return { year2: y2, month: mo, weekIndex: wi };
}

function parseWeekValue(v) {
  const m = /^\s*(\d{4})-W(\d{2})\s*$/.exec(String(v || ''));
  if (!m) return null;
  const y2 = parseYear2FromYYYY(m[1]);
  const w = clampInt(Number(m[2]), 1, 48);
  if (y2 == null) return null;
  const month = clampInt(Math.floor((w - 1) / 4) + 1, 1, 12);
  const weekIndex = clampInt(((w - 1) % 4) + 1, 1, 4);
  return { year2: y2, month, weekIndex };
}

function parseDateTimeLocalValue(v) {
  const m = /^\s*(\d{4})-(\d{2})-(\d{2})[T\s](\d{2}):(\d{2})(?::(\d{2}))?\s*$/.exec(String(v || ''));
  if (!m) return null;
  const y2 = parseYear2FromYYYY(m[1]);
  const mo = clampInt(Number(m[2]), 1, 12);
  const dd = clampInt(Number(m[3]), 1, 31);
  const hh = clampInt(Number(m[4]), 0, 23);
  const mm = clampInt(Number(m[5]), 0, 59);
  const ss = m[6] != null ? clampInt(Number(m[6]), 0, 59) : 0;
  if (y2 == null) return null;
  const wi = clampInt(Math.floor((dd - 1) / 7) + 1, 1, 4);
  return { year2: y2, month: mo, weekIndex: wi, hour: hh, minute: mm, second: ss };
}

function formatMonthValue(st) {
  return `20${pad2(st.year2)}-${pad2(st.month)}`;
}

function formatWeekValue(st) {
  const weekNo = (clampInt(st.month, 1, 12) - 1) * 4 + clampInt(st.weekIndex, 1, 4);
  return `20${pad2(st.year2)}-W${pad2(weekNo)}`;
}

function formatDateValue(st) {
  const day = (clampInt(st.weekIndex, 1, 4) - 1) * 7 + 1;
  return `20${pad2(st.year2)}-${pad2(st.month)}-${pad2(day)}`;
}

function formatTimeValue(st) {
  return `${pad2(st.hour)}:${pad2(st.minute)}:${pad2(st.second)}`;
}

function formatDateTimeLocalValue(st) {
  return `${formatDateValue(st)}T${formatTimeValue(st)}`;
}

function nowState() {
  const d = new Date();
  return {
    year2: clampInt(d.getFullYear() - 2000, 0, 99),
    month: clampInt(d.getMonth() + 1, 1, 12),
    weekIndex: 1,
    hour: clampInt(d.getHours(), 0, 23),
    minute: clampInt(d.getMinutes(), 0, 59),
    second: clampInt(d.getSeconds(), 0, 59),
  };
}

export function temporalTagForInputType(typeAttr) {
  const t = String(typeAttr || '').toLowerCase();
  if (t === 'time') return 'timeinput';
  if (t === 'date') return 'dateinput';
  if (t === 'month') return 'monthinput';
  if (t === 'week') return 'weekinput';
  if (t === 'datetime-local') return 'datetimelocalinput';
  return '';
}

export function isTemporalTag(tag) {
  return TEMPORAL_TAGS.has(String(tag || '').toLowerCase());
}

export function temporalDisplayText(tag, srcNode) {
  const t = String(tag || '').toLowerCase();
  if (!isTemporalTag(t)) return '';

  const raw = String(getAttr(srcNode, 'value') || '');
  if (raw.trim().length > 0) return raw;

  const st = nowState();
  if (t === 'timeinput') return formatTimeValue(st);
  if (t === 'monthinput') return formatMonthValue(st);
  if (t === 'weekinput') return formatWeekValue(st);
  if (t === 'dateinput') return formatDateValue(st);
  return formatDateTimeLocalValue(st);
}

export function applyYogaDefaultsTemporalInput(yogaNode, Yoga, tag, srcNode) {
  if (!yogaNode || !Yoga || !isTemporalTag(tag)) return;
  yogaNode.setPadding(Yoga.EDGE_LEFT, 0);
  yogaNode.setPadding(Yoga.EDGE_RIGHT, 0);
  yogaNode.setPadding(Yoga.EDGE_TOP, 0);
  yogaNode.setPadding(Yoga.EDGE_BOTTOM, 0);

  const t = String(tag || '').toLowerCase();
  const display = temporalDisplayText(t, srcNode);
  const minW = t === 'datetimelocalinput'
    ? Math.max(220, display.length * 8 + 20)
    : Math.max(140, display.length * 8 + 18);
  const minH = 24;

  if (typeof yogaNode.setAlignSelf === 'function') yogaNode.setAlignSelf(Yoga.ALIGN_FLEX_START);
  if (typeof yogaNode.setWidth === 'function') yogaNode.setWidth(minW);
  if (typeof yogaNode.setMinWidth === 'function') yogaNode.setMinWidth(minW);
  if (typeof yogaNode.setHeight === 'function') yogaNode.setHeight(minH);
  if (typeof yogaNode.setMinHeight === 'function') yogaNode.setMinHeight(minH);
}

export function renderTemporalWidget(rect, ctx) {
  if (!rect || !isTemporalTag(rect.tag)) return [];
  if (!ctx || ctx.mode !== 'collect') return [];

  const x = Math.round(Number(rect.x || 0));
  const y = Math.round(Number(rect.y || 0));
  const w = Math.max(8, Math.round(Number(rect.w || 0)));
  const h = Math.max(8, Math.round(Number(rect.h || 0)));
  const depth = Math.max(0, Number(rect.depth || 0));

  const chevronW = Math.max(10, Math.min(18, Math.round(w * 0.18)));
  const mainW = Math.max(1, w - chevronW - 2);

  // Custom commands: subtle control fill + right-side picker lane.
  return [
    x + 1, y + 1, Math.max(1, w - 2), Math.max(1, h - 2), depth + 1, 0, 5,
    x + 1 + mainW, y + 1, chevronW, Math.max(1, h - 2), depth + 2, 0, 2,
  ];
}

export function parseTemporalValue(tag, valueText) {
  const t = String(tag || '').toLowerCase();
  if (t === 'timeinput') return parseTimeValue(valueText);
  if (t === 'monthinput') return parseMonthValue(valueText);
  if (t === 'weekinput') return parseWeekValue(valueText);
  if (t === 'dateinput') return parseDateValue(valueText);
  if (t === 'datetimelocalinput') return parseDateTimeLocalValue(valueText);
  return null;
}
