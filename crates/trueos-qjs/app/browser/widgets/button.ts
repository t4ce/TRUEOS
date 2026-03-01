import type { Container, Graphics } from 'pixi.js';
import { clearContainerEvents } from '../pixiReuse';

export function renderButton(opts: {
  container: Container;
  graphics: Graphics;
  w: number;
  h: number;
  theme: { control: { button: { fill: number; hoverFill: number; activeFill: number; border: number; radius: number } } };

  // Optional hover bridge for non-mouse cursors.
  registerHoverHandlers?: (handlers: { over: () => void; out: () => void }) => void;
}): void {
  const { container, graphics: g, w, h, theme, registerHoverHandlers } = opts;

  const drawButton = (fill: number) => {
    g.clear();
    const sw = 1;
    const inset = sw / 2;
    if (theme.control.button.radius > 0)
      g.roundRect(inset, inset, Math.max(0, w - sw), Math.max(0, h - sw), theme.control.button.radius);
    else g.rect(inset, inset, Math.max(0, w - sw), Math.max(0, h - sw));
    g.fill(fill);
    g.stroke({ width: sw, color: theme.control.button.border });
  };

  drawButton(theme.control.button.fill);

  const over = () => drawButton(theme.control.button.hoverFill);
  const out = () => drawButton(theme.control.button.fill);
  registerHoverHandlers?.({ over, out });

  // Lightweight interactivity: hover/active state.
  clearContainerEvents(container);
  container.eventMode = 'static';
  container.cursor = 'pointer';
  container.on('pointerover', over);
  container.on('pointerout', out);
  container.on('pointerdown', () => drawButton(theme.control.button.activeFill));
  container.on('pointerup', () => drawButton(theme.control.button.hoverFill));
}

export function applyYogaDefaultsButton(yogaNode: any, Yoga: any): void {
  yogaNode.setFlexDirection(Yoga.FLEX_DIRECTION_ROW);
  yogaNode.setPadding(Yoga.EDGE_TOP, 6);
  yogaNode.setPadding(Yoga.EDGE_BOTTOM, 6);
  yogaNode.setMinHeight(36);
  yogaNode.setMinWidth(100);
  yogaNode.setAlignItems(Yoga.ALIGN_CENTER);
  yogaNode.setJustifyContent(Yoga.JUSTIFY_CENTER);
}
