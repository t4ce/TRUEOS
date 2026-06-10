# Pixi Command Probe

Tiny Vite/Pixi fixture for checking the contract between Pixi hierarchy calls and the TrueOS command bridge.

The scene is intentionally small:

`stage -> parent-container -> rect-widget-container -> rect-graphics + rect-text-child`

The important rule: Pixi child `x/y` are local to the parent. `globalX/globalY` are derived diagnostics. A bridge must not replace local child positions with globals while keeping the original parent hierarchy, or text/geometry will drift or stack.

Run:

```sh
npm install
npm run dev -- --host 0.0.0.0
```

Open the printed Vite URL. The right panel dumps the command stream and snapshot. `window.__pixiProbeDump()` returns the same data.
