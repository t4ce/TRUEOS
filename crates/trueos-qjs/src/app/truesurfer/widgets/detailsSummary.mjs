export function detailsSummarySceneStyle(open) {
  return {
    arrowOpen: open ? 1 : 0,
    arrowSize: 16,
    arrowPad: 3,
    arrowStroke: 2,
    textInset: 26,
    minHeight: 36,
  };
}

export function detailsIsOpen(attrs) {
  return !!attrs && Object.prototype.hasOwnProperty.call(attrs, 'open');
}

export function summaryLayoutDefaults() {
  return {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    paddingTop: 6,
    paddingBottom: 6,
    paddingLeft: 26,
    paddingRight: 12,
    minHeight: 36,
  };
}
