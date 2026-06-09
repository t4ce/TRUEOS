export function colorLayoutDefaults(attrs) {
  const wAttr = Number(attrs?.width ?? 0);
  const hAttr = Number(attrs?.height ?? 0);
  const width = Number.isFinite(wAttr) && wAttr > 0 ? wAttr : 240;
  const height = Number.isFinite(hAttr) && hAttr > 0 ? hAttr : 200;
  return { width, height, minWidth: Math.min(240, width), minHeight: Math.min(200, height) };
}

export function colorSceneStyle() {
  return {
    border: 0x666666,
    background: 0xffffff,
    swatch: 0xff0000,
    rail: 0x111111,
    railLight: 0xffffff,
  };
}

