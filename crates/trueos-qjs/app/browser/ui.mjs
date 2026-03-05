import * as browserContext from 'trueos:browser_context';
import * as parse5 from 'parse5';
import { INPUT_HTML } from './input_html.mjs';
import { BLOCK_TAGS } from './htmlDefaults.mjs';

export const SCROLLBAR_PAD = 6;
export const SCROLLBAR_W = 10;
export const USER_POINTER_ID = 1;
export const TRACE_POSITION_FLOW = true;
export const TRACE_YOGA_LIFECYCLE = true;
// Force direct backend mode for deterministic custom gfx rendering.
export const USE_WEBGPU_NATIVE_PAINT = false;
// Cursor-plane can overdraw with an opaque clear in bring-up; keep it off here.
export const USE_CURSOR_PLANE_TICK = !USE_WEBGPU_NATIVE_PAINT;
export const CURSOR_PLANE_TICK_MS = 50;
export const GLOBAL_SCROLL_DIRTY_KEY = '__scroll__';
export const GLOBAL_MENU_DIRTY_KEY = '__menu__';

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

export function getOrInitInputState(key, attrs) {
    const existing = uiState.inputs.get(key);
    if (existing)
        return existing;
    const state = {};
    const type = (attrs?.type ?? 'text').toLowerCase();
    if (type === 'checkbox' || type === 'radio') {
        state.checked = attrs ? Object.prototype.hasOwnProperty.call(attrs, 'checked') : false;
        if (type === 'checkbox') {
            const aria = (attrs?.['aria-checked'] ?? '').toLowerCase();
            const data = (attrs?.['data-indeterminate'] ?? '').toLowerCase();
            state.indeterminate =
                (attrs ? Object.prototype.hasOwnProperty.call(attrs, 'indeterminate') : false) ||
                    aria === 'mixed' ||
                    data === 'true' ||
                    data === '1' ||
                    data === 'yes';
        }
    }
    else {
        state.value = attrs?.value ?? '';
    }
    uiState.inputs.set(key, state);
    return state;
}

export function collectRadioGroups(root) {
    const groups = new Map();
    function walk(node) {
        if (node.kind === 'block' && node.tagName === 'input') {
            const type = (node.attrs?.type ?? 'text').toLowerCase();
            if (type === 'radio') {
                const name = node.attrs?.name ?? '__default__';
                const groupKey = `radio:${name}`;
                const key = node.key;
                if (key) {
                    const arr = groups.get(groupKey) ?? [];
                    arr.push(key);
                    groups.set(groupKey, arr);
                }
            }
        }
        for (const c of node.children)
            walk(c);
    }
    walk(root);
    return groups;
}

function isElement(node) {
    return node && typeof node === 'object' && typeof node.nodeName === 'string' && Array.isArray(node.childNodes);
}

function isText(node) {
    return node && typeof node === 'object' && node.nodeName === '#text' && typeof node.value === 'string';
}

function getBody(doc) {
    const html = (doc.childNodes ?? []).find((n) => isElement(n) && n.tagName === 'html');
    if (!html)
        return undefined;
    return (html.childNodes ?? []).find((n) => isElement(n) && n.tagName === 'body');
}

export function normalizeWhitespace(text) {
    return text.replace(/\s+/g, ' ').trim();
}

function attrsToMap(node) {
    const attrs = node?.attrs;
    if (!Array.isArray(attrs) || attrs.length === 0)
        return undefined;
    const out = {};
    for (const a of attrs) {
        if (a && typeof a.name === 'string')
            out[a.name] = String(a.value ?? '');
    }
    return Object.keys(out).length > 0 ? out : undefined;
}

function extractText(node) {
    if (isText(node))
        return node.value;
    if (!isElement(node))
        return '';
    return (node.childNodes ?? []).map(extractText).join(' ');
}

