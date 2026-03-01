import { Container, Graphics, Text } from 'pixi.js';
function findChild(parent, label) {
    // Pixi v8: `name` is removed; `label` is the supported identifier.
    const child = parent.getChildByLabel
        ? parent.getChildByLabel(label)
        : // Fallback: linear scan by label.
            (parent.children.find((c) => c && c.label === label) ?? null);
    return child;
}
export function getOrCreateContainer(parent, label) {
    const existing = findChild(parent, label);
    if (existing)
        return existing;
    const c = new Container();
    c.label = label;
    parent.addChild(c);
    return c;
}
export function getOrCreateGraphics(parent, label) {
    const existing = findChild(parent, label);
    if (existing)
        return existing;
    const g = new Graphics();
    g.label = label;
    parent.addChild(g);
    return g;
}
export function getOrCreateText(parent, label, init) {
    const existing = findChild(parent, label);
    if (existing)
        return existing;
    const t = new Text({ text: '' });
    t.label = label;
    init?.(t);
    parent.addChild(t);
    return t;
}
export function clearGraphics(g) {
    g.clear();
    // Prevent handler buildup when re-rendering retained objects.
    g.removeAllListeners();
    // Leave eventMode as-is; callers set it for interactive objects.
    g.hitArea = null;
}
export function clearContainerEvents(c) {
    // Prevent handler buildup when re-rendering retained objects.
    c.removeAllListeners();
}
