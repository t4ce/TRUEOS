import type { Container, Graphics } from 'pixi.js';
import { clearContainerEvents, getOrCreateText } from '../pixiReuse';
import { TEXT_BASELINE_NUDGE_Y } from '../text';

export function renderButton(opts: {
  container: Container;
  graphics: Graphics;
  w: number;
  h: number;
  label?: string;
  theme: {
    fontFamily: string;
    fontSize: number;
    control: {
      button: {
        fill: number;
        hoverFill: number;
        activeFill: number;
        border: number;
        text: number;
        radius: number;
      };
    };
  };

  // Optional hover bridge for non-mouse cursors.
  registerHoverHandlers?: (handlers: { over: () => void; out: () => void }) => void;
}): void {
  const { container, graphics: g, w, h, label, theme, registerHoverHandlers } = opts;

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

  const labelText = getOrCreateText(container, '__label', (t) => {
    (t as any).style = {
      fontFamily: theme.fontFamily,
      fontSize: theme.fontSize,
      fill: theme.control.button.text,
      fontWeight: '400',
      wordWrap: false,
      wordWrapWidth: 0,
    };
  });
  const text = String(label ?? '').trim();
  labelText.text = text;
  labelText.visible = text.length > 0;
  (labelText as any).style = {
    ...(labelText as any).style,
    fontFamily: theme.fontFamily,
    fontSize: theme.fontSize,
    fill: theme.control.button.text,
    wordWrap: false,
    wordWrapWidth: Math.max(0, Math.ceil(w - 16)),
  };
  const measuredW = Number((labelText as any).width ?? 0);
  const measuredH = Number((labelText as any).height ?? 0);
  const fallbackLineH = theme.fontSize * 1.25;
  labelText.position.set(
    measuredW > 0 ? Math.max(8, Math.floor((w - measuredW) / 2)) : 8,
    Math.max(0, Math.floor((h - (measuredH > 0 ? measuredH : fallbackLineH)) / 2)) + TEXT_BASELINE_NUDGE_Y
  );

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
