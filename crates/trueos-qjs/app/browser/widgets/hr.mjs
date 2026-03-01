export function renderHr(opts) {
    const { graphics: g, w, theme } = opts;
    g.rect(0, 0, Math.round(w), 1);
    g.fill(theme.hr);
}
export function applyYogaDefaultsHr(yogaNode, Yoga) {
    yogaNode.setPadding(Yoga.EDGE_LEFT, 0);
    yogaNode.setPadding(Yoga.EDGE_RIGHT, 0);
    yogaNode.setPadding(Yoga.EDGE_TOP, 0);
    yogaNode.setPadding(Yoga.EDGE_BOTTOM, 0);
    yogaNode.setMargin(Yoga.EDGE_TOP, 2);
    yogaNode.setMargin(Yoga.EDGE_BOTTOM, 2);
    yogaNode.setHeight(1);
}
