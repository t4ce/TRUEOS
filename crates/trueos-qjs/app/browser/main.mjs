import { Application, Container, Graphics } from 'pixi.js';
import * as parse5 from 'parse5';
import Yoga from 'yoga-layout';
import { defaultTheme } from './theme.mjs';
import { BLOCK_TAGS } from './htmlDefaults.mjs';
// SVG generation/parsing helpers live in widget modules.
import { makeThemedText, TEXT_BASELINE_NUDGE_Y, WRAP_EPSILON_PX } from './text.mjs';
import { clearGraphics, getOrCreateContainer, getOrCreateGraphics, getOrCreateText } from './pixiReuse.mjs';
import { clampWrappedLines, getCaretIndexFromPoint, wrapFieldTextWithIndices } from './widgets/textField.mjs';
import { renderProgressOrMeter } from './widgets/progressMeter.mjs';
import { applyYogaDefaultsProgressOrMeter } from './widgets/progressMeter.mjs';
import { applyYogaDefaultsSlider, createYogaNodeForSliderLabel, renderSlider, renderSliderLabel, getOrInitSliderState as widgetGetOrInitSliderState, } from './widgets/slider.mjs';
import { getEffectiveDetailsChildren, renderSummary } from './widgets/detailsSummary.mjs';
import { applyYogaDefaultsDetails, applyYogaDefaultsSummary } from './widgets/detailsSummary.mjs';
import { renderHr } from './widgets/hr.mjs';
import { applyYogaDefaultsHr } from './widgets/hr.mjs';
import { renderButton } from './widgets/button.mjs';
import { applyYogaDefaultsButton } from './widgets/button.mjs';
import { renderCell, renderTable } from './widgets/table.mjs';
import { applyYogaDefaultsCell, applyYogaDefaultsTable, applyYogaDefaultsTr } from './widgets/table.mjs';
import { isHeadingTag } from './widgets/headings.mjs';
import { applyYogaDefaultsHeading } from './widgets/headings.mjs';
import { renderImg } from './widgets/img.mjs';
import { applyYogaDefaultsImg } from './widgets/img.mjs';
import { applyYogaDefaultsSvg, renderSvgElement } from './widgets/svgElement.mjs';
import { applyYogaDefaultsCanvas, renderCanvasElement } from './widgets/canvasElement.mjs';
import { applyYogaDefaultsIframe, renderIframePlaceholder } from './widgets/iframe.mjs';
import { applyYogaDefaultsInput, renderInput } from './widgets/input.mjs';
import { renderTextarea } from './widgets/textarea.mjs';
import { applyYogaDefaultsTextarea } from './widgets/textarea.mjs';
import { applyYogaDefaultsBarrow } from './widgets/barrow.mjs';
import { applyYogaDefaultsSearchButton, applyYogaDefaultsSearchRow, renderSearchButton } from './widgets/search.mjs';
import { applyYogaDefaultsDialog, getOrInitDialogState, renderDialog } from './widgets/dialog.mjs';
import { applyYogaDefaultsNumber, getOrInitNumberState, renderNumberSpinner } from './widgets/number.mjs';
import { applyYogaDefaultsColor, renderColorPicker, sampleColorPickerAtLocal } from './widgets/color.mjs';
import { applyYogaDefaultsSelect, getOrInitSelectState, renderSelect, renderSelectPopup } from './widgets/select.mjs';
import { applyYogaDefaultsTemporalInput, closeAllTemporalPopups, renderTemporalInput, renderTemporalPopups, } from './widgets/temporal.mjs';
const SCROLLBAR_PAD = 6;
const SCROLLBAR_W = 10;
const USER_POINTER_ID = 1;
const USER_POINTER_ID_3 = 3;
const USER_POINTER_ID_4 = 4;
const uiState = {
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
    // Multi-cursor harness: lets you drive pointerId 1 or 3 using the real mouse,
    // cycling control every few seconds to stress-test the "last cursor wins" logic.
    harness: {
        enabled: true,
        activeUserPointerId: USER_POINTER_ID,
        periodMs: 3000,
    },
    // Stored positions for user cursors (so cursor 1 and cursor 3 can diverge).
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
    virtualCursor: {
        enabled: false,
        x: 0,
        y: 0,
        t: 0,
        radius: 120,
        speed: 0.9,
    },
    // Drag-selection for text-like <input> and <textarea>.
    textDrags: new Map(),
    // Per-frame bounds for text-like fields, used for drag selection.
    fieldBounds: new Map(),
    // Per-frame clamp bounds for dragging dialogs (keyed by dialog key).
    // Bounds are expressed in the coordinate space the dialog is drawn into.
    dialogDragBounds: new Map(),
    detailsOpen: new Map(),
    // One context menu per pointerId.
    contextMenus: new Map(),
    // Per-pointer clipboard (used by context menu Copy/Paste).
    clipboards: new Map(),
};
// Singleton canvas/context for text measurement during rendering (used by inputs/textarea).
let renderMeasureCtx = null;
function getRenderMeasure(theme) {
    if (!renderMeasureCtx) {
        const c = document.createElement('canvas');
        const ctx = c.getContext('2d');
        if (!ctx)
            throw new Error('2D canvas not available');
        renderMeasureCtx = ctx;
    }
    renderMeasureCtx.font = `${theme.fontSize}px ${theme.fontFamily}`;
    return (s) => renderMeasureCtx.measureText(s).width;
}
// Retained-mode: cache LayoutBox containers per scene root so we can update in place.
const retainedNodeCache = new WeakMap();
function wouldCreateCycle(parent, child) {
    // Adding `child` under `parent` would create a cycle if `parent` is already in
    // the ancestry chain of `child`.
    let p = parent;
    while (p) {
        if (p === child)
            return true;
        p = p.parent;
    }
    return false;
}
function ensureChildAt(parent, child, index) {
    // Guard against accidental cycles; adding a container to itself blows the stack.
    if (child === parent)
        return;
    if (wouldCreateCycle(parent, child))
        return;
    if (child.parent !== parent) {
        // addChildAt allows inserting at the end (index == children.length).
        const insertAt = Math.max(0, Math.min(index, parent.children.length));
        parent.addChildAt(child, insertAt);
        return;
    }
    // setChildIndex requires 0..children.length-1 (end is length-1, not length).
    const max = Math.max(0, parent.children.length - 1);
    const target = Math.max(0, Math.min(index, max));
    const cur = parent.getChildIndex(child);
    if (cur !== target)
        parent.setChildIndex(child, target);
}
function ensureChildAtAny(parent, child, index) {
    // Same as ensureChildAt but for Graphics/Text/Mesh.
    if (child === parent)
        return;
    if (wouldCreateCycle(parent, child))
        return;
    if (child.parent !== parent) {
        const insertAt = Math.max(0, Math.min(index, parent.children.length));
        parent.addChildAt(child, insertAt);
        return;
    }
    const max = Math.max(0, parent.children.length - 1);
    const target = Math.max(0, Math.min(index, max));
    const cur = parent.getChildIndex(child);
    if (cur !== target)
        parent.setChildIndex(child, target);
}
let requestRerender = null;
let requestPaint = null;
function getCursorColor(pointerId) {
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
function getEffectivePointerId(ev) {
    const actual = Number(ev?.pointerId ?? ev?.data?.pointerId ?? 0);
    const pt = String(ev?.pointerType ?? ev?.data?.pointerType ?? '').toLowerCase();
    const isMouse = pt === 'mouse' || actual === 1 || actual === uiState.primaryMousePointerId;
    if (uiState.harness.enabled && isMouse) {
        return uiState.harness.activeUserPointerId;
    }
    return actual;
}
function computeScrollableContentHeight(root) {
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
function getOrInitInputState(key, attrs) {
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
function collectRadioGroups(root) {
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
function normalizeWhitespace(text) {
    return text.replace(/\s+/g, ' ').trim();
}
function toRenderTree(node, path = '0') {
    if (!isElement(node))
        return [];
    const out = [];
    const tagName = node.tagName ?? node.nodeName;
    const attrs = attrsToMap(node);
    // Treat textarea as a leaf control: its text content becomes its value.
    if (tagName === 'textarea') {
        const value = extractText(node);
        const a = { ...(attrs ?? {}), value };
        return [{ kind: 'block', key: `${path}:${tagName}`, tagName, attrs: a, children: [] }];
    }
    // Temporal <input> types are remapped into custom leaf widgets.
    // This keeps the authoring surface as "natural HTML" while letting us implement
    // richer picker UIs than the generic text-like <input> renderer.
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
    // Treat progress/meter as leaf controls: their inner text is fallback content.
    if (tagName === 'progress' || tagName === 'meter') {
        const fallbackText = normalizeWhitespace(extractText(node));
        // Render as a row: [label] [bar]
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
        return [
            {
                kind: 'block',
                key: `${path}:${tagName}-row`,
                tagName: 'barrow',
                attrs: { 'data-kind': tagName },
                children: rowChildren,
            },
        ];
    }
    // Demo widget: slider renders as a row: [value label] [bar]
    if (tagName === 'slider') {
        const sliderKey = `${path}:${tagName}`;
        const barNode = {
            kind: 'block',
            key: sliderKey,
            tagName,
            attrs,
            children: [],
        };
        const labelNode = {
            kind: 'block',
            key: `${path}:${tagName}-label`,
            tagName: 'sliderlabel',
            attrs: { 'data-slider-key': sliderKey, 'data-slider-init': String(attrs?.value ?? '') },
            children: [],
        };
        return [
            {
                kind: 'block',
                key: `${path}:${tagName}-row`,
                tagName: 'barrow',
                attrs: { 'data-kind': tagName },
                children: [labelNode, barNode],
            },
        ];
    }
    // Treat img as a leaf node (replaced element).
    if (tagName === 'img') {
        return [{ kind: 'block', key: `${path}:${tagName}`, tagName, attrs, children: [] }];
    }
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
    if (tagName === 'canvas') {
        return [{ kind: 'block', key: `${path}:${tagName}`, tagName, attrs, children: [] }];
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
    // Demo widgets: leaf controls.
    if (tagName === 'number') {
        return [{ kind: 'block', key: `${path}:${tagName}`, tagName, attrs, children: [] }];
    }
    // Composite widget: <color> expands into the picker + 4 internal channel spinners (RGBA).
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
    // Composite widget: <search> becomes [icon button][input].
    if (tagName === 'search') {
        const inputKey = `${path}:search-input`;
        const buttonKey = `${path}:search-btn`;
        const inputAttrs = { ...(attrs ?? {}) };
        // Force text-like input semantics.
        inputAttrs.type = 'text';
        const btnAttrs = {
            'data-focus-key': inputKey,
        };
        return [
            {
                kind: 'block',
                key: `${path}:search-row`,
                tagName: 'searchrow',
                attrs: {},
                children: [
                    { kind: 'block', key: buttonKey, tagName: 'searchbutton', attrs: btnAttrs, children: [] },
                    { kind: 'block', key: inputKey, tagName: 'input', attrs: inputAttrs, children: [] },
                ],
            },
        ];
    }
    // Treat details specially so we can hide/show its content and draw a custom disclosure arrow.
    // Also support <stub> as an alias for <details> (handy while iterating on markup).
    if (tagName === 'details' || tagName === 'stub') {
        const children = [];
        const detailsKey = `${path}:${tagName}`;
        // Find the first <summary>.
        const summaryEl = (node.childNodes ?? []).find((c) => isElement(c) && (c.tagName ?? c.nodeName) === 'summary');
        const summaryTextFallback = summaryEl
            ? normalizeWhitespace(extractText(summaryEl))
            : normalizeWhitespace(String(attrs?.summary ?? attrs?.title ?? '')) || 'Details';
        // Preserve interactive children (like <input type=checkbox>) inside <summary>.
        // We also intentionally reorder checkbox/radio controls to the end so they end up
        // visually on the right (applyYogaDefaultsSummary uses space-between).
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
                    // If it's a control element, keep it as a block child so it gets rendered.
                    if (t === 'input' || t === 'button' || t === 'select' || t === 'textarea') {
                        const nodes = toRenderTree(ch, childPath);
                        const isCheckboxOrRadio = t === 'input' &&
                            (() => {
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
                    // Non-control content: treat its text as part of the label.
                    inlineText += extractText(ch) + ' ';
                }
            }
            const txt = normalizeWhitespace(inlineText);
            const textNode = txt.length > 0 ? [{ kind: 'text', text: txt }] : [];
            // Default: label text first, then controls. Checkbox/radio are forced to the end.
            const out = [...textNode, ...keep, ...trailing];
            return out.length > 0 ? out : summaryTextFallback.length > 0 ? [{ kind: 'text', text: summaryTextFallback }] : [];
        };
        children.push({
            kind: 'block',
            key: `${path}:summary`,
            tagName: 'summary',
            attrs: { ...(attrsToMap(summaryEl) ?? {}), 'data-details-key': detailsKey },
            children: buildSummaryChildren(),
        });
        // Remaining content (excluding summary).
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
        // Alias: always render as <details> so Yoga defaults + renderer paths apply.
        return [{ kind: 'block', key: detailsKey, tagName: 'details', attrs, children }];
    }
    // Gather text (including inline elements) into the current block.
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
                // NOTE: toRenderTree(child) already returns a wrapped block node for normal elements.
                // If we wrapped again here, we'd get nested duplicates (e.g. each <button> becomes two).
                childBlocks.push(...toRenderTree(child, childPath));
            }
            else {
                // Inline-ish: treat its text as part of this block's text.
                inlineText += extractText(child) + ' ';
            }
            continue;
        }
    }
    const tail = normalizeWhitespace(inlineText);
    if (tail.length > 0)
        childBlocks.push({ kind: 'text', text: tail });
    // Wrap: for BODY/HTML just return its children.
    if (tagName === 'html' || tagName === 'body')
        return childBlocks;
    out.push({ kind: 'block', key: `${path}:${tagName}`, tagName, attrs, children: childBlocks });
    return out;
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
function createTextMeasurer(font) {
    const canvas = document.createElement('canvas');
    const ctx = canvas.getContext('2d');
    if (!ctx)
        throw new Error('2D canvas not available');
    ctx.font = font;
    const fontSizeMatch = /(^|\s)(\d+)px\b/.exec(font);
    const fontSize = fontSizeMatch ? Number(fontSizeMatch[2]) : 16;
    const lineHeight = Math.ceil(fontSize * 1.25);
    return {
        measure(text, maxWidth) {
            const words = normalizeWhitespace(text).split(' ').filter(Boolean);
            if (words.length === 0)
                return { width: 0, height: lineHeight, lines: [''] };
            const lines = [];
            let current = '';
            for (const word of words) {
                const next = current ? `${current} ${word}` : word;
                const nextWidth = ctx.measureText(next).width;
                const limit = maxWidth ?? Number.POSITIVE_INFINITY;
                if (nextWidth <= limit || !current) {
                    current = next;
                }
                else {
                    lines.push(current);
                    current = word;
                }
            }
            if (current)
                lines.push(current);
            const width = Math.min(Math.max(...lines.map((l) => ctx.measureText(l).width)), maxWidth ?? Number.POSITIVE_INFINITY);
            const height = lines.length * lineHeight;
            return { width: Math.ceil(width), height: Math.ceil(height), lines };
        },
        lineHeight,
        font,
    };
}
async function buildLayoutTree(renderNodes, viewportWidth, viewportHeight) {
    const padding = 12;
    const gap = 8;
    const theme = defaultTheme;
    const measurer = createTextMeasurer(`${theme.fontSize}px ${theme.fontFamily}`);
    function gapAfter(child) {
        if (child.kind !== 'block')
            return 0;
        // Some nodes manage their own spacing.
        if (child.tagName === 'hr')
            return 0;
        // Table internals are tightly packed.
        if (child.tagName === 'tr' || child.tagName === 'td' || child.tagName === 'th')
            return 0;
        return gap;
    }
    function yogaForNode(node) {
        if (node.kind === 'text') {
            const yogaNode = Yoga.Node.create();
            yogaNode.setMeasureFunc((width, widthMode) => {
                const maxWidth = widthMode === Yoga.MEASURE_MODE_UNDEFINED ? undefined : Math.max(0, width);
                const m = measurer.measure(node.text, maxWidth);
                return { width: m.width, height: m.height };
            });
            // Small spacing so row-wrap paragraphs don't glue words together.
            yogaNode.setMargin(Yoga.EDGE_RIGHT, 6);
            yogaNode.setMargin(Yoga.EDGE_BOTTOM, 0);
            return {
                yogaNode,
                buildBox: () => ({
                    kind: 'text',
                    text: node.text,
                    x: yogaNode.getComputedLeft(),
                    y: yogaNode.getComputedTop(),
                    width: yogaNode.getComputedWidth(),
                    height: yogaNode.getComputedHeight(),
                    children: [],
                }),
            };
        }
        // Some widgets are measured leaf nodes.
        if (node.tagName === 'sliderlabel') {
            return createYogaNodeForSliderLabel({ node, Yoga, measurer });
        }
        const yogaNode = Yoga.Node.create();
        yogaNode.setFlexDirection(Yoga.FLEX_DIRECTION_COLUMN);
        yogaNode.setAlignItems(Yoga.ALIGN_STRETCH);
        yogaNode.setPadding(Yoga.EDGE_LEFT, padding);
        yogaNode.setPadding(Yoga.EDGE_RIGHT, padding);
        yogaNode.setPadding(Yoga.EDGE_TOP, padding);
        yogaNode.setPadding(Yoga.EDGE_BOTTOM, padding);
        // Margins are applied between siblings (not on every node), to avoid
        // "extra bottom padding" inside containers like <form>.
        yogaNode.setMargin(Yoga.EDGE_BOTTOM, 0);
        if (isHeadingTag(node.tagName))
            applyYogaDefaultsHeading(yogaNode, Yoga);
        if (node.tagName === 'hr')
            applyYogaDefaultsHr(yogaNode, Yoga);
        // Inline-ish containers: only use row+wrap when mixing text with controls.
        // For plain text paragraphs, a column layout with a single measured text node is more stable.
        if (node.tagName === 'p' || node.tagName === 'label') {
            const hasControls = node.children.some((c) => c.kind === 'block' &&
                (c.tagName === 'input' ||
                    c.tagName === 'button' ||
                    c.tagName === 'select' ||
                    c.tagName === 'textarea' ||
                    c.tagName === 'timeinput' ||
                    c.tagName === 'dateinput' ||
                    c.tagName === 'monthinput' ||
                    c.tagName === 'weekinput' ||
                    c.tagName === 'datetimelocalinput' ||
                    c.tagName === 'progress' ||
                    c.tagName === 'meter' ||
                    c.tagName === 'slider' ||
                    c.tagName === 'number' ||
                    c.tagName === 'color'));
            if (hasControls) {
                yogaNode.setFlexDirection(Yoga.FLEX_DIRECTION_ROW);
                yogaNode.setFlexWrap(Yoga.WRAP_WRAP);
                yogaNode.setAlignItems(Yoga.ALIGN_CENTER);
            }
            yogaNode.setPadding(Yoga.EDGE_TOP, 4);
            yogaNode.setPadding(Yoga.EDGE_BOTTOM, 4);
            yogaNode.setPadding(Yoga.EDGE_LEFT, 4);
            yogaNode.setPadding(Yoga.EDGE_RIGHT, 4);
        }
        // Table-ish: table is a column of rows; rows are horizontal.
        if (node.tagName === 'table')
            applyYogaDefaultsTable(yogaNode, Yoga);
        if (node.tagName === 'tr')
            applyYogaDefaultsTr(yogaNode, Yoga);
        if (node.tagName === 'td' || node.tagName === 'th')
            applyYogaDefaultsCell(yogaNode, Yoga);
        if (node.tagName === 'input')
            applyYogaDefaultsInput(yogaNode, node, Yoga);
        if (node.tagName === 'textarea')
            applyYogaDefaultsTextarea(yogaNode, Yoga);
        if (node.tagName === 'select')
            applyYogaDefaultsSelect(yogaNode, Yoga);
        if (node.tagName === 'timeinput' ||
            node.tagName === 'dateinput' ||
            node.tagName === 'monthinput' ||
            node.tagName === 'weekinput' ||
            node.tagName === 'datetimelocalinput') {
            const kind = node.tagName === 'timeinput'
                ? 'time'
                : node.tagName === 'monthinput'
                    ? 'month'
                    : node.tagName === 'weekinput'
                        ? 'week'
                        : node.tagName === 'dateinput'
                            ? 'date'
                            : 'datetime-local';
            applyYogaDefaultsTemporalInput(yogaNode, Yoga, kind);
        }
        if (node.tagName === 'img')
            applyYogaDefaultsImg(yogaNode, node, Yoga);
        if (node.tagName === 'svg')
            applyYogaDefaultsSvg(yogaNode, node, Yoga);
        if (node.tagName === 'canvas')
            applyYogaDefaultsCanvas(yogaNode, node, Yoga);
        if (node.tagName === 'iframe')
            applyYogaDefaultsIframe(yogaNode, node, Yoga);
        if (node.tagName === 'button')
            applyYogaDefaultsButton(yogaNode, Yoga);
        if (node.tagName === 'dialog')
            applyYogaDefaultsDialog(yogaNode, Yoga);
        if (node.tagName === 'number')
            applyYogaDefaultsNumber(yogaNode, Yoga);
        if (node.tagName === 'color')
            applyYogaDefaultsColor(yogaNode, node, Yoga);
        if (node.tagName === 'searchrow')
            applyYogaDefaultsSearchRow(yogaNode, Yoga);
        if (node.tagName === 'searchbutton')
            applyYogaDefaultsSearchButton(yogaNode, Yoga);
        if (node.tagName === 'summary')
            applyYogaDefaultsSummary(yogaNode, Yoga);
        if (node.tagName === 'details')
            applyYogaDefaultsDetails(yogaNode, Yoga);
        if (node.tagName === 'barrow')
            applyYogaDefaultsBarrow(yogaNode, Yoga);
        if (node.tagName === 'progress' || node.tagName === 'meter')
            applyYogaDefaultsProgressOrMeter(yogaNode, Yoga);
        if (node.tagName === 'slider')
            applyYogaDefaultsSlider(yogaNode, Yoga);
        const effectiveChildren = getEffectiveDetailsChildren(node, uiState.detailsOpen);
        const childPairs = effectiveChildren.map(yogaForNode);
        for (let i = 0; i < childPairs.length; i++) {
            const childRender = effectiveChildren[i];
            const childPair = childPairs[i];
            if (childRender && childRender.kind === 'block') {
                const m = i === childPairs.length - 1 ? 0 : gapAfter(childRender);
                childPair.yogaNode.setMargin(Yoga.EDGE_BOTTOM, m);
            }
            yogaNode.insertChild(childPair.yogaNode, yogaNode.getChildCount());
        }
        return {
            yogaNode,
            buildBox: () => ({
                kind: 'block',
                key: node.key,
                tagName: node.tagName,
                attrs: node.attrs,
                x: yogaNode.getComputedLeft(),
                y: yogaNode.getComputedTop(),
                width: yogaNode.getComputedWidth(),
                height: yogaNode.getComputedHeight(),
                children: childPairs.map((c) => c.buildBox()),
            }),
        };
    }
    const rootYoga = Yoga.Node.create();
    rootYoga.setFlexDirection(Yoga.FLEX_DIRECTION_COLUMN);
    rootYoga.setAlignItems(Yoga.ALIGN_STRETCH);
    rootYoga.setWidth(viewportWidth);
    rootYoga.setHeight(viewportHeight);
    rootYoga.setPadding(Yoga.EDGE_LEFT, 16);
    rootYoga.setPadding(Yoga.EDGE_TOP, 16);
    // Reserve an extra gutter so content doesn't touch the global scrollbar.
    rootYoga.setPadding(Yoga.EDGE_RIGHT, 16 + SCROLLBAR_PAD);
    rootYoga.setPadding(Yoga.EDGE_BOTTOM, 16);
    const pairs = renderNodes.map(yogaForNode);
    for (let i = 0; i < pairs.length; i++) {
        const renderNode = renderNodes[i];
        const pair = pairs[i];
        if (renderNode && renderNode.kind === 'block') {
            const m = i === pairs.length - 1 ? 0 : gapAfter(renderNode);
            pair.yogaNode.setMargin(Yoga.EDGE_BOTTOM, m);
        }
        rootYoga.insertChild(pair.yogaNode, rootYoga.getChildCount());
    }
    rootYoga.calculateLayout(viewportWidth, viewportHeight, Yoga.DIRECTION_LTR);
    const box = {
        kind: 'block',
        tagName: 'root',
        x: 0,
        y: 0,
        width: rootYoga.getComputedWidth(),
        height: rootYoga.getComputedHeight(),
        children: pairs.map((p) => p.buildBox()),
    };
    // Cleanup yoga nodes to avoid leaks.
    // IMPORTANT: don't manually free children; Yoga's freeRecursive handles the whole subtree.
    rootYoga.freeRecursive?.();
    return box;
}
function renderToPixi(app, box, sceneRoot) {
    const theme = defaultTheme;
    const stage = sceneRoot ?? app.stage;
    // Stable scene structure:
    // [background][contentRoot][dialogRoot][overlayRoot]
    const background = getOrCreateGraphics(stage, '__background');
    const contentRoot = getOrCreateContainer(stage, '__contentRoot');
    const dialogRoot = getOrCreateContainer(stage, '__dialogRoot');
    const overlayRoot = getOrCreateContainer(stage, '__overlayRoot');
    ensureChildAtAny(stage, background, 0);
    ensureChildAt(stage, contentRoot, 1);
    ensureChildAt(stage, dialogRoot, 2);
    ensureChildAt(stage, overlayRoot, 3);
    // Overlays are built immediate-mode (only when open); clear them each paint.
    overlayRoot.removeChildren();
    const selectPopups = [];
    const temporalPopups = [];
    const radioGroups = collectRadioGroups(box);
    uiState.fieldBounds.clear();
    uiState.sliderBounds.clear();
    uiState.dialogDragBounds.clear();
    uiState.hoverRects.length = 0;
    uiState.hoverHandlers.clear();
    uiState.iframeRects.length = 0;
    const nodeCache = retainedNodeCache.get(stage) ?? new Map();
    retainedNodeCache.set(stage, nodeCache);
    const usedNodeKeys = new Set();
    const computeContentHeightForBox = (root) => {
        let max = 0;
        const walk = (n, ax, ay) => {
            // Ignore floating dialogs; they are overlays.
            if (n.kind === 'block' && n.tagName === 'dialog')
                return;
            const nx = ax + n.x;
            const ny = ay + n.y;
            max = Math.max(max, ny + n.height);
            for (const c of n.children ?? [])
                walk(c, nx, ny);
        };
        for (const c of root.children ?? [])
            walk(c, 0, 0);
        return max;
    };
    const activeDragKeys = new Set();
    for (const d of uiState.textDrags.values())
        activeDragKeys.add(d.key);
    const measure = getRenderMeasure(theme);
    function clamp(n, lo, hi) {
        return Math.max(lo, Math.min(hi, n));
    }
    const firstDraggingPointerForKey = (key) => {
        for (const [pid, d] of uiState.textDrags.entries()) {
            if (d.key === key)
                return pid;
        }
        return null;
    };
    const focusedPidForKey = (key) => {
        // If multiple pointers focus the same key, prefer the keyboard owner.
        const kb = uiState.keyboardOwnerPointerId;
        if (uiState.focusedKeyByPointer.get(kb) === key)
            return kb;
        for (const [pid, k] of uiState.focusedKeyByPointer.entries()) {
            if (k === key)
                return pid;
        }
        return null;
    };
    // SVG strings are centralized in src/svgs.ts
    // Background fill.
    clearGraphics(background);
    background.rect(0, 0, app.renderer.width, app.renderer.height);
    background.fill(theme.background);
    // All normal document content lives in this container, which is translated for global scrolling.
    contentRoot.position.set(0, -uiState.scroll.y);
    function drawNode(node, parent, textCtx, absX = 0, absY = 0, dialogSink, dialogClampRect, path, orderIndex) {
        // IMPORTANT: LayoutBox.key can be an empty string in some helper nodes.
        // Treat empty keys as missing to avoid collisions that can create container cycles.
        const stableBlockKey = node.kind === 'block'
            ? node.key && node.key.length > 0
                ? node.key
                : `${path}:${node.tagName ?? 'block'}`
            : '';
        const cacheKey = node.kind === 'block' ? `b:${stableBlockKey}` : `t:${path}`;
        let container = nodeCache.get(cacheKey);
        if (!container || wouldCreateCycle(parent, container)) {
            // If the cached container would create a cycle under this parent, it means
            // the key was reused incorrectly (or the node moved) in a way that would
            // reparent an ancestor into its own subtree. Create a fresh container.
            container = new Container();
            container.label = cacheKey;
            nodeCache.set(cacheKey, container);
        }
        usedNodeKeys.add(cacheKey);
        ensureChildAt(parent, container, orderIndex);
        // Use a dedicated child-root so widget internals don't interleave with layout children.
        const childrenRoot = getOrCreateContainer(container, '__children');
        // Put layout children above the base graphics, but allow widgets to add overlays above.
        ensureChildAt(container, childrenRoot, 1);
        container.position.set(node.x, node.y);
        // Pixel-align 1px rules so symmetric margins look symmetric.
        if (node.kind === 'block' && node.tagName === 'hr') {
            container.position.set(Math.round(node.x), Math.round(node.y));
        }
        // Floating dialogs override their position from UI state, but are clamped to the
        // visible area of their containing stacking context (viewport, iframe content, or parent dialog).
        if (node.kind === 'block' && node.tagName === 'dialog' && node.key) {
            const st = getOrInitDialogState(uiState.dialogs, node.key);
            const dw = Math.max(0, node.width);
            const dh = Math.max(0, node.height);
            const minX = dialogClampRect.x;
            const minY = dialogClampRect.y;
            const maxX = Math.max(minX, dialogClampRect.x + dialogClampRect.w - dw);
            const maxY = Math.max(minY, dialogClampRect.y + dialogClampRect.h - dh);
            // Persist the clamp bounds so pointermove can use them during drags.
            uiState.dialogDragBounds.set(node.key, { minX, minY, maxX, maxY });
            st.x = Math.max(minX, Math.min(maxX, st.x));
            st.y = Math.max(minY, Math.min(maxY, st.y));
            container.position.set(st.x, st.y);
        }
        const nodeAbsX = absX + container.position.x;
        const nodeAbsY = absY + container.position.y;
        if (node.kind === 'block') {
            let nextTextCtx = textCtx;
            if (node.tagName === 'h1' ||
                node.tagName === 'h2' ||
                node.tagName === 'h3' ||
                node.tagName === 'summary' ||
                node.tagName === 'th') {
                nextTextCtx = { bold: true };
            }
            const g = getOrCreateGraphics(container, '__g');
            clearGraphics(g);
            // Make sure the base graphics stays behind everything else.
            ensureChildAtAny(container, g, 0);
            g.zIndex = -10;
            let w = Math.max(0, node.width);
            let h = Math.max(0, node.height);
            let overlayLabel = null;
            // Headings: snap to whole pixels so the 1px border doesn't land on half pixels
            // (which can look like a faint extra 1px row outside the top edge).
            if (node.tagName === 'h1' || node.tagName === 'h2' || node.tagName === 'h3') {
                container.position.set(Math.round(node.x), Math.round(node.y));
                w = Math.round(w);
                h = Math.round(h);
            }
            if (node.tagName === 'hr') {
                renderHr({ graphics: g, w, theme });
            }
            else if (node.tagName === 'barrow') {
                // Layout-only wrapper for [label][bar]. No visuals.
            }
            else if (node.tagName === 'searchrow') {
                // Layout-only wrapper for [search icon button][input]. No visuals.
            }
            else if (node.tagName === 'searchbutton') {
                renderSearchButton({
                    node,
                    container,
                    graphics: g,
                    w,
                    h,
                    theme,
                    uiState,
                    getPointerId: getEffectivePointerId,
                    focusInputKey: node.attrs?.['data-focus-key'],
                    requestPaint,
                });
            }
            else if (node.tagName === 'progress' || node.tagName === 'meter') {
                renderProgressOrMeter({ node, graphics: g, w, h, theme });
            }
            else if (node.tagName === 'sliderlabel') {
                renderSliderLabel({
                    node,
                    container,
                    theme,
                    sliderStates: uiState.sliders,
                });
            }
            else if (node.tagName === 'slider') {
                renderSlider({
                    node,
                    container,
                    graphics: g,
                    w,
                    h,
                    absX: nodeAbsX,
                    absY: nodeAbsY,
                    theme,
                    sliderStates: uiState.sliders,
                    sliderBounds: uiState.sliderBounds,
                    sliderDrags: uiState.sliderDrags,
                    requestPaint,
                    getPointerId: getEffectivePointerId,
                });
            }
            else if (node.tagName === 'timeinput' ||
                node.tagName === 'dateinput' ||
                node.tagName === 'monthinput' ||
                node.tagName === 'weekinput' ||
                node.tagName === 'datetimelocalinput') {
                renderTemporalInput({
                    node,
                    container,
                    graphics: g,
                    w,
                    h,
                    absX: nodeAbsX,
                    absY: nodeAbsY,
                    theme,
                    uiState,
                    getPointerId: getEffectivePointerId,
                    getCursorColor,
                    temporalStates: uiState.temporals,
                    yearSliderOwners: uiState.temporalYearOwners,
                    getOrInitInputValue: (k, attrs) => getOrInitInputState(k, attrs),
                    requestPaint,
                    popupSink: temporalPopups,
                });
            }
            else if (node.tagName === 'input') {
                const key = node.key;
                const focusPid = key != null ? focusedPidForKey(key) : null;
                const isKeyboardFocused = key != null && uiState.focusedKeyByPointer.get(uiState.keyboardOwnerPointerId) === key;
                const caretPointerId = key == null
                    ? null
                    : isKeyboardFocused
                        ? uiState.keyboardOwnerPointerId
                        : activeDragKeys.has(key)
                            ? firstDraggingPointerForKey(key)
                            : null;
                const showCaret = caretPointerId != null;
                const focusColor = focusPid != null ? getCursorColor(focusPid) : null;
                renderInput({
                    node,
                    container,
                    graphics: g,
                    w,
                    h,
                    absX: nodeAbsX,
                    absY: nodeAbsY,
                    theme,
                    textMeasure: measure,
                    uiState,
                    getOrInitInputState,
                    clamp,
                    radioGroups,
                    textDrags: uiState.textDrags,
                    requestPaint,
                    showCaret,
                    caretPointerId,
                    focusColor: focusColor ?? undefined,
                    getCursorColor,
                    getPointerId: getEffectivePointerId,
                });
            }
            else if (node.tagName === 'textarea') {
                const key = node.key;
                const focusPid = key != null ? focusedPidForKey(key) : null;
                const isKeyboardFocused = key != null && uiState.focusedKeyByPointer.get(uiState.keyboardOwnerPointerId) === key;
                const caretPointerId = key == null
                    ? null
                    : isKeyboardFocused
                        ? uiState.keyboardOwnerPointerId
                        : activeDragKeys.has(key)
                            ? firstDraggingPointerForKey(key)
                            : null;
                const showCaret = caretPointerId != null;
                const focusColor = focusPid != null ? getCursorColor(focusPid) : null;
                renderTextarea({
                    node,
                    container,
                    graphics: g,
                    w,
                    h,
                    absX: nodeAbsX,
                    absY: nodeAbsY,
                    theme,
                    textMeasure: measure,
                    uiState,
                    getOrInitInputState,
                    clamp,
                    textDrags: uiState.textDrags,
                    requestPaint,
                    showCaret,
                    caretPointerId,
                    focusColor: focusColor ?? undefined,
                    getCursorColor,
                    getPointerId: getEffectivePointerId,
                });
            }
            else if (node.tagName === 'select') {
                // Ensure state exists so it persists across rerenders.
                if (node.key) {
                    const initIdx = Number(node.attrs?.['data-selected-index'] ?? '0');
                    getOrInitSelectState(uiState.selects, node.key, Number.isFinite(initIdx) ? initIdx : 0);
                }
                renderSelect({
                    node,
                    container,
                    graphics: g,
                    w,
                    h,
                    absX: nodeAbsX,
                    absY: nodeAbsY,
                    theme,
                    selectStates: uiState.selects,
                    uiState,
                    getPointerId: getEffectivePointerId,
                    getCursorColor,
                    requestPaint,
                    popupSink: selectPopups,
                });
            }
            else if (node.tagName === 'summary') {
                if (node.key) {
                    uiState.hoverRects.push({ key: node.key, kind: 'summary', cursor: 'pointer', x: nodeAbsX, y: nodeAbsY, w, h });
                }
                renderSummary({
                    node,
                    container,
                    w,
                    h,
                    theme,
                    detailsOpen: uiState.detailsOpen,
                    requestRerender,
                });
            }
            else if (node.tagName === 'dialog') {
                renderDialog({
                    node,
                    container,
                    w,
                    h,
                    theme,
                    selectedBy: uiState.dialogSelectedBy,
                    getCursorColor,
                    dialogStates: uiState.dialogs,
                    dialogDrags: uiState.dialogDrags,
                    bringToFront: (k) => {
                        uiState.dialogZ.set(k, uiState.dialogZCounter++);
                    },
                    requestPaint,
                    getPointerId: getEffectivePointerId,
                });
            }
            else if (node.tagName === 'img') {
                renderImg({ node, container, graphics: g, w, h, theme, requestRerender });
            }
            else if (node.tagName === 'svg') {
                const svgMarkup = node.attrs?.['data-svg'] ?? '';
                // Reuse the same Graphics container; svg rendering adds its own Graphics.
                renderSvgElement({ svgMarkup, container, w, h, requestRerender });
            }
            else if (node.tagName === 'canvas') {
                renderCanvasElement({ node, container, graphics: g, w, h, theme });
            }
            else if (node.tagName === 'iframe') {
                renderIframePlaceholder({ node, container, graphics: g, w, h, theme });
            }
            else if (node.tagName === 'color') {
                uiState.color.bounds = { x: nodeAbsX, y: nodeAbsY, w: Math.max(0, w), h: Math.max(0, h) };
                renderColorPicker({
                    node,
                    container,
                    graphics: g,
                    w,
                    h,
                    theme,
                    rgb: uiState.color.rgb,
                    setRgb: (rgb) => {
                        uiState.color.rgb = rgb;
                    },
                    alpha: uiState.color.a,
                    setAlpha: (a) => {
                        uiState.color.a = Math.max(0, Math.min(255, Math.round(a)));
                    },
                    pick: uiState.color.pick,
                    setPick: (p) => {
                        uiState.color.pick = p;
                    },
                    requestPaint,
                    getPointerId: getEffectivePointerId,
                    setDraggingPointerId: (pid) => {
                        uiState.color.draggingPointerId = pid;
                    },
                });
            }
            else if (node.tagName === 'number') {
                const key = node.key;
                const ch = String(node.attrs?.channel ?? '').toLowerCase();
                const isCh = ch === 'r' || ch === 'g' || ch === 'b' || ch === 'a';
                if (key) {
                    renderNumberSpinner({
                        node,
                        container,
                        graphics: g,
                        w,
                        h,
                        theme,
                        getValue: () => {
                            if (isCh) {
                                if (ch === 'a')
                                    return uiState.color.a ?? 255;
                                return uiState.color.rgb[ch] ?? 0;
                            }
                            return getOrInitNumberState(uiState.numbers, key, node.attrs).value;
                        },
                        setValue: (n) => {
                            if (isCh) {
                                if (ch === 'a')
                                    uiState.color.a = Math.max(0, Math.min(255, Math.round(n)));
                                else
                                    uiState.color.rgb[ch] = Math.max(0, Math.min(255, Math.round(n)));
                            }
                            else {
                                getOrInitNumberState(uiState.numbers, key, node.attrs).value = n;
                            }
                        },
                        requestPaint,
                        numberHolds: uiState.numberHolds,
                        getPointerId: getEffectivePointerId,
                    });
                }
            }
            else if (node.tagName === 'button') {
                if (node.key) {
                    uiState.hoverRects.push({ key: node.key, kind: 'button', cursor: 'pointer', x: nodeAbsX, y: nodeAbsY, w, h });
                }
                renderButton({
                    container,
                    graphics: g,
                    w,
                    h,
                    theme,
                    registerHoverHandlers: node.key
                        ? (handlers) => {
                            uiState.hoverHandlers.set(node.key, handlers);
                        }
                        : undefined,
                });
            }
            else if (isHeadingTag(node.tagName)) {
                // Headings should not get the generic 1px element border.
            }
            else if (node.tagName === 'table') {
                renderTable({ graphics: g, w, h, boxBorder: theme.boxBorder });
            }
            else if (node.tagName === 'td' || node.tagName === 'th') {
                renderCell({ nodeTag: node.tagName, graphics: g, w, h, theme });
            }
            else {
                // Default block border: draw fully inside the box so it doesn't "bleed"
                // into the outside margin area (which looks like an extra 1px row above).
                const bw = Math.max(0, Math.round(w));
                const bh = Math.max(0, Math.round(h));
                g.rect(0, 0, bw, bh);
                g.stroke({ width: 1, color: theme.boxBorder, alignment: 0 });
            }
            if (overlayLabel)
                container.addChild(overlayLabel);
            // Iframe: clip all nested content to the frame rect.
            // (This is the first step toward a true nested scene.)
            let iframeContentRoot = null;
            let iframeScrollRoot = null;
            const isRootIframe = node.tagName === 'iframe' && String(node.attrs?.['data-root'] ?? '') === '1';
            if (node.tagName === 'iframe' && !isRootIframe) {
                // Chrome is drawn into `g` by renderIframePlaceholder; scroll applies only to content.
                const IFRAME_CHROME_TOP = 34;
                const IFRAME_PAD_X = 8;
                const IFRAME_PAD_BOTTOM = 8;
                // Track iframe rect for wheel routing.
                if (node.key) {
                    uiState.iframeRects.push({ key: node.key, x: nodeAbsX, y: nodeAbsY, w: Math.max(0, w), h: Math.max(0, h) });
                }
                iframeContentRoot = getOrCreateContainer(container, '__iframeContentRoot');
                iframeContentRoot.position.set(0, 0);
                // Mask only the content area (keep header fixed).
                const contentMask = getOrCreateGraphics(container, '__iframeContentMask');
                clearGraphics(contentMask);
                const maskX = 0;
                const maskY = IFRAME_CHROME_TOP;
                const maskW = Math.max(0, w);
                const maskH = Math.max(0, h - IFRAME_CHROME_TOP);
                contentMask.rect(maskX, maskY, maskW, maskH);
                contentMask.fill(0xffffff);
                contentMask.alpha = 0;
                iframeContentRoot.mask = contentMask;
                // Per-iframe scroll state.
                const iframeKey = node.key ?? '';
                const st = uiState.iframeScroll.get(iframeKey) ??
                    {
                        y: 0,
                        contentHeight: 0,
                        viewportHeight: 0,
                        draggingPointerId: null,
                        dragOffsetY: 0,
                        track: { x: 0, y: 0, w: SCROLLBAR_W, h: 0 },
                        thumb: { x: 0, y: 0, w: SCROLLBAR_W, h: 0 },
                        rect: { x: nodeAbsX, y: nodeAbsY, w: Math.max(0, w), h: Math.max(0, h) },
                    };
                st.rect = { x: nodeAbsX, y: nodeAbsY, w: Math.max(0, w), h: Math.max(0, h) };
                // Compute content height from layout subtree (relative), then clamp scroll.
                st.contentHeight = computeContentHeightForBox(node);
                st.viewportHeight = Math.max(0, h - IFRAME_CHROME_TOP - IFRAME_PAD_BOTTOM);
                const maxScroll = Math.max(0, st.contentHeight - st.viewportHeight);
                st.y = Math.max(0, Math.min(st.y, maxScroll));
                // Scroll root (translated).
                iframeScrollRoot = getOrCreateContainer(iframeContentRoot, '__iframeScrollRoot');
                iframeScrollRoot.position.set(0, -st.y);
                // Draw iframe-local scrollbar.
                const scrollbar = getOrCreateGraphics(container, '__iframeScrollbar');
                clearGraphics(scrollbar);
                scrollbar.eventMode = 'static';
                // Place it inside the iframe content area.
                const pad = SCROLLBAR_PAD;
                const trackW = SCROLLBAR_W;
                const trackX = Math.max(0, w - trackW - pad);
                const trackY = IFRAME_CHROME_TOP + pad;
                const trackH = Math.max(0, (h - IFRAME_CHROME_TOP) - pad * 2);
                const show = maxScroll > 0.5 && trackH > 1;
                scrollbar.visible = show;
                if (show) {
                    const minThumbH = 24;
                    const thumbH = Math.max(minThumbH, ((st.viewportHeight || 1) / Math.max(1, st.contentHeight)) * trackH);
                    const travel = Math.max(1, trackH - thumbH);
                    const ratio = maxScroll <= 0 ? 0 : st.y / maxScroll;
                    const thumbY = trackY + travel * ratio;
                    st.track = { x: nodeAbsX + trackX, y: nodeAbsY + trackY, w: trackW, h: trackH };
                    st.thumb = { x: nodeAbsX + trackX, y: nodeAbsY + thumbY, w: trackW, h: thumbH };
                    scrollbar.rect(trackX, trackY, trackW, trackH);
                    scrollbar.fill({ color: 0x000000, alpha: 0.06 });
                    scrollbar.rect(trackX, thumbY, trackW, thumbH);
                    scrollbar.fill({ color: 0x000000, alpha: 0.25 });
                    scrollbar.on('pointerdown', (ev) => {
                        if (ev?.button === 2)
                            return;
                        const pid = getEffectivePointerId(ev);
                        if (pid <= 0)
                            return;
                        const gx = ev.global?.x ?? 0;
                        const gy = ev.global?.y ?? 0;
                        const hitTrack = gx >= st.track.x && gx <= st.track.x + st.track.w && gy >= st.track.y && gy <= st.track.y + st.track.h;
                        if (!hitTrack)
                            return;
                        const hitThumb = gx >= st.thumb.x && gx <= st.thumb.x + st.thumb.w && gy >= st.thumb.y && gy <= st.thumb.y + st.thumb.h;
                        if (hitThumb) {
                            st.draggingPointerId = pid;
                            st.dragOffsetY = gy - st.thumb.y;
                            uiState.iframeScroll.set(iframeKey, st);
                            ev.stopPropagation?.();
                            return;
                        }
                        // Track click: jump thumb and start dragging.
                        const travel2 = Math.max(1, st.track.h - st.thumb.h);
                        const targetTop = Math.max(st.track.y, Math.min(st.track.y + travel2, gy - st.thumb.h / 2));
                        const ratio2 = (targetTop - st.track.y) / travel2;
                        st.y = Math.max(0, Math.min(maxScroll, ratio2 * maxScroll));
                        st.draggingPointerId = pid;
                        st.dragOffsetY = gy - targetTop;
                        uiState.iframeScroll.set(iframeKey, st);
                        requestPaint?.();
                        ev.stopPropagation?.();
                    });
                }
                else {
                    st.track = { x: 0, y: 0, w: trackW, h: 0 };
                    st.thumb = { x: 0, y: 0, w: trackW, h: 0 };
                }
                uiState.iframeScroll.set(iframeKey, st);
            }
            // Dialog stacking model:
            // - Dialogs are hoisted out of normal flow drawing into an overlay list.
            // - The overlay list is per stacking context: root or the nearest ancestor dialog.
            // This makes dialogs float above later siblings even when nested inside other elements,
            // while still letting dialogs nest inside dialogs.
            const localDialogs = [];
            const childSink = node.tagName === 'dialog' || (node.tagName === 'iframe' && !isRootIframe) ? localDialogs : dialogSink;
            // Dialog clamp rect for this stacking context.
            let childDialogClampRect = dialogClampRect;
            if (node.tagName === 'dialog') {
                // Nested dialogs are constrained to the parent dialog box.
                childDialogClampRect = { x: 0, y: 0, w: Math.max(0, w), h: Math.max(0, h) };
            }
            else if (node.tagName === 'iframe' && !isRootIframe) {
                // Dialogs inside iframes are constrained to the iframe content viewport.
                const iframeKey = node.key ?? '';
                const st = uiState.iframeScroll.get(iframeKey);
                const scrollY = st ? st.y : 0;
                const IFRAME_CHROME_TOP = 34;
                childDialogClampRect = {
                    x: 0,
                    y: IFRAME_CHROME_TOP + scrollY,
                    w: Math.max(0, w),
                    h: Math.max(0, h - IFRAME_CHROME_TOP),
                };
            }
            const childParent = iframeScrollRoot ?? iframeContentRoot ?? childrenRoot;
            const childAbsX = nodeAbsX + (iframeContentRoot?.position.x ?? 0);
            const childAbsY = nodeAbsY + (iframeContentRoot?.position.y ?? 0) + (iframeScrollRoot?.position.y ?? 0);
            let childOrder = 0;
            for (let ci = 0; ci < (node.children ?? []).length; ci++) {
                const child = (node.children ?? [])[ci];
                if (child.kind === 'block' && child.tagName === 'dialog') {
                    childSink.push(child);
                }
                else {
                    drawNode(child, childParent, nextTextCtx, childAbsX, childAbsY, childSink, childDialogClampRect, `${path}.${ci}`, childOrder++);
                }
            }
            if ((node.tagName === 'dialog' || (node.tagName === 'iframe' && !isRootIframe)) && localDialogs.length > 0) {
                localDialogs.sort((a, b) => {
                    const az = a.key ? uiState.dialogZ.get(a.key) ?? 0 : 0;
                    const bz = b.key ? uiState.dialogZ.get(b.key) ?? 0 : 0;
                    return az - bz;
                });
                for (const dlg of localDialogs) {
                    const dlgKey = dlg.key && dlg.key.length > 0 ? dlg.key : `${path}.dlg.${childOrder}`;
                    drawNode(dlg, childParent, nextTextCtx, childAbsX, childAbsY, localDialogs, childDialogClampRect, `${path}.dlg.${dlgKey}`, childOrder++);
                }
            }
        }
        else {
            const t = getOrCreateText(container, '__text', (tt) => {
                tt.style = {
                    fontFamily: theme.fontFamily,
                    fontSize: theme.fontSize,
                    fill: theme.text,
                    fontWeight: textCtx.bold ? '700' : '400',
                    wordWrap: true,
                    wordWrapWidth: 0,
                };
            });
            t.text = node.text ?? '';
            t.style.fontFamily = theme.fontFamily;
            t.style.fontSize = theme.fontSize;
            t.style.fill = theme.text;
            t.style.fontWeight = textCtx.bold ? '700' : '400';
            t.style.wordWrap = true;
            t.style.wordWrapWidth = Math.max(0, Math.ceil(node.width) + WRAP_EPSILON_PX);
            t.position.set(0, TEXT_BASELINE_NUDGE_Y);
        }
    }
    const baseTextCtx = { bold: false };
    const stageClampRect = { x: 0, y: 0, w: app.renderer.width, h: app.renderer.height };
    const rootDialogs = [];
    let rootOrder = 0;
    for (let i = 0; i < box.children.length; i++) {
        const child = box.children[i];
        if (child.kind === 'block' && child.tagName === 'dialog')
            rootDialogs.push(child);
        else
            drawNode(child, contentRoot, baseTextCtx, 0, contentRoot.position.y, rootDialogs, stageClampRect, `root.${i}`, rootOrder++);
    }
    if (rootDialogs.length > 0) {
        rootDialogs.sort((a, b) => {
            const az = a.key ? uiState.dialogZ.get(a.key) ?? 0 : 0;
            const bz = b.key ? uiState.dialogZ.get(b.key) ?? 0 : 0;
            return az - bz;
        });
        let dlgOrder = 0;
        for (const dlg of rootDialogs) {
            const dlgKey = dlg.key && dlg.key.length > 0 ? dlg.key : `rootdlg.${dlgOrder}`;
            drawNode(dlg, dialogRoot, baseTextCtx, 0, 0, rootDialogs, stageClampRect, `dlg.${dlgKey}`, dlgOrder++);
        }
    }
    // Draw temporal picker popups before <select> popups so time pickers can contribute
    // nested <select> dropdowns and have those rendered above.
    if (temporalPopups.length > 0) {
        renderTemporalPopups({
            popups: temporalPopups,
            stage: overlayRoot,
            theme,
            viewportW: app.renderer.width,
            viewportH: app.renderer.height,
            temporalStates: uiState.temporals,
            getOrInitInputValue: (k, attrs) => getOrInitInputState(k, attrs),
            sliders: uiState.sliders,
            sliderBounds: uiState.sliderBounds,
            sliderDrags: uiState.sliderDrags,
            selects: uiState.selects,
            selectPopups,
            uiFocus: uiState,
            getPointerId: getEffectivePointerId,
            getCursorColor,
            requestPaint,
        });
    }
    // Draw <select> popups last so they appear above later siblings.
    if (selectPopups.length > 0) {
        for (const p of selectPopups) {
            renderSelectPopup({
                popup: p,
                stage: overlayRoot,
                theme,
                selectStates: uiState.selects,
                uiState,
                getPointerId: getEffectivePointerId,
                requestPaint,
                viewportW: app.renderer.width,
                viewportH: app.renderer.height,
            });
        }
    }
    // Context menu overlay.
    for (const [ownerPid, menuState] of uiState.contextMenus.entries()) {
        if (!menuState?.open)
            continue;
        const menu = new Container();
        menu.eventMode = 'static';
        menu.cursor = 'default';
        menu.position.set(menuState.x, menuState.y);
        const itemW = 140;
        const itemH = 28;
        const pad = 6;
        const labels = ['Copy', 'Paste', 'Close'];
        const bg = new Graphics();
        bg.rect(0, 0, itemW + pad * 2, labels.length * itemH + pad * 2);
        bg.fill(0xffffff);
        // Owner-colored 2px border.
        const borderInset = 1;
        bg.rect(borderInset, borderInset, itemW + pad * 2 - borderInset * 2, labels.length * itemH + pad * 2 - borderInset * 2);
        bg.stroke({ width: 2, color: getCursorColor(ownerPid), alignment: 0 });
        menu.addChild(bg);
        labels.forEach((label, i) => {
            const y = pad + i * itemH;
            const hit = new Container();
            hit.eventMode = 'static';
            hit.cursor = 'pointer';
            hit.position.set(pad, y);
            const gg = new Graphics();
            gg.rect(0, 0, itemW, itemH);
            gg.fill(0xffffff);
            hit.addChild(gg);
            const tt = makeThemedText({
                text: label,
                fontFamily: theme.fontFamily,
                fontSize: theme.fontSize,
                fill: theme.text,
            });
            tt.position.set(8, Math.max(0, (itemH - tt.height) / 2) + TEXT_BASELINE_NUDGE_Y);
            hit.addChild(tt);
            const isOwnerEvent = (ev) => getEffectivePointerId(ev) === ownerPid;
            hit.on('pointerover', (ev) => {
                if (!isOwnerEvent(ev))
                    return;
                gg.clear();
                gg.rect(0, 0, itemW, itemH);
                gg.fill(0xf2f2f2);
            });
            hit.on('pointerout', (ev) => {
                if (!isOwnerEvent(ev))
                    return;
                gg.clear();
                gg.rect(0, 0, itemW, itemH);
                gg.fill(0xffffff);
            });
            hit.on('pointerdown', (ev) => {
                if (!isOwnerEvent(ev))
                    return;
                ev.stopPropagation?.();
                const focusedKey = uiState.focusedKeyByPointer.get(ownerPid) ?? null;
                const focusedState = focusedKey ? uiState.inputs.get(focusedKey) : null;
                // Only allow Copy/Paste for text-like fields (<input>/<textarea>) that registered bounds this paint.
                const isTextField = focusedKey != null &&
                    uiState.fieldBounds.has(focusedKey) &&
                    focusedState != null &&
                    typeof focusedState.value === 'string';
                if (label === 'Copy' && isTextField) {
                    const st = focusedState;
                    const full = st.value ?? '';
                    const sel = st.selections?.get(ownerPid) ?? null;
                    const a = sel ? Math.max(0, Math.min(full.length, sel.start ?? 0)) : 0;
                    const b = sel ? Math.max(0, Math.min(full.length, sel.end ?? a)) : a;
                    const start = Math.min(a, b);
                    const end = Math.max(a, b);
                    const picked = start !== end ? full.slice(start, end) : full;
                    uiState.clipboards.set(ownerPid, picked);
                }
                else if (label === 'Paste' && isTextField) {
                    const clip = uiState.clipboards.get(ownerPid) ?? '';
                    if (clip.length > 0) {
                        const st = focusedState;
                        const full = st.value ?? '';
                        if (!st.selections)
                            st.selections = new Map();
                        if (!st.selections.has(ownerPid)) {
                            const p = full.length;
                            st.selections.set(ownerPid, { start: p, end: p });
                        }
                        const sel = st.selections.get(ownerPid);
                        const a = Math.max(0, Math.min(full.length, sel.start ?? full.length));
                        const b = Math.max(0, Math.min(full.length, sel.end ?? a));
                        const start = Math.min(a, b);
                        const end = Math.max(a, b);
                        st.value = full.slice(0, start) + clip + full.slice(end);
                        const caret = start + clip.length;
                        sel.start = caret;
                        sel.end = caret;
                    }
                }
                // Close on selection (including Close).
                const st = uiState.contextMenus.get(ownerPid);
                if (st) {
                    st.open = false;
                    uiState.contextMenus.set(ownerPid, st);
                }
                requestPaint?.();
            });
            menu.addChild(hit);
        });
        overlayRoot.addChild(menu);
    }
    // Prune cached layout node containers that weren't visited this paint.
    for (const [k, c] of nodeCache.entries()) {
        if (usedNodeKeys.has(k))
            continue;
        try {
            c.removeFromParent();
            c.destroy?.({ children: true });
        }
        catch {
            // Best-effort.
        }
        nodeCache.delete(k);
    }
    // Retained-mode renderer: we keep a stable scene graph rooted at `stage`.
    // Do not clear or re-add `stage` (it may be `sceneRoot` itself).
}
async function main() {
    const rootEl = document.getElementById('app') ?? document.body;
    const app = new Application();
    await app.init({ background: '#ffffff', resizeTo: window, antialias: false, preference: 'webgpu' });
    rootEl.appendChild(app.canvas);
    // We render on-demand (after state/layout changes) rather than continuously.
    // This saves substantial GPU time when the UI is static.
    app.ticker.stop();
    // Disable the browser context menu over the canvas.
    app.canvas.addEventListener('contextmenu', (e) => e.preventDefault());
    // Wheel scroll: scene-level global scrollbar.
    app.canvas.addEventListener('wheel', (e) => {
        const x = e.offsetX ?? 0;
        const y = e.offsetY ?? 0;
        // Deepest iframe under pointer wins.
        let iframeKey = null;
        for (let i = uiState.iframeRects.length - 1; i >= 0; i--) {
            const r = uiState.iframeRects[i];
            if (x >= r.x && x <= r.x + r.w && y >= r.y && y <= r.y + r.h) {
                iframeKey = r.key;
                break;
            }
        }
        if (iframeKey) {
            const st = uiState.iframeScroll.get(iframeKey);
            if (st) {
                const maxScroll = Math.max(0, st.contentHeight - st.viewportHeight);
                if (maxScroll > 0) {
                    st.y = Math.max(0, Math.min(maxScroll, st.y + e.deltaY));
                    uiState.iframeScroll.set(iframeKey, st);
                    requestPaint?.();
                    e.preventDefault();
                }
                return;
            }
        }
        const maxScroll = Math.max(0, uiState.scroll.contentHeight - uiState.scroll.viewportHeight);
        if (maxScroll <= 0)
            return;
        uiState.scroll.y = Math.max(0, Math.min(maxScroll, uiState.scroll.y + e.deltaY));
        requestPaint?.();
        e.preventDefault();
    }, { passive: false });
    // Make sure the stage participates in hit testing.
    app.stage.eventMode = 'static';
    app.stage.hitArea = app.screen;
    // Global context menu + "outside click" behavior.
    // This must be registered once (retained scene); widget handlers can stopPropagation.
    app.stage.on('pointerdown', (ev) => {
        if (ev?.button === 2) {
            const pid = getEffectivePointerId(ev);
            if (pid > 0) {
                const m = uiState.contextMenus.get(pid) ?? { open: false, x: 0, y: 0 };
                m.open = true;
                m.x = ev.global?.x ?? 0;
                m.y = ev.global?.y ?? 0;
                uiState.contextMenus.set(pid, m);
            }
            requestPaint?.();
            ev.preventDefault?.();
            return;
        }
        // Left click closes only THIS pointer's menu (clicks from other pointers don't dismiss it).
        if (ev?.button !== 2) {
            const pid = getEffectivePointerId(ev);
            const m = pid > 0 ? uiState.contextMenus.get(pid) : null;
            if (m && m.open) {
                m.open = false;
                uiState.contextMenus.set(pid, m);
                requestPaint?.();
            }
        }
        // Left click outside closes any open <select> popups.
        if (ev?.button !== 2) {
            let didClose = false;
            for (const st of uiState.selects.values()) {
                if (st.open) {
                    st.open = false;
                    didClose = true;
                }
            }
            if (didClose)
                requestPaint?.();
        }
        // Left click outside closes any open temporal pickers.
        if (ev?.button !== 2) {
            const didCloseTemporal = closeAllTemporalPopups(uiState.temporals);
            if (didCloseTemporal)
                requestPaint?.();
        }
        // Some interactions (e.g. hover/active fills) mutate Graphics directly; ensure we present.
        requestPresent();
    });
    const sceneRoot = new Container();
    const overlayUiRoot = new Container();
    overlayUiRoot.eventMode = 'static';
    // Overlay sits above the scene, but must not steal input.
    const overlayRoot = new Container();
    overlayRoot.eventMode = 'none';
    app.stage.addChild(sceneRoot);
    app.stage.addChild(overlayUiRoot);
    app.stage.addChild(overlayRoot);
    const scrollbarG = new Graphics();
    scrollbarG.eventMode = 'static';
    overlayUiRoot.addChild(scrollbarG);
    const buildCrossShape = (g, opts) => {
        g.clear();
        const { half, strokeWidth, color } = opts;
        g.moveTo(-half, 0);
        g.lineTo(half, 0);
        g.stroke({ width: strokeWidth, color });
        g.moveTo(0, -half);
        g.lineTo(0, half);
        g.stroke({ width: strokeWidth, color });
    };
    // Primary (real) cursor overlay.
    const mouseCursorG = new Graphics();
    mouseCursorG.eventMode = 'none';
    mouseCursorG.visible = false;
    overlayRoot.addChild(mouseCursorG);
    const cursor3G = new Graphics();
    cursor3G.eventMode = 'none';
    cursor3G.visible = false;
    overlayRoot.addChild(cursor3G);
    const cursor4G = new Graphics();
    cursor4G.eventMode = 'none';
    cursor4G.visible = false;
    overlayRoot.addChild(cursor4G);
    const virtualCursorG = new Graphics();
    virtualCursorG.eventMode = 'none';
    overlayRoot.addChild(virtualCursorG);
    const dragMeasureCanvas = document.createElement('canvas');
    const dragMeasureCtx = dragMeasureCanvas.getContext('2d');
    if (!dragMeasureCtx)
        throw new Error('2D canvas not available');
    dragMeasureCtx.font = `${defaultTheme.fontSize}px ${defaultTheme.fontFamily}`;
    const dragMeasure = (s) => dragMeasureCtx.measureText(s).width;
    const dragLineHeight = defaultTheme.fontSize * 1.25;
    let html = '';
    try {
        html = await fetch('/input.html').then((r) => r.text());
    }
    catch {
        html = '<!doctype html><html><body><h1>TRUEOS Parse5 Browser</h1><p>Fallback input rendered (missing /input.html).</p><input type="text" value="hello" /><button>ok</button></body></html>';
    }
    const doc = parse5.parse(html);
    const body = getBody(doc) ?? doc;
    const innerRenderNodes = toRenderTree(body, '0');
    // Conceptual model: the top-level document is itself a hidden iframe.
    // This lets us evolve iframe semantics without changing input.html.
    const renderNodes = [
        {
            kind: 'block',
            key: 'root:internal-iframe',
            tagName: 'iframe',
            attrs: { 'data-root': '1' },
            children: innerRenderNodes,
        },
    ];
    let lastLayout = null;
    const clampScroll = () => {
        const maxScroll = Math.max(0, uiState.scroll.contentHeight - uiState.scroll.viewportHeight);
        uiState.scroll.y = Math.max(0, Math.min(uiState.scroll.y, maxScroll));
    };
    const updateScrollbarVisuals = () => {
        const vw = app.renderer.width;
        const vh = app.renderer.height;
        uiState.scroll.viewportHeight = vh;
        const contentH = uiState.scroll.contentHeight;
        const maxScroll = Math.max(0, contentH - vh);
        const show = maxScroll > 0.5;
        scrollbarG.clear();
        if (!show) {
            uiState.scroll.track = { x: 0, y: 0, w: uiState.scroll.track.w, h: 0 };
            uiState.scroll.thumb = { x: 0, y: 0, w: uiState.scroll.thumb.w, h: 0 };
            return;
        }
        const pad = SCROLLBAR_PAD;
        const trackW = SCROLLBAR_W;
        const trackX = Math.max(0, vw - trackW - pad);
        const trackY = pad;
        const trackH = Math.max(0, vh - pad * 2);
        const minThumbH = 24;
        const thumbH = Math.max(minThumbH, (vh / Math.max(vh, contentH)) * trackH);
        const travel = Math.max(1, trackH - thumbH);
        const ratio = maxScroll <= 0 ? 0 : uiState.scroll.y / maxScroll;
        const thumbY = trackY + travel * ratio;
        uiState.scroll.track = { x: trackX, y: trackY, w: trackW, h: trackH };
        uiState.scroll.thumb = { x: trackX, y: thumbY, w: trackW, h: thumbH };
        // Track
        scrollbarG.rect(trackX, trackY, trackW, trackH);
        scrollbarG.fill({ color: 0x000000, alpha: 0.06 });
        // Thumb
        scrollbarG.rect(trackX, thumbY, trackW, thumbH);
        scrollbarG.fill({ color: 0x000000, alpha: 0.25 });
    };
    const paint = () => {
        if (!lastLayout)
            return;
        clampScroll();
        renderToPixi(app, lastLayout, sceneRoot);
        updateScrollbarVisuals();
        // Manual render (ticker is stopped).
        app.renderer.render(app.stage);
    };
    const rerender = async () => {
        const layout = await buildLayoutTree(renderNodes, window.innerWidth, window.innerHeight);
        lastLayout = layout;
        uiState.scroll.contentHeight = computeScrollableContentHeight(layout);
        uiState.scroll.viewportHeight = window.innerHeight;
        paint();
    };
    requestRerender = () => {
        void rerender();
    };
    // Coalesce paints to at most once per frame.
    let paintScheduled = false;
    let presentScheduled = false;
    const requestPresent = () => {
        if (presentScheduled || paintScheduled)
            return;
        presentScheduled = true;
        requestAnimationFrame(() => {
            presentScheduled = false;
            app.renderer.render(app.stage);
        });
    };
    requestPaint = () => {
        if (paintScheduled)
            return;
        paintScheduled = true;
        requestAnimationFrame(() => {
            paintScheduled = false;
            paint();
        });
    };
    await rerender();
    // Cursor style shared between real + virtual cursor.
    // <details> chevron uses a 2px stroke; match that.
    const CURSOR_STROKE = 2;
    // Double the previous arm length.
    const CURSOR_HALF = 10;
    buildCrossShape(mouseCursorG, { half: CURSOR_HALF, strokeWidth: CURSOR_STROKE, color: getCursorColor(USER_POINTER_ID) });
    buildCrossShape(cursor3G, { half: CURSOR_HALF, strokeWidth: CURSOR_STROKE, color: getCursorColor(USER_POINTER_ID_3) });
    buildCrossShape(cursor4G, { half: CURSOR_HALF, strokeWidth: CURSOR_STROKE, color: getCursorColor(USER_POINTER_ID_4) });
    // Virtual cursor uses a distinct id so it can have a distinct color.
    const VIRTUAL_POINTER_ID = 2;
    buildCrossShape(virtualCursorG, { half: CURSOR_HALF, strokeWidth: CURSOR_STROKE, color: getCursorColor(VIRTUAL_POINTER_ID) });
    // Seed positions so both cursors can be visible immediately.
    uiState.userCursorPos.set(USER_POINTER_ID, { x: app.renderer.width * 0.25, y: app.renderer.height * 0.5 });
    uiState.userCursorPos.set(USER_POINTER_ID_3, { x: app.renderer.width * 0.25 + 40, y: app.renderer.height * 0.5 + 20 });
    uiState.userCursorPos.set(USER_POINTER_ID_4, { x: app.renderer.width * 0.25 + 80, y: app.renderer.height * 0.5 + 40 });
    mouseCursorG.visible = true;
    cursor3G.visible = true;
    cursor4G.visible = true;
    {
        const p1 = uiState.userCursorPos.get(USER_POINTER_ID);
        const p3 = uiState.userCursorPos.get(USER_POINTER_ID_3);
        const p4 = uiState.userCursorPos.get(USER_POINTER_ID_4);
        mouseCursorG.position.set(p1.x, p1.y);
        cursor3G.position.set(p3.x, p3.y);
        cursor4G.position.set(p4.x, p4.y);
    }
    virtualCursorG.visible = uiState.virtualCursor.enabled;
    const updateUserCursorOverlays = () => {
        // Keep overlays and hover state in sync with uiState.userCursorPos.
        const p1 = uiState.userCursorPos.get(USER_POINTER_ID);
        const p3 = uiState.userCursorPos.get(USER_POINTER_ID_3);
        const p4 = uiState.userCursorPos.get(USER_POINTER_ID_4);
        if (p1) {
            mouseCursorG.visible = true;
            mouseCursorG.position.set(p1.x, p1.y);
        }
        if (p3) {
            cursor3G.visible = true;
            cursor3G.position.set(p3.x, p3.y);
        }
        if (p4) {
            cursor4G.visible = true;
            cursor4G.position.set(p4.x, p4.y);
        }
        const findHover = (x, y) => {
            let hitKey = null;
            let hitCursor = null;
            for (let i = uiState.hoverRects.length - 1; i >= 0; i--) {
                const rct = uiState.hoverRects[i];
                if (x >= rct.x && x <= rct.x + rct.w && y >= rct.y && y <= rct.y + rct.h) {
                    hitKey = rct.key;
                    hitCursor = rct.cursor;
                    break;
                }
            }
            return { hitKey, hitCursor };
        };
        if (p1) {
            const { hitKey, hitCursor } = findHover(p1.x, p1.y);
            uiState.hoveredKeyByPointer.set(USER_POINTER_ID, hitKey);
            uiState.hoveredCursorByPointer.set(USER_POINTER_ID, hitCursor);
            const isActive = uiState.textDrags.has(USER_POINTER_ID) || uiState.sliderDrags.has(USER_POINTER_ID) || uiState.dialogDrags.has(USER_POINTER_ID);
            mouseCursorG.rotation = hitCursor != null || isActive ? Math.PI / 4 : 0;
        }
        if (p3) {
            const { hitKey, hitCursor } = findHover(p3.x, p3.y);
            uiState.hoveredKeyByPointer.set(USER_POINTER_ID_3, hitKey);
            uiState.hoveredCursorByPointer.set(USER_POINTER_ID_3, hitCursor);
            const isActive = uiState.textDrags.has(USER_POINTER_ID_3) || uiState.sliderDrags.has(USER_POINTER_ID_3) || uiState.dialogDrags.has(USER_POINTER_ID_3);
            cursor3G.rotation = hitCursor != null || isActive ? Math.PI / 4 : 0;
        }
        if (p4) {
            const { hitKey, hitCursor } = findHover(p4.x, p4.y);
            uiState.hoveredKeyByPointer.set(USER_POINTER_ID_4, hitKey);
            uiState.hoveredCursorByPointer.set(USER_POINTER_ID_4, hitCursor);
            const isActive = uiState.textDrags.has(USER_POINTER_ID_4) || uiState.sliderDrags.has(USER_POINTER_ID_4) || uiState.dialogDrags.has(USER_POINTER_ID_4);
            cursor4G.rotation = hitCursor != null || isActive ? Math.PI / 4 : 0;
        }
        // Cursor overlays and hover-driven visuals update without needing a full paint traversal.
        requestPresent();
    };
    // Multi-cursor harness: cycle mouse control between cursor 1, cursor 3, cursor 4.
    // When enabled, this runs a periodic timer.
    if (uiState.harness.enabled) {
        setInterval(() => {
            const prev = uiState.harness.activeUserPointerId;
            const next = prev === USER_POINTER_ID ? USER_POINTER_ID_3 : prev === USER_POINTER_ID_3 ? USER_POINTER_ID_4 : USER_POINTER_ID;
            uiState.harness.activeUserPointerId = next;
            // Make the toggle visible immediately even if the real mouse hasn't moved:
            // snap the newly-controlled cursor to the last known mouse position and
            // move the previously-controlled cursor back to where the new cursor was.
            if (uiState.lastMouse.has) {
                const prevPos = uiState.userCursorPos.get(prev);
                const nextPos = uiState.userCursorPos.get(next);
                uiState.userCursorPos.set(next, { x: uiState.lastMouse.x, y: uiState.lastMouse.y });
                if (nextPos)
                    uiState.userCursorPos.set(prev, { x: nextPos.x, y: nextPos.y });
                else if (prevPos)
                    uiState.userCursorPos.set(prev, { x: prevPos.x, y: prevPos.y });
            }
            // If we cancel an active drag/scroll, the main scene visuals can change, so repaint.
            const hadTextDrag = uiState.textDrags.size > 0;
            const hadSliderDrag = uiState.sliderDrags.size > 0;
            const hadDialogDrag = uiState.dialogDrags.size > 0;
            const hadScrollDrag = uiState.scroll.draggingPointerId != null;
            const hadColorDrag = uiState.color.draggingPointerId != null;
            let hadIframeDrag = false;
            for (const st of uiState.iframeScroll.values()) {
                if (st.draggingPointerId != null) {
                    hadIframeDrag = true;
                    break;
                }
            }
            const needsRepaint = hadTextDrag || hadSliderDrag || hadDialogDrag || hadScrollDrag || hadColorDrag || hadIframeDrag;
            // Avoid stuck drags when control flips mid-gesture.
            uiState.textDrags.delete(USER_POINTER_ID);
            uiState.textDrags.delete(USER_POINTER_ID_3);
            uiState.textDrags.delete(USER_POINTER_ID_4);
            uiState.sliderDrags.delete(USER_POINTER_ID);
            uiState.sliderDrags.delete(USER_POINTER_ID_3);
            uiState.sliderDrags.delete(USER_POINTER_ID_4);
            uiState.dialogDrags.delete(USER_POINTER_ID);
            uiState.dialogDrags.delete(USER_POINTER_ID_3);
            uiState.dialogDrags.delete(USER_POINTER_ID_4);
            // Avoid stuck number holds.
            for (const pid of [USER_POINTER_ID, USER_POINTER_ID_3, USER_POINTER_ID_4]) {
                const h = uiState.numberHolds.get(pid);
                if (h) {
                    if (h.timeoutId != null)
                        window.clearTimeout(h.timeoutId);
                    if (h.intervalId != null)
                        window.clearInterval(h.intervalId);
                    uiState.numberHolds.delete(pid);
                }
            }
            if (uiState.scroll.draggingPointerId === USER_POINTER_ID ||
                uiState.scroll.draggingPointerId === USER_POINTER_ID_3 ||
                uiState.scroll.draggingPointerId === USER_POINTER_ID_4) {
                uiState.scroll.draggingPointerId = null;
            }
            if (uiState.color.draggingPointerId === USER_POINTER_ID ||
                uiState.color.draggingPointerId === USER_POINTER_ID_3 ||
                uiState.color.draggingPointerId === USER_POINTER_ID_4) {
                uiState.color.draggingPointerId = null;
            }
            updateUserCursorOverlays();
            if (needsRepaint)
                requestPaint?.();
        }, uiState.harness.periodMs);
    }
    // Virtual input device cursor: simple patrol (circle).
    // Disabled by default; when disabled we avoid per-frame ticker work.
    if (uiState.virtualCursor.enabled) {
        app.ticker.add(() => {
            const dt = Math.max(0, app.ticker.deltaMS) / 1000;
            virtualCursorG.visible = true;
            uiState.virtualCursor.t += dt;
            const cx = app.renderer.width * 0.75;
            const cy = app.renderer.height * 0.25;
            const a = uiState.virtualCursor.t * uiState.virtualCursor.speed;
            const r = uiState.virtualCursor.radius;
            uiState.virtualCursor.x = cx + Math.cos(a) * r;
            uiState.virtualCursor.y = cy + Math.sin(a) * r;
            virtualCursorG.position.set(uiState.virtualCursor.x, uiState.virtualCursor.y);
            // Virtual hover simulation.
            {
                const pid = VIRTUAL_POINTER_ID;
                const x = uiState.virtualCursor.x;
                const y = uiState.virtualCursor.y;
                let hitKey = null;
                let hitCursor = null;
                // Iterate from end so later-drawn widgets win.
                for (let i = uiState.hoverRects.length - 1; i >= 0; i--) {
                    const rct = uiState.hoverRects[i];
                    if (x >= rct.x && x <= rct.x + rct.w && y >= rct.y && y <= rct.y + rct.h) {
                        hitKey = rct.key;
                        hitCursor = rct.cursor;
                        break;
                    }
                }
                const prev = uiState.hoveredKeyByPointer.get(pid) ?? null;
                if (prev !== hitKey) {
                    if (prev)
                        uiState.hoverHandlers.get(prev)?.out?.();
                    if (hitKey)
                        uiState.hoverHandlers.get(hitKey)?.over?.();
                    uiState.hoveredKeyByPointer.set(pid, hitKey);
                }
                uiState.hoveredCursorByPointer.set(pid, hitCursor);
                const isActive = uiState.textDrags.has(pid) || uiState.sliderDrags.has(pid) || uiState.dialogDrags.has(pid);
                virtualCursorG.rotation = hitCursor != null || isActive ? Math.PI / 4 : 0;
            }
        });
    }
    // Initial cursor draw.
    uiState.virtualCursor.x = app.renderer.width * 0.75 + uiState.virtualCursor.radius;
    uiState.virtualCursor.y = app.renderer.height * 0.25;
    virtualCursorG.position.set(uiState.virtualCursor.x, uiState.virtualCursor.y);
    // Mouse drag selection for <input>/<textarea>.
    // Also used for slider drag, dialog drag, and scrollbar thumb drag.
    app.stage.on('pointerup', (ev) => {
        const pid = getEffectivePointerId(ev);
        const releasedSliderKey = uiState.sliderDrags.get(pid)?.key ?? null;
        uiState.textDrags.delete(pid);
        uiState.sliderDrags.delete(pid);
        uiState.dialogDrags.delete(pid);
        if (uiState.scroll.draggingPointerId === pid)
            uiState.scroll.draggingPointerId = null;
        if (uiState.color.draggingPointerId === pid)
            uiState.color.draggingPointerId = null;
        for (const st of uiState.iframeScroll.values()) {
            if (st.draggingPointerId === pid)
                st.draggingPointerId = null;
        }
        // End number spinner hold-repeat.
        {
            const h = uiState.numberHolds.get(pid);
            if (h) {
                if (h.timeoutId != null)
                    window.clearTimeout(h.timeoutId);
                if (h.intervalId != null)
                    window.clearInterval(h.intervalId);
                uiState.numberHolds.delete(pid);
            }
        }
        // Year widget: close on slider release.
        if (releasedSliderKey) {
            const ownerTemporalKey = uiState.temporalYearOwners.get(releasedSliderKey) ?? null;
            if (ownerTemporalKey) {
                const t = uiState.temporals.get(ownerTemporalKey);
                if (t && t.openYear) {
                    t.openYear = false;
                    uiState.temporals.set(ownerTemporalKey, t);
                    requestPaint?.();
                }
            }
        }
        // Ensure pointerup-driven visuals (e.g. button state) are presented.
        requestPresent();
    });
    app.stage.on('pointerupoutside', (ev) => {
        const pid = getEffectivePointerId(ev);
        const releasedSliderKey = uiState.sliderDrags.get(pid)?.key ?? null;
        uiState.textDrags.delete(pid);
        uiState.sliderDrags.delete(pid);
        uiState.dialogDrags.delete(pid);
        if (uiState.scroll.draggingPointerId === pid)
            uiState.scroll.draggingPointerId = null;
        if (uiState.color.draggingPointerId === pid)
            uiState.color.draggingPointerId = null;
        for (const st of uiState.iframeScroll.values()) {
            if (st.draggingPointerId === pid)
                st.draggingPointerId = null;
        }
        // End number spinner hold-repeat.
        {
            const h = uiState.numberHolds.get(pid);
            if (h) {
                if (h.timeoutId != null)
                    window.clearTimeout(h.timeoutId);
                if (h.intervalId != null)
                    window.clearInterval(h.intervalId);
                uiState.numberHolds.delete(pid);
            }
        }
        // Year widget: close on slider release.
        if (releasedSliderKey) {
            const ownerTemporalKey = uiState.temporalYearOwners.get(releasedSliderKey) ?? null;
            if (ownerTemporalKey) {
                const t = uiState.temporals.get(ownerTemporalKey);
                if (t && t.openYear) {
                    t.openYear = false;
                    uiState.temporals.set(ownerTemporalKey, t);
                    requestPaint?.();
                }
            }
        }
        requestPresent();
    });
    // Thumb drag start (last cursor wins).
    scrollbarG.on('pointerdown', (ev) => {
        if (ev?.button === 2)
            return;
        const pid = getEffectivePointerId(ev);
        if (pid <= 0)
            return;
        const gx = ev.global?.x ?? 0;
        const gy = ev.global?.y ?? 0;
        const track = uiState.scroll.track;
        const th = uiState.scroll.thumb;
        const hitTrack = gx >= track.x && gx <= track.x + track.w && gy >= track.y && gy <= track.y + track.h;
        if (!hitTrack)
            return;
        const maxScroll = Math.max(0, uiState.scroll.contentHeight - uiState.scroll.viewportHeight);
        if (maxScroll <= 0.5)
            return;
        const hitThumb = gx >= th.x && gx <= th.x + th.w && gy >= th.y && gy <= th.y + th.h;
        if (hitThumb) {
            uiState.scroll.draggingPointerId = pid;
            uiState.scroll.dragOffsetY = gy - th.y;
            ev.stopPropagation?.();
            return;
        }
        // Track click: jump the thumb (centered on the pointer) and start dragging.
        const travel = Math.max(1, track.h - th.h);
        const targetTop = Math.max(track.y, Math.min(track.y + travel, gy - th.h / 2));
        const ratio = (targetTop - track.y) / travel;
        uiState.scroll.y = Math.max(0, Math.min(maxScroll, ratio * maxScroll));
        uiState.scroll.draggingPointerId = pid;
        uiState.scroll.dragOffsetY = gy - targetTop;
        requestPaint?.();
        ev.stopPropagation?.();
    });
    app.stage.on('pointermove', (ev) => {
        // Track the primary (real) cursor separately from drag-selection pointers.
        // Prefer pointerType when available; fallback to pointerId==1 (typical mouse).
        const pidAny = Number(ev?.pointerId ?? ev?.data?.pointerId ?? 1);
        const pt = String(ev?.pointerType ?? ev?.data?.pointerType ?? '').toLowerCase();
        const isMouse = pt === 'mouse' || pidAny === 1;
        if (isMouse) {
            const gx = ev.global?.x ?? 0;
            const gy = ev.global?.y ?? 0;
            uiState.lastMouse.x = gx;
            uiState.lastMouse.y = gy;
            uiState.lastMouse.has = true;
            uiState.primaryMousePointerId = pidAny;
            // Update the stored position for whichever user cursor the harness says is under mouse control.
            const controlPid = uiState.harness.enabled ? uiState.harness.activeUserPointerId : pidAny;
            uiState.userCursorPos.set(controlPid, { x: gx, y: gy });
            // Keep overlays/hover in sync as the real mouse moves.
            updateUserCursorOverlays();
        }
        const pid = getEffectivePointerId(ev);
        if (pid <= 0)
            return;
        let didUpdate = false;
        // Text selection drag.
        {
            const drag = uiState.textDrags.get(pid);
            if (drag) {
                const key = drag.key;
                const bounds = uiState.fieldBounds.get(key);
                const state = uiState.inputs.get(key);
                if (bounds && state && typeof state.value === 'string') {
                    const shown = bounds.isPassword ? '•'.repeat(state.value.length) : state.value;
                    const lines = clampWrappedLines(wrapFieldTextWithIndices(shown, Math.max(0, bounds.innerWidth), dragMeasure), bounds.maxLines);
                    const localX = (ev.global?.x ?? 0) - bounds.x - bounds.innerLeft;
                    const localY = (ev.global?.y ?? 0) - bounds.y - bounds.innerTop;
                    const idx = getCaretIndexFromPoint({
                        fullText: shown,
                        lines,
                        localX,
                        localY,
                        lineHeight: dragLineHeight,
                        measure: dragMeasure,
                    });
                    if (!state.selections)
                        state.selections = new Map();
                    state.selections.set(pid, { start: drag.anchor, end: idx });
                    didUpdate = true;
                }
            }
        }
        // Slider drag.
        {
            const drag = uiState.sliderDrags.get(pid);
            if (drag) {
                const key = drag.key;
                const b = uiState.sliderBounds.get(key);
                if (b) {
                    const gx = ev.global?.x ?? 0;
                    const localX = gx - b.x;
                    const innerW2 = Math.max(1, b.w - b.innerPad * 2);
                    const r = (localX - b.innerPad) / innerW2;
                    const s = widgetGetOrInitSliderState(uiState.sliders, key, undefined);
                    s.value = Math.max(0, Math.min(1, r));
                    didUpdate = true;
                }
            }
        }
        // Color picker drag.
        {
            const dragPid = uiState.color.draggingPointerId;
            if (dragPid != null && dragPid === pid) {
                const b = uiState.color.bounds;
                if (b) {
                    const gx = ev.global?.x ?? 0;
                    const gy = ev.global?.y ?? 0;
                    const lx = gx - b.x;
                    const ly = gy - b.y;
                    const s = sampleColorPickerAtLocal({ lx, ly, w: b.w, h: b.h });
                    if (s) {
                        uiState.color.rgb = s;
                        uiState.color.pick = { x: lx, y: ly };
                        didUpdate = true;
                    }
                }
            }
        }
        // Dialog drag.
        // Scrollbar thumb drag.
        {
            const dragPid = uiState.scroll.draggingPointerId;
            if (dragPid != null && dragPid === pid) {
                const track = uiState.scroll.track;
                const thumb = uiState.scroll.thumb;
                const maxScroll = Math.max(0, uiState.scroll.contentHeight - uiState.scroll.viewportHeight);
                if (maxScroll > 0.5 && track.h > 0 && thumb.h > 0) {
                    const gy = ev.global?.y ?? 0;
                    const travel = Math.max(1, track.h - thumb.h);
                    const top = Math.max(track.y, Math.min(track.y + travel, gy - uiState.scroll.dragOffsetY));
                    const ratio = (top - track.y) / travel;
                    uiState.scroll.y = Math.max(0, Math.min(maxScroll, ratio * maxScroll));
                    didUpdate = true;
                }
            }
        }
        // Iframe scrollbar thumb drag.
        {
            for (const st of uiState.iframeScroll.values()) {
                if (st.draggingPointerId == null || st.draggingPointerId !== pid)
                    continue;
                const maxScroll = Math.max(0, st.contentHeight - st.viewportHeight);
                if (maxScroll <= 0.5 || st.track.h <= 0 || st.thumb.h <= 0)
                    continue;
                const gy = ev.global?.y ?? 0;
                const travel = Math.max(1, st.track.h - st.thumb.h);
                const top = Math.max(st.track.y, Math.min(st.track.y + travel, gy - st.dragOffsetY));
                const ratio = (top - st.track.y) / travel;
                st.y = Math.max(0, Math.min(maxScroll, ratio * maxScroll));
                didUpdate = true;
            }
        }
        {
            const drag = uiState.dialogDrags.get(pid);
            if (drag) {
                const st = getOrInitDialogState(uiState.dialogs, drag.key);
                const gx = ev.global?.x ?? 0;
                const gy = ev.global?.y ?? 0;
                st.x = drag.originX + (gx - drag.startGX);
                st.y = drag.originY + (gy - drag.startGY);
                // Clamp to the most recently computed bounds for this dialog's stacking context.
                const b = uiState.dialogDragBounds.get(drag.key);
                if (b) {
                    st.x = Math.max(b.minX, Math.min(b.maxX, st.x));
                    st.y = Math.max(b.minY, Math.min(b.maxY, st.y));
                }
                didUpdate = true;
            }
        }
        if (didUpdate)
            requestPaint?.();
    });
    // Keyboard input: very lightweight text editing for focused <input type=text|password>.
    window.addEventListener('keydown', (ev) => {
        const pid = uiState.keyboardOwnerPointerId;
        const key = uiState.focusedKeyByPointer.get(pid) ?? null;
        if (!key)
            return;
        const state = uiState.inputs.get(key);
        if (!state)
            return;
        // Only text-like inputs have a value.
        if (typeof state.value !== 'string')
            return;
        // Selection helpers (keyboard focus pointer only)
        if (!state.selections)
            state.selections = new Map();
        if (!state.selections.has(pid)) {
            const p = state.value.length;
            state.selections.set(pid, { start: p, end: p });
        }
        const sel = state.selections.get(pid);
        const len = state.value.length;
        const clampPos = (n) => Math.max(0, Math.min(len, n));
        const a0 = clampPos(sel.start ?? len);
        const b0 = clampPos(sel.end ?? a0);
        sel.start = a0;
        sel.end = b0;
        const start0 = Math.min(a0, b0);
        const end0 = Math.max(a0, b0);
        const hasSel = start0 !== end0;
        const setCaret = (pos) => {
            const p = Math.max(0, Math.min(state.value.length, pos));
            sel.start = p;
            sel.end = p;
        };
        const setSelection = (start, end) => {
            sel.start = Math.max(0, Math.min(state.value.length, start));
            sel.end = Math.max(0, Math.min(state.value.length, end));
        };
        if (ev.key.toLowerCase() === 'a' && (ev.ctrlKey || ev.metaKey)) {
            setSelection(0, state.value.length);
            ev.preventDefault();
            paint();
            return;
        }
        if (ev.key === 'ArrowLeft' || ev.key === 'ArrowRight') {
            const dir = ev.key === 'ArrowLeft' ? -1 : 1;
            if (ev.shiftKey) {
                const anchor = sel.start ?? len;
                const focus = (sel.end ?? anchor) + dir;
                setSelection(anchor, focus);
            }
            else {
                const caret = hasSel ? start0 : end0;
                setCaret(caret + dir);
            }
            ev.preventDefault();
            void rerender();
            return;
        }
        if (ev.key === 'Home') {
            if (ev.shiftKey)
                setSelection(sel.start ?? len, 0);
            else
                setCaret(0);
            ev.preventDefault();
            void rerender();
            return;
        }
        if (ev.key === 'End') {
            if (ev.shiftKey)
                setSelection(sel.start ?? 0, state.value.length);
            else
                setCaret(state.value.length);
            ev.preventDefault();
            void rerender();
            return;
        }
        if (ev.key === 'Backspace') {
            if (hasSel) {
                state.value = state.value.slice(0, start0) + state.value.slice(end0);
                setCaret(start0);
            }
            else {
                const caret = end0;
                if (caret > 0) {
                    state.value = state.value.slice(0, caret - 1) + state.value.slice(caret);
                    setCaret(caret - 1);
                }
            }
            ev.preventDefault();
            void rerender();
            return;
        }
        if (ev.key === 'Enter') {
            // Multiline editing for demo: Enter inserts a newline.
            const insert = '\n';
            if (hasSel) {
                state.value = state.value.slice(0, start0) + insert + state.value.slice(end0);
                setCaret(start0 + insert.length);
            }
            else {
                const caret = end0;
                state.value = state.value.slice(0, caret) + insert + state.value.slice(caret);
                setCaret(caret + insert.length);
            }
            ev.preventDefault();
            void rerender();
            return;
        }
        if (ev.key === 'Delete') {
            if (hasSel) {
                state.value = state.value.slice(0, start0) + state.value.slice(end0);
                setCaret(start0);
            }
            else {
                const caret = end0;
                if (caret < state.value.length) {
                    state.value = state.value.slice(0, caret) + state.value.slice(caret + 1);
                    setCaret(caret);
                }
            }
            ev.preventDefault();
            void rerender();
            return;
        }
        if (ev.key === 'Escape') {
            uiState.focusedKeyByPointer.set(pid, null);
            void rerender();
            return;
        }
        if (ev.key.length === 1 && !ev.ctrlKey && !ev.metaKey && !ev.altKey) {
            if (hasSel) {
                state.value = state.value.slice(0, start0) + ev.key + state.value.slice(end0);
                setCaret(start0 + 1);
            }
            else {
                const caret = end0;
                state.value = state.value.slice(0, caret) + ev.key + state.value.slice(caret);
                setCaret(caret + 1);
            }
            ev.preventDefault();
            void rerender();
        }
    });
    window.addEventListener('resize', () => {
        void rerender();
        // Cursor is animated by ticker; ensure it stays visible immediately after resize.
        virtualCursorG.visible = uiState.virtualCursor.enabled;
    });
}

function formatErrorForLog(err) {
    if (err == null)
        return 'Unknown error';
    const anyErr = err;
    const name = String(anyErr?.name || 'Error');
    const message = String(anyErr?.message || err);
    const stack = (typeof anyErr?.stack === 'string' && anyErr.stack.length > 0)
        ? anyErr.stack
        : null;
    return stack ? `${name}: ${message}\n${stack}` : `${name}: ${message}`;
}

function logErrorWithStack(label, err) {
    const text = `[${label}] ${formatErrorForLog(err)}`;
    console.error(text);
    try {
        const pre = document.createElement('pre');
        pre.textContent = text;
        document.body.appendChild(pre);
    }
    catch {
        // Best-effort UI fallback only.
    }
}

if (typeof globalThis.addEventListener === 'function') {
    globalThis.addEventListener('error', (ev) => {
        logErrorWithStack('window.error', ev?.error ?? ev?.message ?? ev);
    });
    globalThis.addEventListener('unhandledrejection', (ev) => {
        logErrorWithStack('unhandledrejection', ev?.reason ?? ev);
    });
}

main().catch((err) => {
    logErrorWithStack('main.catch', err);
});
