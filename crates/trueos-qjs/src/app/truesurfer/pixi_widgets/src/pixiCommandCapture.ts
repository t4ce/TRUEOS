import { Container, Graphics, Text } from 'pixi.js';
import type { Application } from 'pixi.js';

type ListenerMetadata = {
  id?: number;
  type: string;
  name?: string;
  arity?: number;
};

type ContextMetadata = {
  id?: number;
  type: string;
};

type PixiCommand = {
  frame: number;
  seq: number;
  op: string;
  id?: number;
  target?: string;
  event?: string;
  listener?: ListenerMetadata;
  args?: unknown[];
};

type PixiCaptureApi = {
  enabled: boolean;
  persist: boolean;
  commands: PixiCommand[];
  counts: Record<string, number>;
  objectId(target: object): number;
  snapshotNode(target: object): unknown;
  clear(): void;
  dump(limit?: number): PixiCommand[];
  flush(): void;
  summary(): Record<string, number>;
};

declare global {
  interface Window {
    __pixiCapture?: PixiCaptureApi;
    __TRUEOS_DISPATCH_PIXI_POINTER__?: (
      nodeId: number,
      event: string,
      x: number,
      y: number,
      pointerId: number,
      buttons: number,
      wheelDeltaY?: number
    ) => Record<string, number>;
  }
}

const MAX_COMMANDS = 50_000;
const objectIds = new WeakMap<object, number>();
const objectsById = new Map<number, any>();
let nextObjectId = 1;
let frame = 0;
let seq = 0;
let installed = false;
let pendingPersist: PixiCommand[] = [];
let flushTimer: number | null = null;

function pixiTypeName(target: any): string {
  if (target instanceof Graphics) return 'Graphics';
  if (target instanceof Text) return 'Text';
  if (target instanceof Container) return 'Container';
  return 'Object';
}

function targetName(target: any): string {
  const label = target && typeof target === 'object' ? target.label : undefined;
  const kind = target && typeof target === 'object' ? pixiTypeName(target) : 'Object';
  return label ? `${kind}:${String(label).slice(0, 80)}` : kind;
}

function objectId(target: object): number {
  let id = objectIds.get(target);
  if (!id) {
    id = nextObjectId++;
    objectIds.set(target, id);
  }
  objectsById.set(id, target);
  return id;
}

function compactArg(arg: unknown): unknown {
  if (arg == null || typeof arg === 'number' || typeof arg === 'string' || typeof arg === 'boolean') {
    return arg;
  }
  if (Array.isArray(arg)) return arg.slice(0, 16).map(compactArg);
  if (typeof arg === 'object') {
    const a: any = arg;
    if ('color' in a || 'alpha' in a || ('width' in a && !('x' in a) && !('y' in a) && !('height' in a))) {
      return {
        color: a.color,
        alpha: a.alpha,
        width: a.width,
      };
    }
    if ('x' in a || 'y' in a || 'width' in a || 'height' in a) {
      return {
        x: Number(a.x ?? 0),
        y: Number(a.y ?? 0),
        w: Number(a.width ?? a.w ?? 0),
        h: Number(a.height ?? a.h ?? 0),
      };
    }
    return pixiTypeName(a);
  }
  return String(arg);
}

function snapshotArg(arg: unknown, depth = 0, seen = new WeakSet<object>()): unknown {
  if (arg == null || typeof arg === 'number' || typeof arg === 'string' || typeof arg === 'boolean') {
    return arg;
  }
  if (typeof arg === 'bigint') return Number.isSafeInteger(Number(arg)) ? Number(arg) : String(arg);
  if (typeof arg === 'symbol') return String(arg);
  if (typeof arg === 'function') {
    return {
      type: 'Function',
      name: arg.name || undefined,
      arity: arg.length,
    };
  }
  if (typeof arg !== 'object') return String(arg);
  if (seen.has(arg)) return '[Circular]';
  if (depth > 12) return pixiTypeName(arg);

  seen.add(arg);
  if (Array.isArray(arg)) return arg.slice(0, 256).map((item) => snapshotArg(item, depth + 1, seen));

  const out: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(arg).slice(0, 128)) {
    out[key] = snapshotArg(value, depth + 1, seen);
  }
  return out;
}

function eventName(event: unknown): string | undefined {
  if (event == null) return undefined;
  return typeof event === 'symbol' ? event.toString() : String(event);
}