function toRenderTree(node, path = '0') {
    if (!isElement(node))
        return [];
    const out = [];
    const tagName = node.tagName ?? node.nodeName;
    const attrs = attrsToMap(node);
    if (tagName === 'textarea') {
        const value = extractText(node);
        const a = { ...(attrs ?? {}), value };
        return [{ kind: 'block', key: `${path}:${tagName}`, tagName, attrs: a, children: [] }];
    }
    if (tagName === 'input') {
        const t = String(attrs?.type ?? 'text').toLowerCase();
        if (t === 'time')
            return [{ kind: 'block', key: `${path}:input`, tagName: 'timeinput', attrs, children: [] }];
        if (t === 'month')
            return [{ kind: 'block', key: `${path}:input`, tagName: 'monthinput', attrs, children: [] }];
        if (t === 'week')
            return [{ kind: 'block', key: `${path}:input`, tagName: 'weekinput', attrs, children: [] }];
        if (t === 'date')
            return [{ kind: 'block', key: `${path}:input`, tagName: 'dateinput', attrs, children: [] }];
        if (t === 'datetime-local')
            return [{ kind: 'block', key: `${path}:input`, tagName: 'datetimelocalinput', attrs, children: [] }];
    }
    if (tagName === 'progress' || tagName === 'meter') {
        const fallbackText = normalizeWhitespace(extractText(node));
        const barAttrs = attrs;
        const barNode = {
            kind: 'block',
            key: `${path}:${tagName}`,
            tagName,
            attrs: barAttrs,
            children: [],
        };
        const rowChildren = [];
        if (fallbackText.length > 0)
            rowChildren.push({ kind: 'text', text: fallbackText });
        rowChildren.push(barNode);
        return [{ kind: 'block', key: `${path}:${tagName}-row`, tagName: 'barrow', attrs: { 'data-kind': tagName }, children: rowChildren }];
    }
    if (tagName === 'slider') {
        const sliderKey = `${path}:${tagName}`;
        const barNode = { kind: 'block', key: sliderKey, tagName, attrs, children: [] };
        const labelNode = {
            kind: 'block',
            key: `${path}:${tagName}-label`,
            tagName: 'sliderlabel',
            attrs: { 'data-slider-key': sliderKey, 'data-slider-init': String(attrs?.value ?? '') },
            children: [],
        };
        return [{ kind: 'block', key: `${path}:${tagName}-row`, tagName: 'barrow', attrs: { 'data-kind': tagName }, children: [labelNode, barNode] }];
    }
    if (tagName === 'img' || tagName === 'canvas')
        return [{ kind: 'block', key: `${path}:${tagName}`, tagName, attrs, children: [] }];
    if (tagName === 'svg') {
        const a = { ...(attrs ?? {}) };
        try {
            a['data-svg'] = parse5.serialize(node);
        }
        catch {
            a['data-svg'] = '';
        }
        return [{ kind: 'block', key: `${path}:${tagName}`, tagName, attrs: a, children: [] }];
    }
    if (tagName === 'iframe') {
        const srcdoc = String(attrs?.srcdoc ?? '');
        let children = [];
        if (srcdoc.trim().length > 0) {
            try {
                const doc = parse5.parse(srcdoc);
                const body = getBody(doc) ?? doc;
                children = toRenderTree(body, `${path}:iframe-doc`);
            }
            catch {
                children = [{ kind: 'text', text: '(iframe srcdoc parse error)' }];
            }
        }
        return [{ kind: 'block', key: `${path}:${tagName}`, tagName, attrs, children }];
    }
    if (tagName === 'select') {
        const options = [];
        let selectedIndex = 0;
        const childrenArr = Array.isArray(node.childNodes) ? node.childNodes : [];
        for (const ch of childrenArr) {
            const isOpt = ch && typeof ch === 'object' && (ch.tagName === 'option' || ch.nodeName === 'option');
            if (!isOpt)
                continue;
            const label = normalizeWhitespace(extractText(ch));
            if (label.length > 0)
                options.push(label);
            const oAttrs = ch.attrs;
            const hasSelected = Array.isArray(oAttrs) && oAttrs.some((a) => String(a?.name ?? '').toLowerCase() === 'selected');
            if (hasSelected)
                selectedIndex = Math.max(0, options.length - 1);
        }
        const joined = options.join('\n');
        const a = { ...(attrs ?? {}) };
        a['data-options'] = joined;
        a['data-selected-index'] = String(selectedIndex);
        return [{ kind: 'block', key: `${path}:${tagName}`, tagName, attrs: a, children: [] }];
    }
    if (tagName === 'number')
        return [{ kind: 'block', key: `${path}:${tagName}`, tagName, attrs, children: [] }];
    if (tagName === 'color') {
        const picker = { kind: 'block', key: `${path}:color`, tagName: 'color', attrs, children: [] };
        const mkSpin = (ch) => ({
            kind: 'block',
            key: `${path}:color-${ch}`,
            tagName: 'number',
            attrs: {
                channel: ch,
                min: '0',
                max: '255',
                step: '1',
                value: ch === 'a' ? '255' : ch === 'r' ? '255' : '0',
            },
            children: [],
        });
        const controls = {
            kind: 'block',
            key: `${path}:color-controls`,
            tagName: 'p',
            attrs: {},
            children: [mkSpin('r'), mkSpin('g'), mkSpin('b'), mkSpin('a')],
        };
        return [picker, controls];
    }
    if (tagName === 'search') {
        const inputKey = `${path}:search-input`;
        const buttonKey = `${path}:search-btn`;
        const inputAttrs = { ...(attrs ?? {}) };
        inputAttrs.type = 'text';
        const btnAttrs = { 'data-focus-key': inputKey };
        return [{
            kind: 'block',
            key: `${path}:search-row`,
            tagName: 'searchrow',
            attrs: {},
            children: [
                { kind: 'block', key: buttonKey, tagName: 'searchbutton', attrs: btnAttrs, children: [] },
                { kind: 'block', key: inputKey, tagName: 'input', attrs: inputAttrs, children: [] },
            ],
        }];
    }
    if (tagName === 'details' || tagName === 'stub') {
        const children = [];
        const detailsKey = `${path}:${tagName}`;
        const summaryEl = (node.childNodes ?? []).find((c) => isElement(c) && (c.tagName ?? c.nodeName) === 'summary');
        const summaryTextFallback = summaryEl
            ? normalizeWhitespace(extractText(summaryEl))
            : normalizeWhitespace(String(attrs?.summary ?? attrs?.title ?? '')) || 'Details';
        const buildSummaryChildren = () => {
            if (!summaryEl)
                return summaryTextFallback.length > 0 ? [{ kind: 'text', text: summaryTextFallback }] : [];
            const keep = [];
            const trailing = [];
            let inlineText = '';
            let elementIndex = 0;
            for (const ch of summaryEl.childNodes ?? []) {
                if (isText(ch)) {
                    inlineText += ch.value;
                    continue;
                }
                if (isElement(ch)) {
                    const t = ch.tagName ?? ch.nodeName;
                    const childPath = `${path}:summary.${elementIndex}`;
                    elementIndex++;
                    if (t === 'input' || t === 'button' || t === 'select' || t === 'textarea') {
                        const nodes = toRenderTree(ch, childPath);
                        const isCheckboxOrRadio = t === 'input' && (() => {
                            const a = attrsToMap(ch);
                            const typ = String(a?.type ?? 'text').toLowerCase();
                            return typ === 'checkbox' || typ === 'radio';
                        })();
                        if (isCheckboxOrRadio)
                            trailing.push(...nodes);
                        else
                            keep.push(...nodes);
                        continue;
                    }
                    inlineText += extractText(ch) + ' ';
                }
            }
            const txt = normalizeWhitespace(inlineText);
            const textNode = txt.length > 0 ? [{ kind: 'text', text: txt }] : [];
            const list = [...textNode, ...keep, ...trailing];
            return list.length > 0 ? list : summaryTextFallback.length > 0 ? [{ kind: 'text', text: summaryTextFallback }] : [];
        };
        children.push({
            kind: 'block',
            key: `${path}:summary`,
            tagName: 'summary',
            attrs: { ...(attrsToMap(summaryEl) ?? {}), 'data-details-key': detailsKey },
            children: buildSummaryChildren(),
        });
        let elementIndex = 0;
        for (const child of node.childNodes ?? []) {
            if (!isElement(child))
                continue;
            const childTag = child.tagName ?? child.nodeName;
            const childPath = `${path}.${elementIndex}`;
            elementIndex++;
            if (childTag === 'summary')
                continue;
            if (BLOCK_TAGS.has(childTag))
                children.push(...toRenderTree(child, childPath));
        }
        return [{ kind: 'block', key: detailsKey, tagName: 'details', attrs, children }];
    }
    const childBlocks = [];
    let inlineText = '';
    let elementIndex = 0;
    for (const child of node.childNodes ?? []) {
        if (isText(child)) {
            inlineText += child.value;
            continue;
        }
        if (isElement(child)) {
            const childTag = child.tagName ?? child.nodeName;
            const childPath = `${path}.${elementIndex}`;
            elementIndex++;
            if (BLOCK_TAGS.has(childTag)) {
                const t = normalizeWhitespace(inlineText);
                if (t.length > 0)
                    childBlocks.push({ kind: 'text', text: t });
                inlineText = '';
                childBlocks.push(...toRenderTree(child, childPath));
            }
            else {
                inlineText += extractText(child) + ' ';
            }
            continue;
        }
    }
    const tail = normalizeWhitespace(inlineText);
    if (tail.length > 0)
        childBlocks.push({ kind: 'text', text: tail });
    if (tagName === 'html' || tagName === 'body')
        return childBlocks;
    out.push({ kind: 'block', key: `${path}:${tagName}`, tagName, attrs, children: childBlocks });
    return out;
}

