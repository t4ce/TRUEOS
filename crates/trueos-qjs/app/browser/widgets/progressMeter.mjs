export function renderProgressOrMeter(opts) {
    const { node, graphics: g, w, h, theme } = opts;
    // Use integer pixel geometry for bars to keep inner padding symmetric.
    const bw = Math.max(0, Math.round(w));
    const bh = Math.max(0, Math.round(h));
    {
        const sw = 1;
        const inset = sw / 2;
        g.rect(inset, inset, Math.max(0, bw - sw), Math.max(0, bh - sw));
        g.fill(theme.control.progress.background);
        g.stroke({ width: sw, color: theme.control.progress.border });
    }
    const value = Number(node.attrs?.value ?? '0');
    const max = Number(node.attrs?.max ?? '1');
    const ratio = max > 0 ? Math.max(0, Math.min(1, value / max)) : 0;
    // Inner bar padding: keep the fill off the border.
    // With a 1px stroke, the inside edge sits ~1px in.
    // We want 2px of visible gap from the border, so pad 3px total.
    const innerPad = 3;
    const innerW = Math.max(0, bw - innerPad * 2);
    const innerH = Math.max(0, bh - innerPad * 2);
    g.rect(innerPad, innerPad, Math.max(0, innerW * ratio), innerH);
    g.fill(theme.control.progress.fill);
}
export function applyYogaDefaultsProgressOrMeter(yogaNode, Yoga) {
    yogaNode.setPadding(Yoga.EDGE_LEFT, 0);
    yogaNode.setPadding(Yoga.EDGE_RIGHT, 0);
    yogaNode.setPadding(Yoga.EDGE_TOP, 0);
    yogaNode.setPadding(Yoga.EDGE_BOTTOM, 0);
    // Default theme: bars are thin.
    yogaNode.setHeight(14);
    yogaNode.setMinWidth(240);
}
