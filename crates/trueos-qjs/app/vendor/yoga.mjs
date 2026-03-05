// Non-wasm Yoga adapter.
// Exposes a yoga-layout-like API surface while delegating to trueos:yoga.

import * as native from 'trueos:yoga';

const toF32 = (v) => {
  const n = Number(v);
  return Number.isFinite(n) ? n : 0;
};

const toI32 = (v) => {
  const n = Number(v);
  return Number.isFinite(n) ? (n | 0) : 0;
};

const toU32 = (v) => {
  const n = Number(v);
  if (!Number.isFinite(n) || n <= 0) return 0;
  return n >>> 0;
};

export class Config {
  constructor(handle) {
    this._h = toU32(handle);
  }

  static create() {
    return new Config(native.configCreate?.() ?? 0);
  }

  free() {
    native.configFree?.(this._h);
    this._h = 0;
  }

  setUseWebDefaults(enabled) {
    native.configSetUseWebDefaults?.(this._h, enabled ? 1 : 0);
    return this;
  }
}

export class Node {
  constructor(handle) {
    this._h = toU32(handle);
    this._measure = null;
  }

  static create(config) {
    const cfg = config instanceof Config ? config._h : 0;
    return new Node(native.nodeCreate?.(cfg) ?? 0);
  }

  freeRecursive() {
    native.nodeFreeRecursive?.(this._h);
    this._h = 0;
  }

  insertChild(child, index) {
    native.nodeInsertChild?.(this._h, child?._h ?? 0, toU32(index));
  }

  getChildCount() {
    return toU32(native.nodeGetChildCount?.(this._h) ?? 0);
  }

  setFlexDirection(v) { native.nodeSetFlexDirection?.(this._h, toI32(v)); }
  setAlignItems(v) { native.nodeSetAlignItems?.(this._h, toI32(v)); }
  setAlignSelf(v) { native.nodeSetAlignSelf?.(this._h, toI32(v)); }
  setJustifyContent(v) { native.nodeSetJustifyContent?.(this._h, toI32(v)); }
  setFlexWrap(v) { native.nodeSetFlexWrap?.(this._h, toI32(v)); }
  setFlexGrow(v) { native.nodeSetFlexGrow?.(this._h, toF32(v)); }
  setFlexShrink(v) { native.nodeSetFlexShrink?.(this._h, toF32(v)); }
  setPositionType(v) { native.nodeSetPositionType?.(this._h, toI32(v)); }

  setWidth(v) { native.nodeSetWidth?.(this._h, toF32(v)); }
  setHeight(v) { native.nodeSetHeight?.(this._h, toF32(v)); }
  setMinWidth(v) { native.nodeSetMinWidth?.(this._h, toF32(v)); }
  setMinHeight(v) { native.nodeSetMinHeight?.(this._h, toF32(v)); }

  setPadding(edge, v) { native.nodeSetPadding?.(this._h, toI32(edge), toF32(v)); }
  setMargin(edge, v) { native.nodeSetMargin?.(this._h, toI32(edge), toF32(v)); }
  setPosition(edge, v) { native.nodeSetPosition?.(this._h, toI32(edge), toF32(v)); }

  setMeasureFunc(fn) {
    this._measure = typeof fn === 'function' ? fn : null;
    if (this._measure) {
      try {
        const m = this._measure(NaN, native.MEASURE_MODE_UNDEFINED ?? 0, NaN, native.MEASURE_MODE_UNDEFINED ?? 0);
        if (m && Number.isFinite(m.width)) this.setWidth(m.width);
        if (m && Number.isFinite(m.height)) this.setHeight(m.height);
      } catch (_) {}
    }
  }

  calculateLayout(width, height, direction) {
    native.nodeCalculateLayout?.(this._h, toF32(width), toF32(height), toI32(direction));
  }

  getComputedLeft() { return toF32(native.nodeGetComputedLeft?.(this._h) ?? 0); }
  getComputedTop() { return toF32(native.nodeGetComputedTop?.(this._h) ?? 0); }
  getComputedWidth() { return toF32(native.nodeGetComputedWidth?.(this._h) ?? 0); }
  getComputedHeight() { return toF32(native.nodeGetComputedHeight?.(this._h) ?? 0); }
}

export const ALIGN_AUTO = native.ALIGN_AUTO ?? 0;
export const ALIGN_FLEX_START = native.ALIGN_FLEX_START ?? 1;
export const ALIGN_CENTER = native.ALIGN_CENTER ?? 2;
export const ALIGN_FLEX_END = native.ALIGN_FLEX_END ?? 3;
export const ALIGN_STRETCH = native.ALIGN_STRETCH ?? 4;

export const JUSTIFY_FLEX_START = native.JUSTIFY_FLEX_START ?? 0;
export const JUSTIFY_CENTER = native.JUSTIFY_CENTER ?? 1;
export const JUSTIFY_SPACE_BETWEEN = native.JUSTIFY_SPACE_BETWEEN ?? 3;

export const FLEX_DIRECTION_COLUMN = native.FLEX_DIRECTION_COLUMN ?? 0;
export const FLEX_DIRECTION_ROW = native.FLEX_DIRECTION_ROW ?? 2;

export const WRAP_NO_WRAP = native.WRAP_NO_WRAP ?? 0;
export const WRAP_WRAP = native.WRAP_WRAP ?? 1;

export const POSITION_TYPE_RELATIVE = native.POSITION_TYPE_RELATIVE ?? 1;
export const POSITION_TYPE_ABSOLUTE = native.POSITION_TYPE_ABSOLUTE ?? 2;

export const EDGE_LEFT = native.EDGE_LEFT ?? 0;
export const EDGE_TOP = native.EDGE_TOP ?? 1;
export const EDGE_RIGHT = native.EDGE_RIGHT ?? 2;
export const EDGE_BOTTOM = native.EDGE_BOTTOM ?? 3;

export const DIRECTION_LTR = native.DIRECTION_LTR ?? 1;
export const MEASURE_MODE_UNDEFINED = native.MEASURE_MODE_UNDEFINED ?? 0;

const Yoga = {
  Node,
  Config,
  ALIGN_AUTO,
  ALIGN_FLEX_START,
  ALIGN_CENTER,
  ALIGN_FLEX_END,
  ALIGN_STRETCH,
  JUSTIFY_FLEX_START,
  JUSTIFY_CENTER,
  JUSTIFY_SPACE_BETWEEN,
  FLEX_DIRECTION_COLUMN,
  FLEX_DIRECTION_ROW,
  WRAP_NO_WRAP,
  WRAP_WRAP,
  POSITION_TYPE_RELATIVE,
  POSITION_TYPE_ABSOLUTE,
  EDGE_LEFT,
  EDGE_TOP,
  EDGE_RIGHT,
  EDGE_BOTTOM,
  DIRECTION_LTR,
  MEASURE_MODE_UNDEFINED,
};

export default Yoga;
