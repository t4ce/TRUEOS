export function isHeadingTag(tagName?: string): boolean {
  return /^h[1-6]$/.test(tagName ?? '');
}

export function applyYogaDefaultsHeading(yogaNode: any, Yoga: any): void {
  yogaNode.setPadding(Yoga.EDGE_TOP, 6);
  yogaNode.setPadding(Yoga.EDGE_BOTTOM, 6);
  yogaNode.setMinHeight(36);
  yogaNode.setJustifyContent(Yoga.JUSTIFY_CENTER);
}
