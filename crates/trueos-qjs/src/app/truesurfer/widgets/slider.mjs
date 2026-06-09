export function sliderLayoutDefaults() {
  return { width: 240, height: 14, minWidth: 240, minHeight: 14 };
}

export function sliderLabelLayoutDefaults() {
  return { width: 32, height: 24, minWidth: 32, minHeight: 24, marginRight: 6 };
}

export function sliderSceneStyle() {
  return {
    border: 0x666666,
    background: 0xffffff,
    fill: 0x3b82f6,
    indicator: 0x111111,
    innerPad: 3,
  };
}

export function sliderRatio(attrs) {
  const value = Number(attrs?.value ?? 0);
  return Number.isFinite(value) ? Math.max(0, Math.min(1, value)) : 0;
}

