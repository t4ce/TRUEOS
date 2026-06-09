export function temporalKindFromInputType(type = '') {
  const t = String(type || '').toLowerCase();
  if (t === 'time' || t === 'date' || t === 'month' || t === 'week' || t === 'datetime-local') {
    return t;
  }
  return '';
}

export function temporalKindFromTag(tagName = '') {
  const tag = String(tagName || '').toLowerCase();
  if (tag === 'timeinput') return 'time';
  if (tag === 'dateinput') return 'date';
  if (tag === 'monthinput') return 'month';
  if (tag === 'weekinput') return 'week';
  if (tag === 'datetimelocalinput') return 'datetime-local';
  return '';
}

export function temporalTagForInputType(type = '') {
  const kind = temporalKindFromInputType(type);
  if (kind === 'time') return 'timeinput';
  if (kind === 'date') return 'dateinput';
  if (kind === 'month') return 'monthinput';
  if (kind === 'week') return 'weekinput';
  if (kind === 'datetime-local') return 'datetimelocalinput';
  return '';
}

export function temporalLayoutDefaults(kind = '') {
  return {
    width: kind === 'datetime-local' ? 340 : 220,
    height: 36,
    minWidth: kind === 'datetime-local' ? 340 : 220,
    minHeight: 36,
  };
}

export function temporalSceneStyle() {
  return {
    background: 0xffffff,
    border: 0x666666,
    muted: 0x666666,
    insetFill: 0xf7f7f7,
    text: 0x111111,
    radius: 0,
  };
}

export function temporalDisplayValue(attrs, kind = '') {
  const raw = String(attrs?.value ?? '');
  if (raw.length > 0) return raw.replace('T', ' ');
  if (kind === 'time') return '00:00:00';
  if (kind === 'month') return '2026-01';
  if (kind === 'week') return '2026-W01';
  if (kind === 'date') return '2026-01-01';
  if (kind === 'datetime-local') return '2026-01-01 00:00:00';
  return '';
}

export function temporalDatePart(attrs) {
  const shown = temporalDisplayValue(attrs, 'datetime-local');
  const parts = shown.split(/[ T]/).filter(Boolean);
  return parts[0] || '2026-01-01';
}

export function temporalTimePart(attrs) {
  const shown = temporalDisplayValue(attrs, 'datetime-local');
  const parts = shown.split(/[ T]/).filter(Boolean);
  return parts[1] || '00:00:00';
}