function listenerMetadata(listener: unknown): ListenerMetadata | undefined {
  if (listener == null) return undefined;
  if (typeof listener === 'function') {
    return {
      type: 'function',
      name: listener.name || undefined,
      arity: listener.length,
    };
  }
  if (typeof listener === 'object') {
    return {
      id: objectId(listener),
      type: pixiTypeName(listener),
    };
  }
  return { type: typeof listener };
}

function contextMetadata(context: unknown): ContextMetadata | undefined {
  if (context == null) return undefined;
  if (typeof context === 'object') {
    return {
      id: objectId(context),
      type: pixiTypeName(context),
    };
  }
  if (typeof context === 'function') return { type: 'function' };
  return { type: typeof context };
}

function capturedOnArgs(args: unknown[]): unknown[] {
  const captured: Record<string, unknown> = {
    event: eventName(args[0]),
    listener: listenerMetadata(args[1]),
  };
  if (args.length > 2) captured.context = contextMetadata(args[2]);
  return [captured];
}

function compactText(text: unknown): string {
  return String(text ?? '').slice(0, 240);
}

function compactTextStyle(style: unknown): unknown {
  if (!style || typeof style !== 'object') return compactArg(style);

  const source = style as Record<string, unknown>;
  const out: Record<string, unknown> = {
    type: (style as any).constructor?.name ?? 'object',
  };
  for (const key of [
    'fontFamily',
    'fontSize',
    'fontStyle',
    'fontWeight',
    'fill',
    'align',
    'lineHeight',
    'letterSpacing',
    'wordWrap',
    'wordWrapWidth',
    'padding',
  ]) {
    const value = source[key];
    if (value !== undefined) out[key] = compactArg(value);
  }
  return out;
}

function snapshotRect(value: unknown): { x: number; y: number; w: number; h: number } | undefined {
  if (!value || typeof value !== 'object') return undefined;
  const rect = value as Record<string, unknown>;
  const x = Number(rect.x ?? 0);
  const y = Number(rect.y ?? 0);
  const w = Number(rect.width ?? rect.w ?? 0);
  const h = Number(rect.height ?? rect.h ?? 0);
  if (!Number.isFinite(x) || !Number.isFinite(y) || !Number.isFinite(w) || !Number.isFinite(h)) {
    return undefined;
  }
  if (w <= 0 || h <= 0) return undefined;
  return { x, y, w, h };
}

function capturedArgsFor(op: string, args?: unknown[]): unknown[] | undefined {
  if (!args) return undefined;
  if (op === 'addChild' || op === 'removeChild') {
    return args.map((arg) => (arg && typeof arg === 'object' ? objectId(arg) : 0));
  }
  if (op === 'mask') {
    const mask = args[0];
    return [mask && typeof mask === 'object' ? objectId(mask) : 0];
  }
  if (op === 'addChildAt' || op === 'setChildIndex') {
    const child = args[0];
    return [child && typeof child === 'object' ? objectId(child) : 0, Number(args[1]) || 0];
  }
  if (op === 'on') return capturedOnArgs(args);
  if (op === 'snapshot') return args;
  if (op === 'text.text.set') return args.length ? [compactText(args[0])] : [];
  if (op === 'text.style.set') return args.length ? [compactTextStyle(args[0])] : [];
  return args.map(compactArg);
}

function record(target: any, op: string, args?: unknown[]): void {
  try {
    (window as any).__TRUEOS_PIXI_CAPTURE_STEP__ = `record:${op}:begin`;
    const cap = window.__pixiCapture;
    if (!cap?.enabled) return;

    cap.counts[op] = (cap.counts[op] ?? 0) + 1;
    const command = {
      frame,
      seq: ++seq,
      op,
      id: target && typeof target === 'object' ? objectId(target) : undefined,
      target: targetName(target),
      event: op === 'on' && args?.length ? eventName(args[0]) : undefined,
      listener: op === 'on' && args?.length ? listenerMetadata(args[1]) : undefined,
      args: capturedArgsFor(op, args),
    };
    (window as any).__TRUEOS_PIXI_CAPTURE_STEP__ = `record:${op}:push`;
    cap.commands.push(command);
    if (cap.persist) queuePersist(command);
    if (cap.commands.length > MAX_COMMANDS) {
      cap.commands.splice(0, cap.commands.length - MAX_COMMANDS);
    }
    (window as any).__TRUEOS_PIXI_CAPTURE_STEP__ = `record:${op}:done`;
  } catch (err) {
    try {
      (window as any).__TRUEOS_PIXI_CAPTURE_ERROR__ = `record:${op}:${String((err as any)?.message ?? err)}`;
    } catch {
      // Capture must never break the app.
    }
  }
}