function collectHtmlElementTagCounts(node, out = new Map()) {
    if (!node || typeof node !== 'object')
        return out;
    if (isElement(node)) {
        const tag = String(node.tagName ?? node.nodeName ?? '').toLowerCase();
        if (tag.length > 0)
            out.set(tag, (out.get(tag) ?? 0) + 1);
        const kids = Array.isArray(node.childNodes) ? node.childNodes : [];
        for (let i = 0; i < kids.length; i++)
            collectHtmlElementTagCounts(kids[i], out);
        return out;
    }
    const kids = Array.isArray(node.childNodes) ? node.childNodes : [];
    for (let i = 0; i < kids.length; i++)
        collectHtmlElementTagCounts(kids[i], out);
    return out;
}

function collectRenderBlockTagCounts(nodes, out = new Map()) {
    const list = Array.isArray(nodes) ? nodes : [];
    for (let i = 0; i < list.length; i++) {
        const n = list[i];
        if (!n || typeof n !== 'object')
            continue;
        if (n.kind === 'block') {
            const tag = String(n.tagName ?? '').toLowerCase();
            if (tag.length > 0)
                out.set(tag, (out.get(tag) ?? 0) + 1);
        }
        collectRenderBlockTagCounts(n.children, out);
    }
    return out;
}

