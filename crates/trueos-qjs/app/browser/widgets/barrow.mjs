export function applyYogaDefaultsBarrow(yogaNode, Yoga) {
    yogaNode.setFlexDirection(Yoga.FLEX_DIRECTION_ROW);
    yogaNode.setAlignItems(Yoga.ALIGN_CENTER);
    yogaNode.setJustifyContent(Yoga.JUSTIFY_FLEX_START);
    // Same left inset used elsewhere so text doesn't touch/clamp against borders.
    yogaNode.setPadding(Yoga.EDGE_LEFT, 8);
    yogaNode.setPadding(Yoga.EDGE_RIGHT, 0);
    yogaNode.setPadding(Yoga.EDGE_TOP, 0);
    yogaNode.setPadding(Yoga.EDGE_BOTTOM, 0);
}
