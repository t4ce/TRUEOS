import { Container, Graphics, Text } from 'pixi.js';

type CaptureCommand = {
  op: string;
  id: number;
  target: string;
  args: unknown[];
};

type SnapshotNode = {
  id: number;
  type: string;
  label: string;
  x: number;
  y: number;
  globalX: number;
  globalY: number;
  commands?: unknown[][];
  text?: string;
  children: SnapshotNode[];
};

let nextId = 1;
const ids = new WeakMap<object, number>();
const commands: CaptureCommand[] = [];

function idOf(value: object): number {
  let id = ids.get(value);
  if (id == null) {
    id = nextId++;
    ids.set(value, id);
  }
  return id;
}

function typeOf(value: unknown): string {
  if (value instanceof Text) return 'Text';
  if (value instanceof Graphics) return 'Graphics';
  if (value instanceof Container) return 'Container';
  return 'Unknown';
}

function labelOf(value: any): string {
  return String(value.label || value.name || typeOf(value));
}

function commandArg(value: unknown): unknown {
  if (value instanceof Container || value instanceof Graphics || value instanceof Text) {
    return { id: idOf(value), type: typeOf(value), label: labelOf(value) };
  }
  if (Array.isArray(value)) return value.map(commandArg);
  if (value && typeof value === 'object') return { ...(value as Record<string, unknown>) };
  return value;
}

function record(target: any, op: string, args: unknown[]): void {
  commands.push({
    op,
    id: idOf(target),
    target: `${typeOf(target)}:${labelOf(target)}`,
    args: args.map(commandArg),
  });
}

function installMethodPatch(proto: any, name: string, op = name): void {
  const original = proto[name];
  if (typeof original !== 'function' || original.__trueosProbePatched) return;
  proto[name] = function patchedMethod(this: any, ...args: unknown[]) {
    const result = original.apply(this, args);
    record(this, op, args);
    return result;
  };
  proto[name].__trueosProbePatched = true;
}

function installPositionPatch(): void {
  const probe = new Container();
  const proto = Object.getPrototypeOf(probe.position);
  const original = proto.set;
  if (typeof original !== 'function' || original.__trueosProbePatched) return;
  proto.set = function patchedPositionSet(this: any, x = 0, y = x) {
    const result = original.call(this, x, y);
    const owner = this._observer;
    if (owner) record(owner, 'position.set', [x, y]);
    return result;
  };
  proto.set.__trueosProbePatched = true;
}

export function installProbeCapture(): void {
  installPositionPatch();
  for (const name of ['addChild', 'addChildAt', 'removeChild', 'removeChildren']) {
    installMethodPatch(Container.prototype, name);
  }
  for (const name of ['clear', 'rect', 'roundRect', 'circle', 'moveTo', 'lineTo', 'fill', 'stroke']) {
    installMethodPatch(Graphics.prototype, name);
  }
}

export function markNode(node: Container, label: string): void {
  node.label = label;
  record(node, 'node.label', [label]);
}

export function snapshot(root: Container): SnapshotNode {
  const walk = (node: any): SnapshotNode => {
    const global = node.getGlobalPosition();
    const out: SnapshotNode = {
      id: idOf(node),
      type: typeOf(node),
      label: labelOf(node),
      x: Number(node.position?.x ?? node.x ?? 0),
      y: Number(node.position?.y ?? node.y ?? 0),
      globalX: Number(global.x || 0),
      globalY: Number(global.y || 0),
      children: [],
    };
    if (node instanceof Graphics && Array.isArray((node as any).__probeGraphicsCommands)) {
      out.commands = (node as any).__probeGraphicsCommands.slice();
    }
    if (node instanceof Text) out.text = node.text;
    if (Array.isArray(node.children)) out.children = node.children.map(walk);
    return out;
  };
  return walk(root);
}

export function rememberGraphicsCommand(graphics: Graphics, command: unknown[]): void {
  const bag = ((graphics as any).__probeGraphicsCommands ??= []);
  bag.push(command);
}

export function dumpCapture(root: Container): string {
  return JSON.stringify(
    {
      note: 'Pixi contract: children keep local x/y under their parent. globalX/globalY are derived, not replacement locals.',
      commands,
      snapshot: snapshot(root),
    },
    null,
    2
  );
}
