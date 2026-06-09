export function progressMeterLayoutDefaults() {
  return { width: 240, height: 14, minWidth: 240, minHeight: 14 };
}

export function progressMeterSceneStyle() {
  return {
    border: 0x666666,
    background: 0xffffff,
    fill: 0x3b82f6,
    innerPad: 3,
  };
}

export function progressMeterRatio(attrs, tagName = 'progress') {
  const maxDefault = String(tagName || '').toLowerCase() === 'meter' ? 1 : 100;
  const max = Number(attrs?.max ?? maxDefault);
  const value = Number(attrs?.value ?? 0);
  if (!Number.isFinite(max) || max <= 0 || !Number.isFinite(value)) return 0;
  return Math.max(0, Math.min(1, value / max));
}

