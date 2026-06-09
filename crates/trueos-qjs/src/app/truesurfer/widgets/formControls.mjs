export function inputLayoutDefaults(type = 'text') {
  const t = String(type || 'text').toLowerCase();
  if (t === 'checkbox' || t === 'radio') {
    return { width: 16, height: 16, minWidth: 16, minHeight: 16, marginRight: 6 };
  }
  return { width: 220, height: 36, minWidth: 220, minHeight: 36, paddingTop: 6, paddingBottom: 6 };
}

export function textareaLayoutDefaults() {
  return { width: 220, height: 108, minWidth: 220, minHeight: 108, paddingTop: 6, paddingBottom: 6 };
}

export function selectLayoutDefaults() {
  return { width: 220, height: 36, minWidth: 220, minHeight: 36 };
}

export function numberLayoutDefaults() {
  return { width: 140, height: 36, minWidth: 140, minHeight: 36 };
}

export function searchButtonLayoutDefaults() {
  return { width: 36, height: 36, minWidth: 36, minHeight: 36, marginRight: 6 };
}

export function controlSceneStyle() {
  return {
    background: 0xffffff,
    border: 0x666666,
    focusBorder: 0x3b82f6,
    accent: 0x3b82f6,
    muted: 0x666666,
    insetFill: 0xf7f7f7,
    radius: 0,
  };
}

export function temporalType(type = '') {
  const t = String(type || '').toLowerCase();
  return t === 'date' || t === 'time' || t === 'month' || t === 'week' || t === 'datetime-local';
}

export function selectedOptionLabel(attrs) {
  const options = String(attrs?.['data-options'] ?? '').split('\n').filter(Boolean);
  const idxRaw = Number(attrs?.['data-selected-index'] ?? 0);
  const idx = Number.isFinite(idxRaw) ? Math.max(0, Math.min(options.length - 1, idxRaw | 0)) : 0;
  return options[idx] ?? '';
}

