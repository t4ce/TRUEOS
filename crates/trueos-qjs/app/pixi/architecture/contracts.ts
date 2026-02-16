import type { Camera, Scene, WebGLRenderer } from "three";

export type Stage = "input" | "world_update" | "world_render" | "ui_update" | "ui_render" | "present";

export interface FrameInfo {
  nowMs: number;
  dtSec: number;
  frame: number;
}

export interface InputSnapshot {
  pointerX: number;
  pointerY: number;
  pointerDown: boolean;
  pointerDeltaX: number;
  pointerDeltaY: number;
  wheelDeltaY: number;
  keys: ReadonlySet<string>;
}

export interface PassContext {
  frame: FrameInfo;
  input: Readonly<InputSnapshot>;
  bridge: WorldUIBridge;
  diagnostics: DiagnosticsSink;
}

export interface RenderPass {
  id: string;
  stage: Stage;
  enabled: boolean;
  run(ctx: PassContext): void;
}

export interface PassGraph {
  register(pass: RenderPass): void;
  list(): readonly RenderPass[];
  runStage(stage: Stage, ctx: PassContext): void;
}

export interface InputEventRecord {
  kind: "pointer" | "wheel" | "keyboard";
  handledByUI: boolean;
}

export interface InputRouter {
  snapshot(): Readonly<InputSnapshot>;
  routeEvent(event: Event): InputEventRecord;
}

export interface WorldViewModel {
  readonly selectionId: string | null;
  readonly hoverId: string | null;
  readonly fps: number;
  readonly mode: string;
}

export type UICommand =
  | { type: "openPanel"; panelId: string }
  | { type: "setTool"; toolId: string }
  | { type: "focusEntity"; entityId: string };

export interface WorldUIBridge {
  publish(view: WorldViewModel): void;
  readView(): Readonly<WorldViewModel>;
  emitCommand(command: UICommand): void;
  drainCommands(): readonly UICommand[];
}

export interface DiagnosticsEvent {
  type: "warn" | "error";
  code: string;
  message: string;
  frame: number;
}

export interface DiagnosticsSink {
  markPassTime(passId: string, ms: number): void;
  emit(event: DiagnosticsEvent): void;
}

export interface ThreeWorldAdapter {
  readonly renderer: WebGLRenderer;
  readonly scene: Scene;
  readonly camera: Camera;
  init?(): void;
  resize(width: number, height: number, dpr: number): void;
  update(ctx: PassContext): void;
  destroy?(): void;
}

export interface UIRendererAdapter {
  init?(): void;
  resize(width: number, height: number, dpr: number): void;
  update(ctx: PassContext): void;
  render(ctx: PassContext): void;
  destroy?(): void;
}

export interface SchedulerDeps {
  input: InputRouter;
  bridge: WorldUIBridge;
  graph: PassGraph;
  diagnostics: DiagnosticsSink;
  present: () => void;
}

export class OrderedPassGraph implements PassGraph {
  private readonly passes: RenderPass[] = [];

  register(pass: RenderPass): void {
    this.passes.push(pass);
  }

  list(): readonly RenderPass[] {
    return this.passes;
  }

  runStage(stage: Stage, ctx: PassContext): void {
    for (const pass of this.passes) {
      if (!pass.enabled || pass.stage !== stage) continue;
      const t0 = performance.now();
      pass.run(ctx);
      ctx.diagnostics.markPassTime(pass.id, performance.now() - t0);
    }
  }
}

export class InMemoryBridge implements WorldUIBridge {
  private view: WorldViewModel = {
    selectionId: null,
    hoverId: null,
    fps: 0,
    mode: "idle",
  };

  private commands: UICommand[] = [];

  publish(view: WorldViewModel): void {
    this.view = { ...view };
  }

  readView(): Readonly<WorldViewModel> {
    return this.view;
  }

  emitCommand(command: UICommand): void {
    this.commands.push(command);
  }

  drainCommands(): readonly UICommand[] {
    const out = this.commands;
    this.commands = [];
    return out;
  }
}

export class FrameClock {
  private rafId: number | null = null;
  private lastMs = 0;
  private frame = 0;
  private running = false;

  constructor(private readonly deps: SchedulerDeps) {}

  start(): void {
    if (this.running) return;
    this.running = true;
    this.lastMs = performance.now();

    const tick = (nowMs: number) => {
      if (!this.running) return;
      const dtSec = Math.min((nowMs - this.lastMs) / 1000, 0.05);
      this.lastMs = nowMs;
      this.frame += 1;

      const ctx: PassContext = {
        frame: { nowMs, dtSec, frame: this.frame },
        input: this.deps.input.snapshot(),
        bridge: this.deps.bridge,
        diagnostics: this.deps.diagnostics,
      };

      this.deps.graph.runStage("input", ctx);
      this.deps.graph.runStage("world_update", ctx);
      this.deps.graph.runStage("world_render", ctx);
      this.deps.graph.runStage("ui_update", ctx);
      this.deps.graph.runStage("ui_render", ctx);
      this.deps.graph.runStage("present", ctx);
      this.deps.present();

      this.rafId = requestAnimationFrame(tick);
    };

    this.rafId = requestAnimationFrame(tick);
  }

  stop(): void {
    this.running = false;
    if (this.rafId !== null) cancelAnimationFrame(this.rafId);
    this.rafId = null;
  }
}

export function wireThreeWorldPasses(world: ThreeWorldAdapter): RenderPass[] {
  return [
    {
      id: "three.world.update",
      stage: "world_update",
      enabled: true,
      run: (ctx) => world.update(ctx),
    },
    {
      id: "three.world.render",
      stage: "world_render",
      enabled: true,
      run: () => world.renderer.render(world.scene, world.camera),
    },
  ];
}

export function wireUIPasses(ui: UIRendererAdapter): RenderPass[] {
  return [
    {
      id: "pixi.ui.update",
      stage: "ui_update",
      enabled: true,
      run: (ctx) => ui.update(ctx),
    },
    {
      id: "pixi.ui.render",
      stage: "ui_render",
      enabled: true,
      run: (ctx) => ui.render(ctx),
    },
  ];
}
