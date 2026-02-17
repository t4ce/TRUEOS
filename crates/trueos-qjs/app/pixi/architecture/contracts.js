export class OrderedPassGraph {
  constructor() {
    this.passes = [];
  }

  register(pass) {
    this.passes.push(pass);
  }

  list() {
    return this.passes;
  }

  runStage(stage, ctx) {
    for (const pass of this.passes) {
      if (!pass.enabled || pass.stage !== stage) continue;
      const t0 = performance.now();
      pass.run(ctx);
      ctx.diagnostics.markPassTime(pass.id, performance.now() - t0);
    }
  }
}

export class InMemoryBridge {
  constructor() {
    this.view = {
      selectionId: null,
      hoverId: null,
      fps: 0,
      mode: "idle",
    };
    this.commands = [];
  }

  publish(view) {
    this.view = { ...view };
  }

  readView() {
    return this.view;
  }

  emitCommand(command) {
    this.commands.push(command);
  }

  drainCommands() {
    const out = this.commands;
    this.commands = [];
    return out;
  }
}

export class FrameClock {
  constructor(deps) {
    this.deps = deps;
    this.rafId = null;
    this.lastMs = 0;
    this.frame = 0;
    this.running = false;
  }

  start() {
    if (this.running) return;
    this.running = true;
    this.lastMs = performance.now();

    const tick = (nowMs) => {
      if (!this.running) return;
      const dtSec = Math.min((nowMs - this.lastMs) / 1000, 0.05);
      this.lastMs = nowMs;
      this.frame += 1;

      const ctx = {
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

  stop() {
    this.running = false;
    if (this.rafId !== null) cancelAnimationFrame(this.rafId);
    this.rafId = null;
  }
}

export function wireThreeWorldPasses(world) {
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

export function wireUIPasses(ui) {
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

export function makeCenteredHexagonState(width, height) {
  const minSide = Math.max(1, Math.min(width, height));
  return {
    width,
    height,
    radius: Math.max(24, Math.floor(minSide * 0.18)),
    angleRad: 0,
    clearRgb: 0x081830,
    fillRgb: 0x3ddc97,
  };
}
