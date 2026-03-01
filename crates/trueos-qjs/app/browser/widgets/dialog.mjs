import { Rectangle } from 'pixi.js';
import { clearGraphics, getOrCreateGraphics } from '../pixiReuse.mjs';
export function getOrInitDialogState(map, key) {
    const existing = map.get(key);
    if (existing)
        return existing;
    // Default position is top-left of the parent scene.
    const state = { x: 0, y: 0 };
    map.set(key, state);
    return state;
}
export function applyYogaDefaultsDialog(yogaNode, Yoga) {
    // Floating: don't participate in normal document flow.
    yogaNode.setPositionType(Yoga.POSITION_TYPE_ABSOLUTE);
    yogaNode.setPosition(Yoga.EDGE_LEFT, 0);
    yogaNode.setPosition(Yoga.EDGE_TOP, 0);
    yogaNode.setPadding(Yoga.EDGE_LEFT, 12);
    yogaNode.setPadding(Yoga.EDGE_RIGHT, 12);
    yogaNode.setPadding(Yoga.EDGE_TOP, 12);
    yogaNode.setPadding(Yoga.EDGE_BOTTOM, 12);
    yogaNode.setMinWidth(320);
    yogaNode.setMinHeight(180);
}
export function renderDialog(opts) {
    const { node, container, w, h, theme, selectedBy, getCursorColor, dialogStates, dialogDrags, bringToFront, requestPaint } = opts;
    const key = node.key;
    if (!key)
        return;
    // Border: 2px when selected (consistent with other widgets), 1px otherwise.
    const selectedPid = selectedBy.get(key);
    const borderColor = selectedPid == null ? theme.boxBorder : getCursorColor(selectedPid);
    const bw = Math.max(0, Math.round(w));
    const bh = Math.max(0, Math.round(h));
    const border = getOrCreateGraphics(container, '__dialogBorder');
    clearGraphics(border);
    // Solid background so content doesn't show through.
    border.rect(0, 0, bw, bh);
    border.fill({ color: 0xffffff, alpha: 0.8 });
    const sw = selectedPid == null ? 1 : 2;
    const inset = sw / 2;
    border.rect(inset, inset, Math.max(0, bw - sw), Math.max(0, bh - sw));
    border.stroke({ width: sw, color: borderColor, alignment: 0 });
    // Capture drag only on the background, not on children.
    border.eventMode = 'static';
    border.cursor = 'move';
    border.hitArea = new Rectangle(0, 0, bw, bh);
    border.on('pointerdown', (ev) => {
        if (ev?.button === 2)
            return;
        const pid = opts.getPointerId ? opts.getPointerId(ev) : Number(ev?.pointerId ?? ev?.data?.pointerId ?? 0);
        if (pid <= 0)
            return;
        if (pid <= 0)
            return;
        // "Last cursor wins": if someone else was dragging this dialog, cancel their drag.
        for (const [otherPid, d] of dialogDrags.entries()) {
            if (d.key === key && otherPid !== pid)
                dialogDrags.delete(otherPid);
        }
        // Mark selected by this cursor.
        selectedBy.set(key, pid);
        bringToFront?.(key);
        const st = getOrInitDialogState(dialogStates, key);
        dialogDrags.set(pid, {
            key,
            startGX: ev.global?.x ?? 0,
            startGY: ev.global?.y ?? 0,
            originX: st.x,
            originY: st.y,
        });
        requestPaint?.();
        ev.stopPropagation?.();
    });
    // Retained-mode: ensure the dialog chrome stays behind the layout children.
    // In the retained renderer, `__children` holds nested content; if the border
    // sits above it, it will both gray out content (alpha fill) and steal input.
    {
        const byLabel = container.getChildByLabel;
        const childrenRoot = byLabel?.call(container, '__children') ?? container.children.find((c) => c && c.label === '__children') ?? null;
        if (childrenRoot && border.parent === container) {
            const idxChildren = container.getChildIndex(childrenRoot);
            const max = Math.max(0, container.children.length - 1);
            const target = Math.max(0, Math.min(idxChildren - 1, max));
            const cur = container.getChildIndex(border);
            if (cur > target)
                container.setChildIndex(border, target);
        }
    }
}