function queuePersist(command: PixiCommand): void {
  pendingPersist.push(command);
  if (command.op === 'snapshot') {
    flushPersist();
    return;
  }
  if (pendingPersist.length >= 512) {
    flushPersist();
    return;
  }
  if (flushTimer != null) return;
  flushTimer = window.setTimeout(() => {
    flushTimer = null;
    flushPersist();
  }, 50);
}

function flushPersist(): void {
  if (pendingPersist.length === 0) return;
  const batch = pendingPersist;
  pendingPersist = [];
  const body = batch.map((command) => JSON.stringify(command)).join('\n') + '\n';

  if (navigator.sendBeacon) {
    const ok = navigator.sendBeacon('/__pixi_capture', new Blob([body], { type: 'application/x-ndjson' }));
    if (ok) return;
  }

  void fetch('/__pixi_capture', {
    method: 'POST',
    headers: { 'Content-Type': 'application/x-ndjson' },
    body,
    keepalive: true,
  }).catch(() => {
    pendingPersist = batch.concat(pendingPersist);
  });
}

function invokeCapturedMethod(target: any, op: string, args: unknown[]): unknown {
  if (op === 'on') {
    const event = eventName(args[0]);
    const listener = args[1];
    if (!event || typeof listener !== 'function') return target;
    if (!target.listeners || typeof target.listeners !== 'object') target.listeners = {};
    if (!Array.isArray(target.listeners[event])) target.listeners[event] = [];
    target.listeners[event].push(listener);
    return target;
  }

  if (op === 'addChild') {
    const children = args as any[];
    if (!Array.isArray(target.children)) target.children = [];
    for (const child of children) {
      if (!child) continue;
      if (child.parent && Array.isArray(child.parent.children)) {
        const old = child.parent.children.indexOf(child);
        if (old >= 0) child.parent.children.splice(old, 1);
      }
      child.parent = target;
      target.children.push(child);
    }
    return children[0];
  }

  if (op === 'addChildAt') {
    const child = args[0] as any;
    if (!child) return child;
    if (!Array.isArray(target.children)) target.children = [];
    if (child.parent && Array.isArray(child.parent.children)) {
      const old = child.parent.children.indexOf(child);
      if (old >= 0) child.parent.children.splice(old, 1);
    }
    child.parent = target;
    const at = Math.max(0, Math.min(Number(args[1]) | 0, target.children.length));
    target.children.splice(at, 0, child);
    return child;
  }

  if (op === 'removeChild') {
    const children = args as any[];
    if (!Array.isArray(target.children)) target.children = [];
    for (const child of children) {
      const idx = target.children.indexOf(child);
      if (idx >= 0) target.children.splice(idx, 1);
      if (child) child.parent = null;
    }
    return children[0];
  }

  if (op === 'removeChildren') {
    if (!Array.isArray(target.children)) target.children = [];
    const begin = Math.max(0, Number(args[0] ?? 0) | 0);
    const endDefault = Array.isArray(target.children) ? target.children.length : begin;
    const end = Math.max(begin, Math.min(Number(args[1] ?? endDefault) | 0, endDefault));
    const removed = target.children.splice(begin, end - begin);
    for (const child of removed) child.parent = null;
    return removed;
  }

  if (op === 'setChildIndex') {
    const child = args[0] as any;
    if (!Array.isArray(target.children)) target.children = [];
    const old = target.children.indexOf(child);
    if (old < 0) return undefined;
    target.children.splice(old, 1);
    const at = Math.max(0, Math.min(Number(args[1]) | 0, target.children.length));
    target.children.splice(at, 0, child);
    return undefined;
  }

  if (op === 'removeAllListeners') {
    if (!target.listeners || typeof target.listeners !== 'object') target.listeners = {};
    if (args[0] == null) target.listeners = {};
    else delete target.listeners[String(args[0])];
    return target;
  }

  if (op === 'clear') {
    if (!Array.isArray(target.commands)) target.commands = [];
    target.commands.length = 0;
    return target;
  }

  if (
    op === 'rect' ||
    op === 'roundRect' ||
    op === 'circle' ||
    op === 'ellipse' ||
    op === 'moveTo' ||
    op === 'lineTo' ||
    op === 'closePath' ||
    op === 'poly' ||
    op === 'fill' ||
    op === 'stroke' ||
    op === 'image' ||
    op === 'svg'
  ) {
    if (!Array.isArray(target.commands)) target.commands = [];
    target.commands.push([op, ...args]);
    return target;
  }

  if (op === 'text.setSize') return target;

  return undefined;
}

