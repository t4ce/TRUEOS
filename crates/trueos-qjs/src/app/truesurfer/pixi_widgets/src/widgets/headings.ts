export function isHeadingTag(tagName?: string): boolean {
  const tag = String(tagName ?? '').toLowerCase();
  if (tag.length !== 2 || tag.charAt(0) !== 'h') return false;
  const n = tag.charCodeAt(1);
  return n >= 49 && n <= 54;
}

export function applyYogaDefaultsHeading(yogaNode: any, Yoga: any): void {
  yogaNode.setPadding(Yoga.EDGE_TOP, 6);
  yogaNode.setPadding(Yoga.EDGE_BOTTOM, 6);
  yogaNode.setMinHeight(36);
  yogaNode.setJustifyContent(Yoga.JUSTIFY_CENTER);
}
