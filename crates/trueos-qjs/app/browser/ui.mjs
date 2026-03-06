import * as browserContext from 'trueos:browser_context';

export const CURSOR_PLANE_TICK_MS = 50;
export const USER_POINTER_ID = 1;

// Keep in sync with cmd_backend direct-cursor defaults.
export const DEFAULT_FOUR_CURSORS = [
  { id: 1, color: 0x111111, posX: 0.31*1280, posY: 0.58*800 },
  { id: 2, color: 0x2563eb, posX: 0.36*1280, posY: 0.54*800 },
  { id: 3, color: 0x16a34a, posX: 0.42*1280, posY: 0.62*800 },
  { id: 4, color: 0xdc2626, posX: 0.47*1280, posY: 0.57*800 },
];

export const cursorPlaneState = {
  focusedKeyByPointer: new Map(),
  keyboardOwnerPointerId: USER_POINTER_ID,
  cursorColors: new Map(),
  primaryMousePointerId: USER_POINTER_ID,
  userCursorPos: new Map(),
  lastMouse: { x: 0, y: 0, has: false },
  hoverRects: [],
  hoverHandlers: new Map(),
  hoveredKeyByPointer: new Map(),
  hoveredCursorByPointer: new Map(),
  kernelButtonsByPointer: new Map(),
  hoverTiltByPointer: new Map(),
};

// Derive a lightweight card-tilt transform from cursor position over a rect.
export function computeHoverTilt(x, y, w, h, cursorX, cursorY, maxTiltDeg = 6) {
  const ww = Math.max(1, Number(w) || 0);
  const hh = Math.max(1, Number(h) || 0);
  const cx = Number(x) + ww * 0.5;
  const cy = Number(y) + hh * 0.5;
  const nx = ((Number(cursorX) - cx) / (ww * 0.5));
  const ny = ((Number(cursorY) - cy) / (hh * 0.5));
  const clampedX = Math.max(-1, Math.min(1, nx));
  const clampedY = Math.max(-1, Math.min(1, ny));
  const tiltX = -clampedY * maxTiltDeg;
  const tiltY = clampedX * maxTiltDeg;
  return { tiltX, tiltY };
}

export function setHoverTilt(pointerId, x, y, w, h, cursorX, cursorY, maxTiltDeg = 6) {
  const tilt = computeHoverTilt(x, y, w, h, cursorX, cursorY, maxTiltDeg);
  cursorPlaneState.hoverTiltByPointer.set(Number(pointerId) || 0, tilt);
  return tilt;
}

export function clearHoverTilt(pointerId) {
  cursorPlaneState.hoverTiltByPointer.delete(Number(pointerId) || 0);
}

export function getCursorColor(pointerId) {
  const existing = cursorPlaneState.cursorColors.get(pointerId);
  if (existing != null) return existing;

  const palette = [0x111111, 0x2563eb, 0x16a34a, 0xdc2626, 0x7c3aed, 0x0ea5e9, 0xf59e0b];
  const idx = Math.abs(Number(pointerId) || 0) % palette.length;
  const color = palette[idx];
  cursorPlaneState.cursorColors.set(pointerId, color);
  return color;
}

export function getEffectivePointerId(ev) {
  const actual = Number(ev?.pointerId ?? ev?.data?.pointerId ?? 0);
  if (actual > 0) {
    try {
      browserContext.setActiveCursor(actual);
    } catch {
      // Keep routing resilient if native cursor context is unavailable.
    }
    return actual;
  }

  try {
    return Number(browserContext.getActiveCursor?.() ?? 0) | 0;
  } catch {
    return actual;
  }
}

export function getMenuOwnerPointerId(ev, fallbackPid = 0) {
  const raw = Number(fallbackPid || getEffectivePointerId(ev) || 0) | 0;
  if (raw >= 1 && raw <= 4) return raw;

  const pt = String(ev?.pointerType ?? ev?.data?.pointerType ?? '').toLowerCase();
  if (pt === 'mouse') return USER_POINTER_ID;

  try {
    const active = Number(browserContext.getActiveCursor?.() ?? 0) | 0;
    if (active >= 1 && active <= 4) return active;
  } catch {
    // Keep fallback path resilient when browser_context is unavailable.
  }
  return USER_POINTER_ID;
}

export function logCursorButtonEvent(kind, pid, button, x, y) {
  try {
    console.log(
      `[cursor-btn] ${kind} pid=${Number(pid || 0) | 0} button=${Number(button || 0) | 0} x=${Math.round(Number(x || 0))} y=${Math.round(Number(y || 0))}`
    );
  } catch {
    // Keep input path resilient if logging fails.
  }
}
