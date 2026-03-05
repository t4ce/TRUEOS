import { Rectangle } from 'pixi.js';
import { clearContainerEvents } from '../../pixi/architecture/pixiReuse.mjs';
function drawMagnifier(g, cx, cy, r, color) {
    // Lens circle.
    g.circle(cx, cy, r);
    g.stroke({ width: 2, color });
    // Handle: a short diagonal stroke (the "tick").
    const hx0 = cx + r * 0.65;
    const hy0 = cy + r * 0.65;
    const hx1 = cx + r * 1.55;
    const hy1 = cy + r * 1.55;
    g.moveTo(hx0, hy0);
    g.lineTo(hx1, hy1);
    g.stroke({ width: 2, color });
}
export function applyYogaDefaultsSearchRow(yogaNode, Yoga) {
    // A compact row: [icon button][text input].
    yogaNode.setFlexDirection(Yoga.FLEX_DIRECTION_ROW);
    yogaNode.setFlexWrap(Yoga.WRAP_NO_WRAP);
    yogaNode.setAlignItems(Yoga.ALIGN_CENTER);
    yogaNode.setJustifyContent(Yoga.JUSTIFY_FLEX_START);
    yogaNode.setPadding(Yoga.EDGE_LEFT, 0);
    yogaNode.setPadding(Yoga.EDGE_RIGHT, 0);
    yogaNode.setPadding(Yoga.EDGE_TOP, 0);
    yogaNode.setPadding(Yoga.EDGE_BOTTOM, 0);
}
export function applyYogaDefaultsSearchButton(yogaNode, Yoga) {
    yogaNode.setPadding(Yoga.EDGE_LEFT, 0);
    yogaNode.setPadding(Yoga.EDGE_RIGHT, 0);
    yogaNode.setPadding(Yoga.EDGE_TOP, 0);
    yogaNode.setPadding(Yoga.EDGE_BOTTOM, 0);
    // Match our default control height.
    yogaNode.setWidth(36);
    yogaNode.setHeight(36);
    yogaNode.setMinWidth(36);
    yogaNode.setMinHeight(36);
    yogaNode.setFlexGrow(0);
    yogaNode.setFlexShrink(0);
    yogaNode.setMargin(Yoga.EDGE_RIGHT, 6);
}
export function renderSearchButton(opts) {
    const { node, container, graphics: g, w, h, theme, uiState, getPointerId, focusInputKey, requestPaint } = opts;
    const draw = (fill) => {
        g.clear();
        const sw = 1;
        const inset = sw / 2;
        if (theme.control.button.radius > 0)
            g.roundRect(inset, inset, Math.max(0, w - sw), Math.max(0, h - sw), theme.control.button.radius);
        else
            g.rect(inset, inset, Math.max(0, w - sw), Math.max(0, h - sw));
        g.fill(fill);
        g.stroke({ width: sw, color: theme.control.button.border });
        const cx = w / 2 - 2;
        const cy = h / 2 - 2;
        const r = Math.max(5, Math.min(7, Math.min(w, h) * 0.22));
        drawMagnifier(g, cx, cy, r, theme.text);
    };
    draw(theme.control.button.fill);
    // Interactivity: hover/active + focus the linked input.
    clearContainerEvents(container);
    container.eventMode = 'static';
    container.cursor = 'pointer';
    container.hitArea = new Rectangle(0, 0, Math.max(0, w), Math.max(0, h));
    container.on('pointerover', () => draw(theme.control.button.hoverFill));
    container.on('pointerout', () => draw(theme.control.button.fill));
    container.on('pointerdown', (ev) => {
        if (ev?.button === 2)
            return;
        draw(theme.control.button.activeFill);
        if (focusInputKey) {
            const pid = getPointerId(ev);
            if (pid > 0) {
                uiState.focusedKeyByPointer.set(pid, focusInputKey);
                uiState.keyboardOwnerPointerId = pid;
            }
        }
        requestPaint?.();
        ev.stopPropagation?.();
    });
    container.on('pointerup', () => draw(theme.control.button.hoverFill));
    // Allow overriding which input gets focused via attrs too.
    const _unused = node.attrs;
    void _unused;
}