function sourceTagCoveredByRender(tag, renderCounts) {
    const has = (t) => (renderCounts.get(t) ?? 0) > 0;
    switch (tag) {
        case 'search':
            return has('searchrow') || has('searchbutton') || has('input');
        case 'input':
            return has('input') || has('timeinput') || has('dateinput') || has('monthinput') || has('weekinput') || has('datetimelocalinput');
        case 'stub':
            return has('details');
        default:
            return has(tag);
    }
}

function logRichHtmlCoverage(body, renderNodes) {
    const sourceCounts = collectHtmlElementTagCounts(body, new Map());
    const renderCounts = collectRenderBlockTagCounts(renderNodes, new Map());
    const tracked = ['details', 'summary', 'input', 'textarea', 'select', 'progress', 'meter', 'slider', 'number', 'color', 'search', 'dialog', 'canvas', 'svg', 'iframe'];
    const missing = [];
    for (let i = 0; i < tracked.length; i++) {
        const tag = tracked[i];
        const srcN = sourceCounts.get(tag) ?? 0;
        if (srcN <= 0)
            continue;
        if (!sourceTagCoveredByRender(tag, renderCounts))
            missing.push(tag);
    }
    const sourceTotal = Array.from(sourceCounts.values()).reduce((a, b) => a + b, 0);
    const renderTotal = Array.from(renderCounts.values()).reduce((a, b) => a + b, 0);
    try {
        console.log(`[richui-coverage] sourceElems=${sourceTotal} renderBlocks=${renderTotal} trackedMissing=${missing.length ? missing.join(',') : 'none'}`);
    }
    catch {
        // Keep diagnostics non-fatal.
    }
}

export function buildDefaultRenderNodes() {
    const html = INPUT_HTML;
    try {
        const isEmptyBringup = html.includes('<body></body>');
        const marker = isEmptyBringup
            ? 'bringup-empty'
            : (html.includes('Hello world') ? 'bringup-simple' : 'rich');
        console.log(`[richui-html] len=${html.length} marker=${marker}`);
    }
    catch {
        // Debug logging should never affect startup.
    }
    const doc = parse5.parse(html);
    const body = getBody(doc) ?? doc;
    const innerRenderNodes = toRenderTree(body, '0');
    logRichHtmlCoverage(body, innerRenderNodes);
    const renderNodes = [
        {
            kind: 'block',
            key: 'root:internal-iframe',
            tagName: 'iframe',
            attrs: { 'data-root': '1' },
            children: innerRenderNodes,
        },
    ];
    return { renderNodes, innerRenderNodes };
}
