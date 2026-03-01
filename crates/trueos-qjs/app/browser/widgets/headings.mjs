export function isHeadingTag(tagName) {
    return /^h[1-6]$/.test(tagName ?? '');
}
export function applyYogaDefaultsHeading(yogaNode, Yoga) {
    yogaNode.setPadding(Yoga.EDGE_TOP, 6);
    yogaNode.setPadding(Yoga.EDGE_BOTTOM, 6);
    yogaNode.setMinHeight(36);
    yogaNode.setJustifyContent(Yoga.JUSTIFY_CENTER);
}
