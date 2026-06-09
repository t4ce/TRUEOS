type Listener = (...args: any[]) => void;

class ObservablePoint {
  x: number;
  y: number;

  constructor(x = 0, y = 0) {
    this.x = Number(x) || 0;
    this.y = Number(y) || 0;
  }

  set(x = 0, y = x): void {
    this.x = Number(x) || 0;
    this.y = Number(y) || 0;
  }
}

export class Rectangle {
  x: number;
  y: number;
  width: number;
  height: number;

  constructor(x = 0, y = 0, width = 0, height = 0) {
    this.x = Number(x) || 0;
    this.y = Number(y) || 0;
    this.width = Number(width) || 0;
    this.height = Number(height) || 0;
  }
}

export class DisplayObject {
  parent: Container | null;
  children?: DisplayObject[];
  label?: string;
  name?: string;
  position: ObservablePoint;
  scale: ObservablePoint;
  pivot: ObservablePoint;
  visible: boolean;
  alpha: number;
  mask: DisplayObject | null;
  rotation: number;
  zIndex: number;
  eventMode: string | null;
  cursor: string | null;
  hitArea: unknown;
  listeners: Record<string, Listener[]>;

  constructor() {
    this.parent = null;
    this.position = new ObservablePoint();
    this.scale = new ObservablePoint(1, 1);
    this.pivot = new ObservablePoint();
    this.visible = true;
    this.alpha = 1;
    this.mask = null;
    this.rotation = 0;
    this.zIndex = 0;
    this.eventMode = null;
    this.cursor = null;
    this.hitArea = null;
    this.listeners = {};
  }

  get x(): number {
    return this.position.x;
  }

  set x(value: number) {
    this.position.x = Number(value) || 0;
  }

  get y(): number {
    return this.position.y;
  }

  set y(value: number) {
    this.position.y = Number(value) || 0;
  }

  on(_event: string, _listener: Listener): this {
    // The TRUEOS capture build only needs listener registration as Pixi vocabulary.
    // The command capture wrapper records the "on" op before this facade is called.
    return this;
  }

  removeAllListeners(event?: string): this {
    if (event == null) this.listeners = {};
    else delete this.listeners[String(event)];
    return this;
  }

  removeFromParent(): this {
    this.parent?.removeChild(this);
    return this;
  }

  destroy(_opts?: unknown): void {
    this.removeFromParent();
    this.removeAllListeners();
  }

  toLocal(point: { x?: number; y?: number } | null | undefined): { x: number; y: number } {
    const p = point || {};
    return {
      x: (Number(p.x) || 0) - this.getGlobalX(),
      y: (Number(p.y) || 0) - this.getGlobalY(),
    };
  }

  getGlobalPosition(): { x: number; y: number } {
    return { x: this.getGlobalX(), y: this.getGlobalY() };
  }

  protected getGlobalX(): number {
    return (this.parent ? this.parent.getGlobalX() : 0) + this.x;
  }

  protected getGlobalY(): number {
    return (this.parent ? this.parent.getGlobalY() : 0) + this.y;
  }
}

export class Container extends DisplayObject {
  children: DisplayObject[];
  sortableChildren: boolean;

  constructor() {
    super();
    this.children = [];
    this.sortableChildren = false;
  }

  addChild<T extends DisplayObject[]>(...children: T): T[0] {
    for (const child of children) {
      if (!child) continue;
      child.parent?.removeChild(child);
      child.parent = this;
      this.children.push(child);
    }
    return children[0];
  }

  addChildAt<T extends DisplayObject>(child: T, index: number): T {
    child.parent?.removeChild(child);
    child.parent = this;
    const at = Math.max(0, Math.min(Number(index) | 0, this.children.length));
    this.children.splice(at, 0, child);
    return child;
  }

  removeChild<T extends DisplayObject[]>(...children: T): T[0] {
    for (const child of children) {
      const idx = this.children.indexOf(child);
      if (idx >= 0) this.children.splice(idx, 1);
      if (child) child.parent = null;
    }
    return children[0];
  }

  removeChildren(beginIndex = 0, endIndex = this.children.length): DisplayObject[] {
    const begin = Math.max(0, Number(beginIndex) | 0);
    const end = Math.max(begin, Math.min(Number(endIndex) | 0, this.children.length));
    const removed = this.children.splice(begin, end - begin);
    for (const child of removed) child.parent = null;
    return removed;
  }

  setChildIndex(child: DisplayObject, index: number): void {
    const old = this.children.indexOf(child);
    if (old < 0) return;
    this.children.splice(old, 1);
    const at = Math.max(0, Math.min(Number(index) | 0, this.children.length));
    this.children.splice(at, 0, child);
  }

  getChildIndex(child: DisplayObject): number {
    return this.children.indexOf(child);
  }

  getChildByLabel<T extends DisplayObject = DisplayObject>(label: string): T | null {
    for (let i = 0; i < this.children.length; i += 1) {
      const child = this.children[i] as any;
      if (child && child.label === label) return child as T;
    }
    return null;
  }
}

export class Graphics extends Container {
  commands: unknown[];

