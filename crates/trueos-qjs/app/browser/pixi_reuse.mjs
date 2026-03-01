import * as PIXI from '/qjs/vendor/pixi.mjs';

const { Container, Graphics, Text } = PIXI;

function findChild(parent, label) {
  // Pixi v8 prefers `label` over `name`.
  const viaApi = parent && parent.getChildByLabel ? parent.getChildByLabel(label) : null;
  if (viaApi) {
    return viaApi;
  }
  const children = (parent && parent.children) || [];
  return children.find((c) => c && c.label === label) || null;
}

export function getOrCreateContainer(parent, label) {
  const existing = findChild(parent, label);
  if (existing) return existing;
  const c = new Container();
  c.label = label;
  parent.addChild(c);
  return c;
}

export function getOrCreateGraphics(parent, label) {
  const existing = findChild(parent, label);
  if (existing) return existing;
  const g = new Graphics();
  g.label = label;
  parent.addChild(g);
  return g;
}

export function getOrCreateText(parent, label, init) {
  const existing = findChild(parent, label);
  if (existing) return existing;
  const t = new Text({ text: '' });
  t.label = label;
  if (typeof init === 'function') {
    init(t);
  }
  parent.addChild(t);
  return t;
}

export function clearGraphics(g) {
  g.clear();
  // Prevent handler buildup when re-rendering retained objects.
  g.removeAllListeners();
  // Keep eventMode unchanged; callers own interactive mode.
  g.hitArea = null;
}

export function clearContainerEvents(c) {
  c.removeAllListeners();
}