function installTrueosPointerDispatcher(): void {
  const renderSerial = () => Number((window as any).__TRUEOS_PIXI_RENDER_SERIAL__ ?? 0) || 0;

  const captureRepaintPending = () =>
    Boolean(
      (window as any).__TRUEOS_PIXI_REPAINT_REQUIRED__ ||
        (window as any).__TRUEOS_PIXI_SCROLL_REPAINT_REQUIRED__ ||
        (window as any).__TRUEOS_PIXI_OVERLAY_REPAINT_REQUIRED__,
    );

  window.__TRUEOS_DISPATCH_PIXI_POINTER__ = (nodeId, event, x, y, pointerId, buttons, wheelDeltaY = 0) => {
    const diag = (step: string) => {
      try {
        (window as any).__TRUEOS_PIXI_POINTER_DISPATCH_STEP__ = step;
        if (typeof console !== 'undefined' && typeof console.log === 'function') {
          console.log(`[trueos pointer dispatch] ${step}`);
        }
      } catch {
        // Diagnostics must not affect dispatch.
      }
    };
    diag(`start node=${Number(nodeId) || 0} event=${String(event || '')}`);
    const app = (window as any).__TRUEOS_PIXI_APP;
    if (String(event || '') === 'wheel') {
      const canvas = app?.canvas;
      if (!canvas || typeof canvas.dispatchEvent !== 'function') {
        diag('wheel-canvas-missing');
        return { handled: 0, listenerCount: 0, painted: 0, targetFound: 0 };
      }
      const before = window.__pixiCapture?.commands?.length ?? 0;
      const ev: any = {
        type: 'wheel',
        deltaX: 0,
        deltaY: Number(wheelDeltaY) || 0,
        deltaMode: 0,
        offsetX: Number(x) || 0,
        offsetY: Number(y) || 0,
        clientX: Number(x) || 0,
        clientY: Number(y) || 0,
        pointerId: Number(pointerId) || 1,
        buttons: Number(buttons) || 0,
        defaultPrevented: false,
        propagationStopped: false,
        preventDefault() {
          this.defaultPrevented = true;
        },
        stopPropagation() {
          this.propagationStopped = true;
        },
      };
      diag(`wheel-dispatch deltaY=${ev.deltaY}`);
      const beforeRenderSerial = renderSerial();
      canvas.dispatchEvent(ev);
      let painted = 0;
      if ((window as any).__TRUEOS_CAPTURE_ONLY__) {
        const repaintNow = (window as any).__TRUEOS_REPAINT_NOW__;
        if (captureRepaintPending() && typeof repaintNow === 'function') {
          diag('wheel-repaint-call');
          repaintNow();
          diag('wheel-repaint-return');
          painted = 1;
        }
      } else if (app?.renderer?.render && app?.stage) {
        app.renderer.render(app.stage);
        painted = 1;
      }
      const after = window.__pixiCapture?.commands?.length ?? before;
      const rendered = renderSerial() !== beforeRenderSerial;
      const listener = canvas.listeners?.wheel;
      const listenerCount = Array.isArray(listener) ? listener.length : typeof listener === 'function' ? 1 : 0;
      const handled = ev.defaultPrevented || listenerCount > 0 ? 1 : 0;
      diag(`wheel-done handled=${handled} listeners=${listenerCount} painted=${painted}`);
      const scrollFastPath = (window as any).__TRUEOS_PIXI_LAST_SCROLL_FAST_PATH__;
      if (scrollFastPath?.owner === 'root' || scrollFastPath?.owner === 'iframe') {
        return {
          handled,
          listenerCount,
          painted: 1,
          targetFound: 1,
          scrollFastPath: 1,
          rootNode: Number(scrollFastPath.rootNode) || 0,
          contentNode: Number(scrollFastPath.contentNode) || 0,
          contentY: Number(scrollFastPath.contentY) || 0,
          scrollbarNode: Number(scrollFastPath.scrollbarNode) || 0,
          scrollbarVisible: Number(scrollFastPath.scrollbarVisible) || 0,
          trackX: Number(scrollFastPath.trackX) || 0,
          trackY: Number(scrollFastPath.trackY) || 0,
          trackW: Number(scrollFastPath.trackW) || 0,
          trackH: Number(scrollFastPath.trackH) || 0,
          thumbX: Number(scrollFastPath.thumbX) || 0,
          thumbY: Number(scrollFastPath.thumbY) || 0,
          thumbW: Number(scrollFastPath.thumbW) || 0,
          thumbH: Number(scrollFastPath.thumbH) || 0,
        };
      }
      return { handled, listenerCount, painted: after > before || rendered || painted ? 1 : 0, targetFound: 1 };
    }
    const target = objectsById.get(Number(nodeId) || 0);
    let handled = 0;
    let listenerCount = 0;
    let painted = 0;
    if (!target) {
      diag('target-missing');
      return { handled, listenerCount, painted, targetFound: 0 };
    }

    (window as any).__TRUEOS_PIXI_LAST_GRAPHICS_FAST_PATH__ = null;
    (window as any).__TRUEOS_PIXI_LAST_OVERLAY_FAST_PATH__ = null;
    (window as any).__TRUEOS_PIXI_LAST_SCROLL_FAST_PATH__ = null;
    const ev: any = {
      type: String(event || ''),
      button: Number(buttons) & 2 ? 2 : 0,
      buttons: Number(buttons) || 0,
      pointerId: Number(pointerId) || 1,
      pointerType: 'mouse',
      global: { x: Number(x) || 0, y: Number(y) || 0 },
      data: {
        pointerId: Number(pointerId) || 1,
        pointerType: 'mouse',
        global: { x: Number(x) || 0, y: Number(y) || 0 },
      },
      target,
      currentTarget: target,
      defaultPrevented: false,
      propagationStopped: false,
      preventDefault() {
        this.defaultPrevented = true;
      },
      stopPropagation() {
        this.propagationStopped = true;
      },
    };

    const before = window.__pixiCapture?.commands?.length ?? 0;
    const beforeRenderSerial = renderSerial();
    diag(`target-found label=${String(target.label ?? '')}`);
    for (let node: any = target; node; node = node.parent) {
      ev.currentTarget = node;
      const listeners = node.listeners?.[ev.type];
      if (!Array.isArray(listeners) || listeners.length === 0) continue;
      listenerCount += listeners.length;
      diag(`listeners node=${objectIds.get(node) ?? 0} count=${listeners.length}`);
      for (const listener of listeners.slice()) {
        if (typeof listener !== 'function') continue;
        handled = 1;
        diag(`listener-call node=${objectIds.get(node) ?? 0}`);
        listener.call(node, ev);
        diag(`listener-return node=${objectIds.get(node) ?? 0}`);
        if (ev.propagationStopped) break;
      }
      if (ev.propagationStopped) break;
    }

    if ((window as any).__TRUEOS_CAPTURE_ONLY__) {
      const repaintNow = (window as any).__TRUEOS_REPAINT_NOW__;
      if (captureRepaintPending() && typeof repaintNow === 'function') {
        diag('capture-repaint-call');
        repaintNow();
        diag('capture-repaint-return');
        painted = 1;
      }
    } else if (app?.renderer?.render && app?.stage) {
      diag('paint-call');
      app.renderer.render(app.stage);
      diag('paint-return');
      painted = 1;
    }

    const after = window.__pixiCapture?.commands?.length ?? before;
    const rendered = renderSerial() !== beforeRenderSerial;
    const scrollFastPath = (window as any).__TRUEOS_PIXI_LAST_SCROLL_FAST_PATH__;
    if (scrollFastPath?.owner === 'root' || scrollFastPath?.owner === 'iframe') {
      diag(`scroll-fast owner=${scrollFastPath.owner}`);
      return {
        handled,
        listenerCount,
        painted: 1,
        targetFound: 1,
        scrollFastPath: 1,
        rootNode: Number(scrollFastPath.rootNode) || 0,
        contentNode: Number(scrollFastPath.contentNode) || 0,
        contentY: Number(scrollFastPath.contentY) || 0,
        scrollbarNode: Number(scrollFastPath.scrollbarNode) || 0,
        scrollbarVisible: Number(scrollFastPath.scrollbarVisible) || 0,
        trackX: Number(scrollFastPath.trackX) || 0,
        trackY: Number(scrollFastPath.trackY) || 0,
        trackW: Number(scrollFastPath.trackW) || 0,
        trackH: Number(scrollFastPath.trackH) || 0,
        thumbX: Number(scrollFastPath.thumbX) || 0,
        thumbY: Number(scrollFastPath.thumbY) || 0,
        thumbW: Number(scrollFastPath.thumbW) || 0,
        thumbH: Number(scrollFastPath.thumbH) || 0,
      };
    }
    const graphicsFastPath = (window as any).__TRUEOS_PIXI_LAST_GRAPHICS_FAST_PATH__;
    const graphicsFastEvent =
      graphicsFastPath?.owner === 'button-hover'
        ? ev.type === 'pointerover' || ev.type === 'pointerout' || ev.type === 'pointerdown' || ev.type === 'pointerup'
        : ev.type === 'pointerover' || ev.type === 'pointerout';
    if (
      (graphicsFastPath?.owner === 'context-menu-hover' || graphicsFastPath?.owner === 'button-hover') &&
      graphicsFastEvent &&
      after > before
    ) {
      if (window.__pixiCapture?.commands) {
        window.__pixiCapture.commands.splice(before, after - before);
      }
      diag(`graphics-fast owner=${graphicsFastPath.owner}`);
      return {
        handled,
        listenerCount,
        painted: 1,
        targetFound: 1,
        graphicsFastPath: 1,
        rootNode: Number(graphicsFastPath.rootNode) || 0,
        graphicsNode: Number(graphicsFastPath.graphicsNode) || 0,
        rectX: Number(graphicsFastPath.x) || 0,
        rectY: Number(graphicsFastPath.y) || 0,
        rectW: Number(graphicsFastPath.w) || 0,
        rectH: Number(graphicsFastPath.h) || 0,
        damageX: Number(graphicsFastPath.worldX) + Number(graphicsFastPath.x) || 0,
        damageY: Number(graphicsFastPath.worldY) + Number(graphicsFastPath.y) || 0,
        damageW: Number(graphicsFastPath.w) || 0,
        damageH: Number(graphicsFastPath.h) || 0,
        fillColor: Number(graphicsFastPath.fillColor) || 0,
        fillAlpha: Number(graphicsFastPath.fillAlpha) || 0,
        strokeColor: Number(graphicsFastPath.strokeColor) || 0,
        strokeAlpha: Number(graphicsFastPath.strokeAlpha) || 0,
        strokeWidth: Number(graphicsFastPath.strokeWidth) || 0,
      };
    }
    const overlayFastPath = (window as any).__TRUEOS_PIXI_LAST_OVERLAY_FAST_PATH__;
    if (overlayFastPath && Number(overlayFastPath.rootNode) > 0 && Number(overlayFastPath.damageW) > 0 && Number(overlayFastPath.damageH) > 0) {
      diag('overlay-fast');
      return {
        handled,
        listenerCount,
        painted: 1,
        targetFound: 1,
        overlayFastPath: 1,
        rootNode: Number(overlayFastPath.rootNode) || 0,
        damageX: Number(overlayFastPath.damageX) || 0,
        damageY: Number(overlayFastPath.damageY) || 0,
        damageW: Number(overlayFastPath.damageW) || 0,
        damageH: Number(overlayFastPath.damageH) || 0,
      };
    }
    painted = after > before || rendered || painted ? 1 : 0;
    diag(`done handled=${handled} listeners=${listenerCount} painted=${painted}`);
    return { handled, listenerCount, painted, targetFound: 1 };
  };
}