  constructor() {
    super();
    this.commands = [];
  }

  clear(): this {
    this.commands.length = 0;
    return this;
  }

  rect(x: number, y: number, width: number, height: number): this {
    this.commands.push(["rect", x, y, width, height]);
    return this;
  }

  roundRect(x: number, y: number, width: number, height: number, radius = 0): this {
    this.commands.push(["roundRect", x, y, width, height, radius]);
    return this;
  }

  circle(x: number, y: number, radius: number): this {
    this.commands.push(["circle", x, y, radius]);
    return this;
  }

  ellipse(x: number, y: number, rx: number, ry: number): this {
    this.commands.push(["ellipse", x, y, rx, ry]);
    return this;
  }

  moveTo(x: number, y: number): this {
    this.commands.push(["moveTo", x, y]);
    return this;
  }

  lineTo(x: number, y: number): this {
    this.commands.push(["lineTo", x, y]);
    return this;
  }

  closePath(): this {
    this.commands.push(["closePath"]);
    return this;
  }

  poly(points: unknown): this {
    this.commands.push(["poly", points]);
    return this;
  }

  fill(style?: unknown): this {
    this.commands.push(["fill", style]);
    return this;
  }

  stroke(style?: unknown): this {
    this.commands.push(["stroke", style]);
    return this;
  }

  image(texId?: unknown, x?: unknown, y?: unknown, w?: unknown, h?: unknown): this {
    this.commands.push(["image", texId, x, y, w, h]);
    return this;
  }

  svg(source?: unknown): this {
    this.commands.push(["svg", source]);
    return this;
  }
}

export class Text extends Container {
  private _text: string;
  private _style: Record<string, any>;
  private _resolution: number;

  constructor(options: { text?: string; style?: Record<string, any> } | string = "") {
    super();
    this._text = "";
    this._style = {};
    this._resolution = 1;
    if (typeof options === "string") {
      this._text = options;
    } else {
      this._text = String(options.text ?? "");
      this._style = { ...(options.style ?? {}) };
    }
  }

  get text(): string {
    return this._text;
  }

  set text(value: string) {
    this._text = String(value ?? "");
  }

  get style(): Record<string, any> {
    return this._style;
  }

  set style(value: Record<string, any>) {
    this._style = value ?? {};
  }

  get resolution(): number {
    return this._resolution;
  }

  set resolution(value: number) {
    this._resolution = Math.max(1, Number(value) || 1);
  }

  get width(): number {
    const fontSize = Number(this._style.fontSize) || 16;
    return this._text.length * fontSize * 0.58;
  }

  get height(): number {
    const fontSize = Number(this._style.fontSize) || 16;
    return Number(this._style.lineHeight) || fontSize * 1.25;
  }

  setSize(_width?: number, _height?: number): this {
    return this;
  }
}

export class MeshGeometry {
  options: unknown;

  constructor(options: unknown = {}) {
    this.options = options;
  }

  addAttribute(_name: string, _options: unknown): this {
    return this;
  }

  destroy(): void {}
}

export class Mesh extends Container {
  geometry: MeshGeometry;
  shader: Shader;

  constructor(options: { geometry?: MeshGeometry; shader?: Shader } = {}) {
    super();
    this.geometry = options.geometry ?? new MeshGeometry();
    this.shader = options.shader ?? new Shader();
  }
}

export class Buffer {
  options: unknown;

  constructor(options: unknown = {}) {
    this.options = options;
  }
}

export const BufferUsage = {
  VERTEX: 1,
  COPY_DST: 2,
};

export class Shader {
  options: unknown;

  constructor(options: unknown = {}) {
    this.options = options;
  }
}

export function compileHighShaderGlProgram(options: unknown): unknown {
  return options;
}

export const colorBitGl = "";
export const localUniformBitGl = "";
export const roundPixelsBitGl = "";

export class Application {
  stage: Container;
  screen: Rectangle;
  canvas: HTMLCanvasElement;
  renderer: {
    width: number;
    height: number;
    screen: Rectangle;
    render(root?: unknown): unknown;
    resize(width: number, height: number): void;
  };
  ticker: {
    stop(): void;
    add(): void;
    remove(): void;
  };

  constructor() {
    const width = Math.max(1, Number((globalThis as any).innerWidth || 1920) | 0);
    const height = Math.max(1, Number((globalThis as any).innerHeight || 1080) | 0);
    this.stage = new Container();
    this.screen = new Rectangle(0, 0, width, height);
    this.canvas = document.createElement("canvas") as HTMLCanvasElement;
    this.ticker = {
      stop() {},
      add() {},
      remove() {},
    };
    this.renderer = {
      width,
      height,
      screen: this.screen,
      render(root?: unknown) {
        return root;
      },
      resize: (nextWidth: number, nextHeight: number) => {
        const w = Math.max(1, Number(nextWidth || width) | 0);
        const h = Math.max(1, Number(nextHeight || height) | 0);
        this.renderer.width = w;
        this.renderer.height = h;
        this.screen.width = w;
        this.screen.height = h;
      },
    };
  }

  async init(_options?: unknown): Promise<void> {}
}
