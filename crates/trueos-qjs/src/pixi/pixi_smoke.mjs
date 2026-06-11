import { Application, Assets, Container, Sprite, Text, Texture } from 'pixi.js';

function nodeSummary(node) {
  const children = Array.isArray(node.children) ? node.children : [];
  return {
    type: node.constructor?.name || 'Unknown',
    label: String(node.label || node.name || ''),
    x: Number(node.x || 0),
    y: Number(node.y || 0),
    childCount: children.length,
    children: children.map(nodeSummary),
  };
}

export function runPixiSmoke() {
  const app = new Application();
  app.stage.label = 'smoke-stage';

  const root = new Container();
  root.label = 'smoke-root';
  root.position.set(32, 24);
  app.stage.addChild(root);

  const background = new Sprite(Texture.WHITE);
  background.label = 'white-texture-background';
  background.width = 320;
  background.height = 180;
  root.addChild(background);

  const buttonTexture = Texture.WHITE;
  const buttonPositions = [80, 42, 240, 42, 160, 96, 80, 150, 240, 150];
  const buttons = [];

  for (let i = 0; i < 5; i += 1) {
    const button = new Sprite(buttonTexture);
    button.label = `button-${i}`;
    button.anchor.set(0.5);
    button.x = buttonPositions[i * 2];
    button.y = buttonPositions[i * 2 + 1];
    button.eventMode = 'static';
    button.cursor = 'pointer';
    root.addChild(button);
    buttons.push(button);
  }

  const label = new Text({
    text: 'pixi smoke retained scene',
    style: {
      fontFamily: 'monospace',
      fontSize: 16,
      fill: 0x111111,
    },
  });
  label.label = 'smoke-label';
  label.position.set(16, 14);
  root.addChild(label);

  const result = {
    ok: true,
    message: 'pixi smoke ok: imported PixiJS and built a virtual retained scene',
    imported: {
      Application: typeof Application,
      Assets: typeof Assets,
      Sprite: typeof Sprite,
      Texture: typeof Texture,
    },
    stageChildren: app.stage.children.length,
    buttons: buttons.length,
    tree: nodeSummary(app.stage),
  };

  console.log(`[trueos pixi smoke] ${result.message}; stageChildren=${result.stageChildren} buttons=${result.buttons}`);
  globalThis.__TRUEOS_PIXI_SMOKE_RESULT__ = result;
  return result;
}

runPixiSmoke();