function patchMethod(proto: any, name: string, op = name): void {
  const original = proto?.[name];
  if (typeof original !== 'function' || original.__pixiCapturePatched) return;

  const patched = function patchedPixiMethod(this: unknown, ...args: unknown[]) {
    record(this, op, args);
    if (!(window as any).__TRUEOS_CAPTURE_ONLY__) {
      return original.apply(this, args);
    }
    try {
      (window as any).__TRUEOS_PIXI_CAPTURE_STEP__ = `invoke:${op}:begin`;
      const out = invokeCapturedMethod(this, op, args);
      (window as any).__TRUEOS_PIXI_CAPTURE_STEP__ = `invoke:${op}:done`;
      return out;
    } catch (err) {
      try {
        (window as any).__TRUEOS_PIXI_CAPTURE_ERROR__ = `invoke:${op}:${String((err as any)?.message ?? err)}`;
      } catch {
        // Capture must never break the app.
      }
      return op === 'addChild' || op === 'addChildAt' || op === 'removeChild' ? args[0] : this;
    }
  };
  patched.__pixiCapturePatched = true;
  proto[name] = patched;
}

function propertyDescriptor(proto: any, name: string): PropertyDescriptor | undefined {
  let cursor = proto;
  while (cursor) {
    const descriptor = Object.getOwnPropertyDescriptor(cursor, name);
    if (descriptor) return descriptor;
    cursor = Object.getPrototypeOf(cursor);
  }
  return undefined;
}

