import { Application, Container, Graphics, Text } from 'pixi.js';
import { dumpCapture, installProbeCapture, markNode, rememberGraphicsCommand } from './capture';
import './style.css';

installProbeCapture();

const appHost = document.querySelector<HTMLDivElement>('#app');
if (!appHost) throw new Error('missing #app');

appHost.innerHTML = `
  <main class="shell">
    <section class="viewport">
      <div id="status">starting pixi probe...</div>
      <div id="pixi"></div>
    </section>
    <aside class="panel">
      <div class="toolbar">
        <button id="move-parent">Move Parent</button>
        <button id="move-child">Move Child</button>
        <button id="reset">Reset</button>
      </div>
      <pre id="dump"></pre>
    </aside>
  </main>
`;

const pixiHost = document.querySelector<HTMLDivElement>('#pixi')!;
const dumpEl = document.querySelector<HTMLPreElement>('#dump')!;
const statusEl = document.querySelector<HTMLDivElement>('#status')!;

async function main(): Promise<void> {
  const app = new Application();
  await app.init({
    width: 640,
    height: 420,
    background: 0xf7f7f7,
    antialias: true,
    preference: 'webgl',
  });
  pixiHost.appendChild(app.canvas);
  statusEl.textContent = 'pixi ready: stage -> parent -> rectangle + text child';

  markNode(app.stage, 'stage');

  const parent = new Container();
  markNode(parent, 'parent-container');
  parent.position.set(90, 70);
  app.stage.addChild(parent);

  const rectBox = new Container();
  markNode(rectBox, 'rect-widget-container');
  rectBox.position.set(40, 35);
  parent.addChild(rectBox);

  const rect = new Graphics();
  markNode(rect, 'rect-graphics');
  rect.rect(0, 0, 220, 120);
  rememberGraphicsCommand(rect, ['rect', 0, 0, 220, 120]);
  rect.fill({ color: 0x2d7ff9, alpha: 0.2 });
  rememberGraphicsCommand(rect, ['fill', { color: 0x2d7ff9, alpha: 0.2 }]);
  rect.stroke({ color: 0x184a90, width: 3 });
  rememberGraphicsCommand(rect, ['stroke', { color: 0x184a90, width: 3 }]);
  rectBox.addChild(rect);

  const label = new Text({
    text: 'text child: local (16, 18)',
    style: {
      fontFamily: 'monospace',
      fontSize: 18,
      fill: 0x111111,
    },
  });
  markNode(label, 'rect-text-child');
  label.position.set(16, 18);
  rectBox.addChild(label);

  const cross = new Graphics();
  markNode(cross, 'origin-cross');
  cross.moveTo(-8, 0);
  rememberGraphicsCommand(cross, ['moveTo', -8, 0]);
  cross.lineTo(8, 0);
  rememberGraphicsCommand(cross, ['lineTo', 8, 0]);
  cross.moveTo(0, -8);
  rememberGraphicsCommand(cross, ['moveTo', 0, -8]);
  cross.lineTo(0, 8);
  rememberGraphicsCommand(cross, ['lineTo', 0, 8]);
  cross.stroke({ color: 0xff3344, width: 2 });
  rememberGraphicsCommand(cross, ['stroke', { color: 0xff3344, width: 2 }]);
  rectBox.addChild(cross);

  function renderDump(): void {
    dumpEl.textContent = dumpCapture(app.stage);
  }

  document.querySelector<HTMLButtonElement>('#move-parent')!.addEventListener('click', () => {
    parent.position.set(parent.position.x + 30, parent.position.y + 20);
    renderDump();
  });

  document.querySelector<HTMLButtonElement>('#move-child')!.addEventListener('click', () => {
    rectBox.position.set(rectBox.position.x + 20, rectBox.position.y + 10);
    renderDump();
  });

  document.querySelector<HTMLButtonElement>('#reset')!.addEventListener('click', () => {
    parent.position.set(90, 70);
    rectBox.position.set(40, 35);
    renderDump();
  });

  (window as any).__pixiProbeDump = () => JSON.parse(dumpCapture(app.stage));

  renderDump();
}

main().catch((error) => {
  const message = String(error?.stack || error);
  statusEl.textContent = 'pixi init failed';
  dumpEl.textContent = message;
});
