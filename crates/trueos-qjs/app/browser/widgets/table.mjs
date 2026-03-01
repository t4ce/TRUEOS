export function renderTable(opts) {
    const { graphics: g, w, h, boxBorder } = opts;
    const bw = Math.max(0, Math.round(w));
    const bh = Math.max(0, Math.round(h));
    g.rect(0, 0, bw, bh);
    g.stroke({ width: 1, color: boxBorder, alignment: 0 });
}
export function renderCell(opts) {
    const { nodeTag, graphics: g, w, h, theme } = opts;
    if (nodeTag === 'th') {
        g.rect(0, 0, w, h);
        g.fill(theme.control.table.headerFill);
    }
    g.rect(0, 0, w, h);
    g.stroke({ width: 1, color: theme.control.table.cellBorder });
}
export function applyYogaDefaultsTable(yogaNode, Yoga) {
    yogaNode.setPadding(Yoga.EDGE_LEFT, 0);
    yogaNode.setPadding(Yoga.EDGE_RIGHT, 0);
    yogaNode.setPadding(Yoga.EDGE_TOP, 0);
    yogaNode.setPadding(Yoga.EDGE_BOTTOM, 0);
}
export function applyYogaDefaultsTr(yogaNode, Yoga) {
    yogaNode.setFlexDirection(Yoga.FLEX_DIRECTION_ROW);
    yogaNode.setFlexWrap(Yoga.WRAP_NO_WRAP);
    yogaNode.setPadding(Yoga.EDGE_LEFT, 0);
    yogaNode.setPadding(Yoga.EDGE_RIGHT, 0);
    yogaNode.setPadding(Yoga.EDGE_TOP, 0);
    yogaNode.setPadding(Yoga.EDGE_BOTTOM, 0);
    yogaNode.setMargin(Yoga.EDGE_BOTTOM, 0);
}
export function applyYogaDefaultsCell(yogaNode, Yoga) {
    yogaNode.setFlexGrow(1);
    yogaNode.setFlexShrink(1);
    yogaNode.setMinWidth(80);
    yogaNode.setPadding(Yoga.EDGE_LEFT, 8);
    yogaNode.setPadding(Yoga.EDGE_RIGHT, 8);
    yogaNode.setPadding(Yoga.EDGE_TOP, 6);
    yogaNode.setPadding(Yoga.EDGE_BOTTOM, 6);
    yogaNode.setMargin(Yoga.EDGE_BOTTOM, 0);
}
