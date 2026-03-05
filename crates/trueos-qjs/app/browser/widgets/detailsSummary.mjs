import { Rectangle } from 'pixi.js';
import { clearContainerEvents, clearGraphics, getOrCreateGraphics } from '../../pixi/architecture/pixiReuse.mjs';
export function renderSummary(opts) {
    const { node, container, w, h, theme, detailsOpen, requestRerender } = opts;
    const detailsKey = node.attrs?.['data-details-key'];
    const isOpen = detailsKey ? detailsOpen.get(detailsKey) === true : false;
    const toggle = (ev) => {
        if (!detailsKey)
            return;
        if (ev?.button === 2)
            return;
        const next = !(detailsOpen.get(detailsKey) === true);
        detailsOpen.set(detailsKey, next);
        requestRerender?.();
        ev?.stopPropagation?.();
    };
    // Disclosure arrow: draw a chevron using Graphics primitives.
    const arrowSize = 16;
    const arrowG = getOrCreateGraphics(container, '__arrow');
    clearGraphics(arrowG);
    const strokeW = 2;
    const pad = 3;
    const x0 = pad;
    const y0 = pad;
    const x1 = arrowSize - pad;
    const y1 = arrowSize - pad;
    if (isOpen) {
        // Down chevron: \ /
        arrowG.moveTo(x0, y0);
        arrowG.lineTo((x0 + x1) / 2, y1);
        arrowG.lineTo(x1, y0);
    }
    else {
        // Right chevron: >
        arrowG.moveTo(x0, y0);
        arrowG.lineTo(x1, (y0 + y1) / 2);
        arrowG.lineTo(x0, y1);
    }
    arrowG.stroke({ width: strokeW, color: theme.text });
    arrowG.position.set(4, Math.max(0, (h - arrowSize) / 2));
    // Make the arrow itself clickable.
    arrowG.eventMode = 'static';
    arrowG.cursor = 'pointer';
    arrowG.hitArea = new Rectangle(0, 0, arrowSize + 8, arrowSize + 8);
    arrowG.on('pointerdown', toggle);
    // Toggle the owning <details>.
    if (detailsKey) {
        clearContainerEvents(container);
        container.eventMode = 'static';
        container.cursor = 'pointer';
        container.hitArea = new Rectangle(0, 0, Math.max(0, w), Math.max(0, h));
        container.on('pointerdown', toggle);
    }
}
export function applyYogaDefaultsSummary(yogaNode, Yoga) {
    // A summary is a single row; reserve space for the arrow (16px) + gap.
    yogaNode.setFlexDirection(Yoga.FLEX_DIRECTION_ROW);
    yogaNode.setAlignItems(Yoga.ALIGN_CENTER);
    // Put trailing controls (like checkboxes) all the way on the right.
    yogaNode.setJustifyContent(Yoga.JUSTIFY_SPACE_BETWEEN);
    yogaNode.setPadding(Yoga.EDGE_TOP, 6);
    yogaNode.setPadding(Yoga.EDGE_BOTTOM, 6);
    yogaNode.setPadding(Yoga.EDGE_LEFT, 26);
    yogaNode.setPadding(Yoga.EDGE_RIGHT, 12);
    yogaNode.setMinHeight(36);
}
export function applyYogaDefaultsDetails(yogaNode, Yoga) {
    yogaNode.setPadding(Yoga.EDGE_LEFT, 0);
    yogaNode.setPadding(Yoga.EDGE_RIGHT, 0);
    yogaNode.setPadding(Yoga.EDGE_TOP, 0);
    yogaNode.setPadding(Yoga.EDGE_BOTTOM, 0);
}
export function getEffectiveDetailsChildren(node, detailsOpen) {
    // Collapse <details> content unless open.
    if (!node || node.tagName !== 'details' || !node.key)
        return node?.children ?? [];
    const open = detailsOpen.get(node.key) === true;
    if (open)
        return node.children ?? [];
    return (node.children ?? []).filter((c) => c && c.kind === 'block' && c.tagName === 'summary');
}
