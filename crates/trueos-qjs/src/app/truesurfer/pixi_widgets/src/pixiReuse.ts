import { Container, Graphics, Text } from 'pixi.js';
import type { DisplayObject } from 'pixi.js';

function findChild<T extends DisplayObject>(parent: Container, label: string): T | null {
  const children = (parent as any).children;
  if (!Array.isArray(children)) return null;
  for (let i = 0; i < children.length; i += 1) {
    const child = children[i] as any;
    if (child && child.label === label) return child as T;
  }
  return null;
}

export function getOrCreateContainer(parent: Container, label: string): Container {
  const existing = findChild<Container>(parent, label);
  if (existing) return existing;
  const c = new Container();
  (c as any).label = label;
  parent.addChild(c);
  return c;
}

export function getOrCreateGraphics(parent: Container, label: string): Graphics {
  const existing = findChild<Graphics>(parent, label);
  if (existing) return existing;
  const g = new Graphics();
  (g as any).label = label;
  parent.addChild(g);
  return g;
}

export function getOrCreateText(parent: Container, label: string, init?: (t: Text) => void): Text {
  const existing = findChild<Text>(parent, label);
  if (existing) return existing;
  const t = new Text({ text: '' });
  (t as any).label = label;
  init?.(t);
  parent.addChild(t);
  return t;
}

export function clearGraphics(g: Graphics): void {
  g.clear();
  // Prevent handler buildup when re-rendering retained objects.
  g.removeAllListeners();
  // Leave eventMode as-is; callers set it for interactive objects.
  (g as any).hitArea = null;
}

export function clearContainerEvents(c: Container): void {
  // Prevent handler buildup when re-rendering retained objects.
  c.removeAllListeners();
}