function patchSetter(proto: any, name: string, op: string): void {
  if (!proto?.constructor || proto.constructor[`__pixiCapturePatched_${op}`]) return;

  const descriptor = propertyDescriptor(proto, name);
  if (descriptor?.configurable === false) return;
  if (descriptor && !descriptor.set && !descriptor.writable) return;

  const fallbackKey =
    typeof Symbol === 'function'
      ? Symbol(`pixiCapture:${op}`)
      : `__pixiCaptureValue_${op}`;

  Object.defineProperty(proto, name, {
    configurable: descriptor?.configurable ?? true,
    enumerable: descriptor?.enumerable ?? true,
    get: descriptor?.get
      ? function getPixiCapturedProperty(this: unknown) {
          return descriptor.get?.call(this);
        }
      : function getPixiCapturedField(this: unknown) {
          const target = this as any;
          if (Object.prototype.hasOwnProperty.call(target, fallbackKey)) return target[fallbackKey];
          return descriptor && 'value' in descriptor ? descriptor.value : undefined;
        },
    set: function setPixiCapturedProperty(this: unknown, value: unknown) {
      record(this, op, [value]);
      if (!(window as any).__TRUEOS_CAPTURE_ONLY__) {
        if (descriptor?.set) descriptor.set.call(this, value);
        else {
          Object.defineProperty(this, fallbackKey, {
            configurable: true,
            enumerable: false,
            writable: true,
            value,
          });
        }
        return;
      }
      const target = this as any;
      if (op === 'text.text.set') target._text = String(value ?? '');
      else if (op === 'text.style.set') target._style = value ?? {};
      else if (op === 'text.resolution.set') target._resolution = Math.max(1, Number(value) || 1);
      else {
        Object.defineProperty(target, fallbackKey, {
          configurable: true,
          enumerable: false,
          writable: true,
          value,
        });
      }
    },
  });
  proto.constructor[`__pixiCapturePatched_${op}`] = true;
}

