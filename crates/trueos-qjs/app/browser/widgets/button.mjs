import { clearContainerEvents } from '../pixiReuse.mjs';
export function renderButton(opts) {
    const { container, graphics: g, w, h, theme, registerHoverHandlers } = opts;
    const drawButton = (fill) => {
        g.clear();
        const sw = 1;
        const inset = sw / 2;
        if (theme.control.button.radius > 0)
            g.roundRect(inset, inset, Math.max(0, w - sw), Math.max(0, h - sw), theme.control.button.radius);
        else
            g.rect(inset, inset, Math.max(0, w - sw), Math.max(0, h - sw));
        g.fill(fill);
        g.stroke({ width: sw, color: theme.control.button.border });
    };
    drawButton(theme.control.button.fill);
    const over = () => drawButton(theme.control.button.hoverFill);
    const out = () => drawButton(theme.control.button.fill);
    const down = () => drawButton(theme.control.button.activeFill);
    const up = () => drawButton(theme.control.button.hoverFill);
    registerHoverHandlers?.({ over, out, down, up });
    // Lightweight interactivity: hover/active state.
    clearContainerEvents(container);
    container.eventMode = 'static';
    container.cursor = 'pointer';
    container.on('pointerover', over);
    container.on('pointerout', out);
    container.on('pointerdown', down);
    container.on('pointerup', up);
}
export function applyYogaDefaultsButton(yogaNode, Yoga) {
    yogaNode.setFlexDirection(Yoga.FLEX_DIRECTION_ROW);
    yogaNode.setPadding(Yoga.EDGE_TOP, 6);
    yogaNode.setPadding(Yoga.EDGE_BOTTOM, 6);
    yogaNode.setMinHeight(36);
    yogaNode.setMinWidth(100);
    yogaNode.setAlignItems(Yoga.ALIGN_CENTER);
    yogaNode.setJustifyContent(Yoga.JUSTIFY_CENTER);
}
