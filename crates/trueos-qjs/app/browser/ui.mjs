import * as browserContext from 'trueos:browser_context';

export const SCROLLBAR_PAD = 6;
export const SCROLLBAR_W = 10;
export const USER_POINTER_ID = 1;
export const USE_DIRECT_CMD_BACKEND = true;
export const TRACE_POSITION_FLOW = true;
export const TRACE_YOGA_LIFECYCLE = true;
export const USE_CURSOR_PLANE_TICK = true;
export const CURSOR_PLANE_TICK_MS = 50;
export const GLOBAL_SCROLL_DIRTY_KEY = '__scroll__';

export const RT_GLOBAL = globalThis;
export const RT_WINDOW = (typeof window === 'object' && window)
    ? window
    : ((typeof RT_GLOBAL.window === 'object' && RT_GLOBAL.window) ? RT_GLOBAL.window : RT_GLOBAL);
export const RT_DOCUMENT = (typeof document === 'object' && document)
    ? document
    : (RT_WINDOW?.document ?? null);
export const RT_HAS_WINDOW_RESIZE = !!(RT_WINDOW
    && typeof RT_WINDOW.addEventListener === 'function'
    && typeof RT_WINDOW.removeEventListener === 'function');
export const RT_EVENT_TARGET = RT_HAS_WINDOW_RESIZE
    ? RT_WINDOW
    : ((RT_GLOBAL && typeof RT_GLOBAL.addEventListener === 'function') ? RT_GLOBAL : null);
export const IS_TRUEOS_QJS_RUNTIME = !!(RT_GLOBAL && (RT_GLOBAL.__trueosWebGpuState || RT_GLOBAL.__trueosCmdStream));

export const uiState = {
    // Per-pointer focus (so multiple cursors can have focused fields at once).
    // Keyboard input is routed to keyboardOwnerPointerId (last cursor to click a field).
    focusedKeyByPointer: new Map(),
    keyboardOwnerPointerId: 1,
    inputs: new Map(),
    sliders: new Map(),
    sliderDrags: new Map(),
    sliderBounds: new Map(),
    dialogs: new Map(),
    dialogDrags: new Map(),
    dialogSelectedBy: new Map(),
    dialogZ: new Map(),
    dialogZCounter: 1,
    numbers: new Map(),
    // Pointer-hold repeat for <number> spinners.
    numberHolds: new Map(),
    selects: new Map(),
    // Temporal inputs: <input type=time|date|month|week|datetime-local>
    temporals: new Map(),
    // yearSliderKey -> temporal input key (so we can close year widget on slider release).
    temporalYearOwners: new Map(),
    // Single shared color (for now): the <color> picker updates these, and <number channel=r|g|b>
    // edits them.
    color: {
        rgb: { r: 255, g: 0, b: 0 },
        a: 255,
        pick: null,
        draggingPointerId: null,
        // Absolute bounds (in stage coordinates) of the last rendered <color> widget.
        bounds: null,
    },
    // Cursor colors (per pointerId). Used for cursor cross and selection border color.
    cursorColors: new Map(),
    primaryMousePointerId: 1,
    // Stored positions for user cursors.
    userCursorPos: new Map(),
    lastMouse: { x: 0, y: 0, has: false },
    scroll: {
        y: 0,
        contentHeight: 0,
        viewportHeight: 0,
        draggingPointerId: null,
        dragOffsetY: 0,
        // Updated each paint.
        track: { x: 0, y: 0, w: SCROLLBAR_W, h: 0 },
        thumb: { x: 0, y: 0, w: SCROLLBAR_W, h: 0 },
    },
    // Per-iframe scroll state (keyed by iframe LayoutBox key).
    iframeScroll: new Map(),
    // Frame-ordered iframe rects for event routing (deepest wins by iterating from end).
    iframeRects: [],
    // Hover simulation for non-mouse cursors (virtual/AI pointers).
    hoverRects: [],
    hoverHandlers: new Map(),
    hoveredKeyByPointer: new Map(),
    hoveredCursorByPointer: new Map(),
    kernelButtonsByPointer: new Map(),
    // Drag-selection for text-like <input> and <textarea>.
    textDrags: new Map(),
    // Per-frame bounds for text-like fields, used for drag selection.
    fieldBounds: new Map(),
    // Per-frame clamp bounds for dragging dialogs (keyed by dialog key).
    // Bounds are expressed in the coordinate space the dialog is drawn into.
    dialogDragBounds: new Map(),
    detailsOpen: new Map(),
};

export function getCursorColor(pointerId) {
    const existing = uiState.cursorColors.get(pointerId);
    if (existing != null)
        return existing;
    // Simple palette; stable assignment per pointerId.
    const palette = [0x111111, 0x2563eb, 0x16a34a, 0xdc2626, 0x7c3aed, 0x0ea5e9, 0xf59e0b];
    const idx = Math.abs(Number(pointerId) || 0) % palette.length;
    const col = palette[idx];
    uiState.cursorColors.set(pointerId, col);
    return col;
}

export function getEffectivePointerId(ev) {
    const actual = Number(ev?.pointerId ?? ev?.data?.pointerId ?? 0);
    if (actual > 0) {
        try {
            browserContext.setActiveCursor(actual);
        }
        catch {
            // Keep pointer-id routing resilient if native module is unavailable.
        }
        return actual;
    }
    try {
        return Number(browserContext.getActiveCursor() ?? 0) | 0;
    }
    catch {
        return actual;
    }
}

export function getMenuOwnerPointerId(ev, fallbackPid = 0) {
    const raw = Number(fallbackPid || getEffectivePointerId(ev) || 0) | 0;
    if (raw >= 1 && raw <= 4)
        return raw;
    const pt = String(ev?.pointerType ?? ev?.data?.pointerType ?? '').toLowerCase();
    if (pt === 'mouse')
        return USER_POINTER_ID;
    try {
        const active = Number(browserContext.getActiveCursor?.() ?? 0) | 0;
        if (active >= 1 && active <= 4)
            return active;
    }
    catch {
        // Keep fallback path resilient when browser_context isn't available.
    }
    return USER_POINTER_ID;
}

export function logCursorButtonEvent(kind, pid, button, x, y) {
    try {
        console.log(`[cursor-btn] ${kind} pid=${Number(pid || 0) | 0} button=${Number(button || 0) | 0} x=${Math.round(Number(x || 0))} y=${Math.round(Number(y || 0))}`);
    }
    catch {
        // Keep input path resilient if logging fails.
    }
}

export function computeScrollableContentHeight(root) {
    let max = 0;
    const walk = (n, ax, ay) => {
        const nx = ax + n.x;
        const ny = ay + n.y;
        // Ignore floating dialogs; they are viewport overlay widgets.
        if (n.kind === 'block' && n.tagName === 'dialog')
            return;
        max = Math.max(max, ny + n.height);
        for (const c of n.children ?? [])
            walk(c, nx, ny);
    };
    for (const c of root.children ?? [])
        walk(c, 0, 0);
    return max;
}

export function countLayoutNodes(root) {
    if (!root)
        return 0;
    let n = 1;
    const kids = Array.isArray(root.children) ? root.children : [];
    for (let i = 0; i < kids.length; i++) {
        n += countLayoutNodes(kids[i]);
    }
    return n;
}