function snapshotNode(node: any, depth = 0): unknown {
  if (!node || depth > 64) return null;
  let globalX: number | undefined;
  let globalY: number | undefined;
  try {
    const global = typeof node.getGlobalPosition === 'function' ? node.getGlobalPosition() : null;
    if (global && Number.isFinite(Number(global.x)) && Number.isFinite(Number(global.y))) {
      globalX = Number(global.x);
      globalY = Number(global.y);
    }
  } catch {
    // Snapshot capture is diagnostic/bridge data; never let it break rendering.
  }
  const out: Record<string, unknown> = {
    id: objectId(node),
    type: pixiTypeName(node),
    label: node.label ?? undefined,
    x: node.position?.x ?? node.x ?? 0,
    y: node.position?.y ?? node.y ?? 0,
    globalX,
    globalY,
    scaleX: Number.isFinite(Number(node.scale?.x)) ? Number(node.scale.x) : 1,
    scaleY: Number.isFinite(Number(node.scale?.y)) ? Number(node.scale.y) : 1,
    visible: node.visible,
    alpha: Number.isFinite(Number(node.alpha)) ? Number(node.alpha) : 1,
    maskId: node.mask ? objectId(node.mask) : 0,
    zIndex: Number(node.zIndex) || 0,
    sortableChildren: node.sortableChildren === true,
  };
  const hitArea = snapshotRect(node.hitArea);
  if (hitArea) out.hitArea = hitArea;
  if (node.listeners && typeof node.listeners === 'object') {
    const events = Object.keys(node.listeners).filter((name) => {
      const listeners = node.listeners?.[name];
      return Array.isArray(listeners) && listeners.length > 0;
    });
    if (events.length > 0) out.listeners = events.slice(0, 16);
  }
  if (node instanceof Graphics && Array.isArray(node.commands) && node.commands.length > 0) {
    out.commands = node.commands.slice(-256).map((command: unknown) => snapshotArg(command, 0));
  }
  if (typeof node.text === 'string') {
    out.text = node.text.slice(0, 120);
    if (node instanceof Text && node.style && typeof node.style === 'object') {
      const style: Record<string, unknown> = {};
      const rawStyle = node.style as any;
      if (typeof rawStyle.fontSize !== 'undefined') style.fontSize = snapshotArg(rawStyle.fontSize, 0);
      if (typeof rawStyle.fontWeight !== 'undefined') style.fontWeight = snapshotArg(rawStyle.fontWeight, 0);
      if (typeof rawStyle.fill !== 'undefined') style.fill = snapshotArg(rawStyle.fill, 0);
      if (Object.keys(style).length > 0) out.textStyle = style;
    }
  }
  if (Array.isArray(node.children) && node.children.length) {
    out.children = node.children.map((child: unknown) => snapshotNode(child, depth + 1));
  }
  return out;
}

