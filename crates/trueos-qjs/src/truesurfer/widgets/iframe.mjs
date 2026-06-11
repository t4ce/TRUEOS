export function isRootIframe(node) {
  return String(node?.attrs?.['data-root'] ?? '') === '1';
}

export function iframeLayoutDefaults(node) {
  if (isRootIframe(node)) {
    return {
      root: 1,
      paddingLeft: 0,
      paddingRight: 0,
      paddingTop: 0,
      paddingBottom: 0,
      flexGrow: 1,
      flexShrink: 1,
      minWidth: 0,
      minHeight: 0,
      chromeTop: 0,
      contentInset: 0,
    };
  }

  const wAttr = Number(node?.attrs?.width ?? '0');
  const hAttr = Number(node?.attrs?.height ?? '0');
  const hasW = Number.isFinite(wAttr) && wAttr > 0;
  const hasH = Number.isFinite(hAttr) && hAttr > 0;
  const width = hasW ? wAttr : 420;
  const height = hasH ? hAttr : 240;

  return {
    root: 0,
    paddingLeft: 8,
    paddingRight: 8,
    paddingTop: 34,
    paddingBottom: 8,
    width,
    height,
    minWidth: Math.min(200, width),
    minHeight: Math.min(160, height),
    chromeTop: 34,
    contentInset: 8,
  };
}

export function iframeSceneOffset(node) {
  const defaults = iframeLayoutDefaults(node);
  return defaults.root ? { x: 32, y: 32 } : { x: 0, y: 0 };
}

export function iframeContentOffset(node) {
  const defaults = iframeLayoutDefaults(node);
  return {
    x: defaults.root ? 0 : defaults.paddingLeft,
    y: defaults.root ? 0 : defaults.paddingTop,
  };
}
