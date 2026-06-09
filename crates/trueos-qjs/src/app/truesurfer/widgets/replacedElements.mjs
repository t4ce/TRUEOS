export function replacedElementLayoutDefaults(attrs, kind = 'canvas') {
  const wAttr = Number(attrs?.width ?? 0);
  const hAttr = Number(attrs?.height ?? 0);
  const fallbackW = String(kind || '').toLowerCase() === 'img' ? 160 : 300;
  const fallbackH = String(kind || '').toLowerCase() === 'img' ? 120 : 150;
  const width = Number.isFinite(wAttr) && wAttr > 0 ? wAttr : fallbackW;
  const height = Number.isFinite(hAttr) && hAttr > 0 ? hAttr : fallbackH;
  return {
    width,
    height,
    minWidth: Math.min(String(kind || '').toLowerCase() === 'img' ? 80 : 120, width),
    minHeight: Math.min(String(kind || '').toLowerCase() === 'img' ? 60 : 80, height),
  };
}

export function replacedElementSceneStyle() {
  return {
    background: 0xffffff,
    border: 0x666666,
    muted: 0x666666,
    guide: 0xdddddd,
  };
}