export function installPixiCommandCapture(): PixiCaptureApi {
  if (window.__pixiCapture) return window.__pixiCapture;

  const api: PixiCaptureApi = {
    enabled: true,
    persist: !(window as any).__TRUEOS_CAPTURE_ONLY__,
    commands: [],
    counts: Object.create(null),
    objectId(target: object) {
      return objectId(target);
    },
    snapshotNode(target: object) {
      return snapshotNode(target);
    },
    clear() {
      this.commands.length = 0;
      this.counts = Object.create(null);
    },
    dump(limit = 200) {
      return this.commands.slice(-limit);
    },
    flush() {
      flushPersist();
    },
    summary() {
      return { ...this.counts };
    },
  };
  window.__pixiCapture = api;
  installTrueosPointerDispatcher();
  window.addEventListener('beforeunload', () => flushPersist());

  if (!installed) {
    installed = true;

    if (typeof (Graphics.prototype as any).image !== 'function') {
      (Graphics.prototype as any).image = function image(): any {
        return this;
      };
    }

    for (const name of [
      'clear',
      'rect',
      'roundRect',
      'circle',
      'ellipse',
      'moveTo',
      'lineTo',
      'closePath',
      'poly',
      'fill',
      'stroke',
      'image',
      'svg',
    ]) {
      patchMethod(Graphics.prototype, name);
    }

    for (const name of [
      'addChild',
      'addChildAt',
      'removeChild',
      'removeChildren',
      'setChildIndex',
      'on',
      'removeAllListeners',
    ]) {
      patchMethod(Container.prototype, name);
    }

    patchSetter(Text.prototype, 'text', 'text.text.set');
    patchSetter(Text.prototype, 'style', 'text.style.set');
    patchSetter(Text.prototype, 'resolution', 'text.resolution.set');
    patchMethod(Text.prototype, 'setSize', 'text.setSize');
    patchSetter(Container.prototype, 'visible', 'visible');
    patchSetter(Container.prototype, 'alpha', 'alpha');
    patchSetter(Container.prototype, 'mask', 'mask');
  }

  return api;
}

export function attachPixiRenderCapture(app: Application): void {
  const renderer = app.renderer as any;
  const original = renderer?.render;
  if (typeof original !== 'function' || original.__pixiCapturePatched) return;

  const patched = function patchedRendererRender(root: unknown) {
    const renderRoot =
      root && typeof root === 'object' && 'container' in (root as any)
        ? (root as any).container
        : root || app.stage;
    frame++;
    (window as any).__TRUEOS_PIXI_RENDER_SERIAL__ = (Number((window as any).__TRUEOS_PIXI_RENDER_SERIAL__ ?? 0) || 0) + 1;
    if ((window as any).__TRUEOS_CAPTURE_ONLY__) {
      window.__pixiCapture?.clear();
    }
    record(renderRoot, 'render', []);
    record(renderRoot, 'snapshot', [snapshotNode(renderRoot)]);
    if ((window as any).__TRUEOS_CAPTURE_ONLY__) return renderRoot;
    return original.call(this, root);
  };
  patched.__pixiCapturePatched = true;
  renderer.render = patched;
}
