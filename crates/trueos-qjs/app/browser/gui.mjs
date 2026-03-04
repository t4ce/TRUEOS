import { Application, Container, Graphics } from 'pixi.js';
import * as parse5 from 'parse5';
import Yoga from 'yoga-layout';
import * as browserContext from 'trueos:browser_context';
import { INPUT_HTML } from './input_html.mjs';
import { defaultTheme } from './theme.mjs';
import { BLOCK_TAGS } from './htmlDefaults.mjs';
// SVG generation/parsing helpers live in widget modules.
import { makeThemedText, TEXT_BASELINE_NUDGE_Y, WRAP_EPSILON_PX } from './text.mjs';
import { clearGraphics, getOrCreateContainer, getOrCreateGraphics, getOrCreateText } from './pixiReuse.mjs';
import { renderCursorPlaneFrame, renderDirectCmdFrame } from './cmd_backend.mjs';
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
import {
    SCROLLBAR_PAD,
    SCROLLBAR_W,
    USER_POINTER_ID,
    USE_DIRECT_CMD_BACKEND,
    TRACE_POSITION_FLOW,
    TRACE_YOGA_LIFECYCLE,
    USE_CURSOR_PLANE_TICK,
    CURSOR_PLANE_TICK_MS,
    GLOBAL_SCROLL_DIRTY_KEY,
    RT_GLOBAL,
    RT_WINDOW,
    RT_DOCUMENT,
    RT_HAS_WINDOW_RESIZE,
    RT_EVENT_TARGET,
    IS_TRUEOS_QJS_RUNTIME,
    uiState,
    getCursorColor,
    getEffectivePointerId,
    getMenuOwnerPointerId,
    logCursorButtonEvent,
    computeScrollableContentHeight,
    countLayoutNodes,
} from './ui.mjs';

// Singleton canvas/context for text measurement during rendering (used by inputs/textarea).
let renderMeasureCtx = null;
function getRenderMeasure(theme) {
    if (!renderMeasureCtx) {
        const c = RT_DOCUMENT?.createElement?.('canvas');
        if (!c)
            throw new Error('canvas element not available');
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
let requestPresent = null;

function collectPixiSnapshotItems(roots, viewportW, viewportH, maxItems = 480) {
    const out = [];
    const stack = [];
    for (let i = 0; i < roots.length; i++) {
        const r = roots[i];
        if (r)
            stack.push({ node: r, depth: 0 });
    }

    while (stack.length > 0 && out.length < maxItems) {
        const cur = stack.pop();
        const n = cur?.node;
        const depth = Number(cur?.depth || 0) | 0;
        if (!n || typeof n !== 'object')
            continue;
        if (n.visible === false)
            continue;
        const alpha = Number(n.worldAlpha ?? n.alpha ?? 1);
        if (!(alpha > 0.001))
            continue;

        const kids = Array.isArray(n.children) ? n.children : [];
        for (let i = kids.length - 1; i >= 0; i--) {
            stack.push({ node: kids[i], depth: depth + 1 });
        }

        // Prefer concrete leaf drawables instead of aggregate container bounds.
        if (kids.length > 0)
            continue;

        if (typeof n.getBounds !== 'function')
            continue;
        let b = null;
        try {
            b = n.getBounds();
        }
        catch {
            b = null;
        }
        if (!b)
            continue;

        const x = Number(b.x || 0);
        const y = Number(b.y || 0);
        const w = Number(b.width || 0);
        const h = Number(b.height || 0);
        if (!(w > 1 && h > 1))
            continue;

        const offRight = x > viewportW + 12;
        const offBottom = y > viewportH + 12;
        const offLeft = (x + w) < -12;
        const offTop = (y + h) < -12;
        if (offRight || offBottom || offLeft || offTop)
            continue;

        const label = String(n.label || n.cursor || n.constructor?.name || 'node');
        const ll = label.toLowerCase();
        if (ll.includes('__background') || ll.includes('__contentroot') || ll.includes('__overlayroot') || ll.includes('__children')) {
            continue;
        }
        let isText = false;
        let text = '';
        let fontSize = 12;
        let color = 0x202020;
        if (typeof n.text === 'string' && n.text.length > 0) {
            isText = true;
            text = String(n.text);
            const fs = Number(n.style?.fontSize ?? n._style?.fontSize ?? 12);
            if (Number.isFinite(fs) && fs > 0) fontSize = fs;
            const fill = n.style?.fill ?? n._style?.fill;
            if (typeof fill === 'number') {
                color = Number(fill) >>> 0;
            }
        }
        out.push({ x, y, w, h, depth, alpha, label, isText, text, fontSize, color });
    }

    return out;
}

function buildDirectBackendLayoutFromItems(items, viewportW, viewportH) {
    const outChildren = [];
    if (Array.isArray(items)) {
        for (let i = 0; i < items.length; i++) {
            const it = items[i] || {};
            if (it.isText)
                continue;
            const w = Number(it.w || 0);
            const h = Number(it.h || 0);
            if (!(w > 1 && h > 1))
                continue;
            outChildren.push({
                kind: 'block',
                key: `direct:${i}`,
                tagName: String(it.label || 'block'),
                x: Number(it.x || 0),
                y: Number(it.y || 0),
                width: w,
                height: h,
                children: [],
            });
        }
    }
    return {
        kind: 'block',
        tagName: 'root',
        x: 0,
        y: 0,
        width: Math.max(1, Number(viewportW || 1) | 0),
        height: Math.max(1, Number(viewportH || 1) | 0),
        children: outChildren,
    };
}

function summarizeLayoutAbs(root, maxSamples = 8) {
    let total = 0;
    let sized = 0;
    let nearOrigin = 0;
    let minX = Infinity;
    let minY = Infinity;
    let maxX = -Infinity;
    let maxY = -Infinity;
    const samples = [];
    const zeroSamples = [];

    const walk = (n, ax = 0, ay = 0, depth = 0) => {
        if (!n || typeof n !== 'object')
            return;
        const rx = Number(n.x || 0);
        const ry = Number(n.y || 0);
        const x = ax + rx;
        const y = ay + ry;
        const w = Number(n.width || 0);
        const h = Number(n.height || 0);
        if (n.kind === 'block') {
            total++;
            if (w > 1 && h > 1) {
                sized++;
                if (Math.abs(x) < 4 && Math.abs(y) < 4)
                    nearOrigin++;
                minX = Math.min(minX, x);
                minY = Math.min(minY, y);
                maxX = Math.max(maxX, x + w);
                maxY = Math.max(maxY, y + h);
                if (samples.length < maxSamples) {
                    samples.push(`${String(n.tagName || 'block')}@${x.toFixed(1)},${y.toFixed(1)} ${w.toFixed(1)}x${h.toFixed(1)} d=${depth}`);
                }
            }
            else if (zeroSamples.length < maxSamples) {
                zeroSamples.push(`${String(n.tagName || 'block')}@${x.toFixed(1)},${y.toFixed(1)} ${w.toFixed(1)}x${h.toFixed(1)} d=${depth}`);
            }
        }
        const kids = Array.isArray(n.children) ? n.children : [];
        for (let i = 0; i < kids.length; i++) {
            walk(kids[i], x, y, depth + 1);
        }
    };

    const children = Array.isArray(root?.children) ? root.children : [];
    for (let i = 0; i < children.length; i++) {
        walk(children[i], Number(root?.x || 0), Number(root?.y || 0), 0);
    }

    return {
        total,
        sized,
        nearOrigin,
        minX: Number.isFinite(minX) ? minX : 0,
        minY: Number.isFinite(minY) ? minY : 0,
        maxX: Number.isFinite(maxX) ? maxX : 0,
        maxY: Number.isFinite(maxY) ? maxY : 0,
        samples,
        zeroSamples,
    };
}

function summarizeItems(items, maxSamples = 8) {
    let total = 0;
    let nearOrigin = 0;
    let minX = Infinity;
    let minY = Infinity;
    let maxX = -Infinity;
    let maxY = -Infinity;
    const samples = [];
    const list = Array.isArray(items) ? items : [];
    for (let i = 0; i < list.length; i++) {
        const it = list[i] || {};
        if (it.isText)
            continue;
        const x = Number(it.x || 0);
        const y = Number(it.y || 0);
        const w = Number(it.w || 0);
        const h = Number(it.h || 0);
        if (!(w > 1 && h > 1))
            continue;
        total++;
        if (Math.abs(x) < 4 && Math.abs(y) < 4)
            nearOrigin++;
        minX = Math.min(minX, x);
        minY = Math.min(minY, y);
        maxX = Math.max(maxX, x + w);
        maxY = Math.max(maxY, y + h);
        if (samples.length < maxSamples) {
            samples.push(`${String(it.label || 'item')}@${x.toFixed(1)},${y.toFixed(1)} ${w.toFixed(1)}x${h.toFixed(1)} d=${Number(it.depth || 0) | 0}`);
        }
    }
    return {
        total,
        nearOrigin,
        minX: Number.isFinite(minX) ? minX : 0,
        minY: Number.isFinite(minY) ? minY : 0,
        maxX: Number.isFinite(maxX) ? maxX : 0,
        maxY: Number.isFinite(maxY) ? maxY : 0,
        samples,
    };
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

function collectHtmlElementTagCounts(node, out = new Map()) {
    if (!node || typeof node !== 'object')
        return out;
    if (isElement(node)) {
        const tag = String(node.tagName ?? node.nodeName ?? '').toLowerCase();
        if (tag.length > 0) {
            out.set(tag, (out.get(tag) ?? 0) + 1);
        }
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

function createTextMeasurer(font) {
    const canvas = RT_DOCUMENT?.createElement?.('canvas');
    if (!canvas)
        throw new Error('canvas element not available');
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
    const yogaTraceEnabled = TRACE_POSITION_FLOW && TRACE_YOGA_LIFECYCLE;
    const yogaNodeMeta = new WeakMap();
    const yogaTrace = yogaTraceEnabled
        ? {
            created: 0,
            rootInserts: 0,
            nestedInserts: 0,
            insertErrors: 0,
            countAnomalies: 0,
            samples: [],
            maxSamples: 48,
        }
        : null;
    const pushYogaTraceSample = (s) => {
        if (!yogaTrace || yogaTrace.samples.length >= yogaTrace.maxSamples)
            return;
        yogaTrace.samples.push(String(s));
    };
    const safeYogaCount = (n) => {
        if (!n || typeof n.getChildCount !== 'function')
            return -1;
        try {
            return Number(n.getChildCount() || 0) | 0;
        }
        catch {
            return -1;
        }
    };
    const getYogaMeta = (n) => {
        if (!n)
            return { kind: 'none', key: 'none', depth: -1 };
        return yogaNodeMeta.get(n) ?? { kind: 'unknown', key: 'unknown', depth: -1 };
    };
    const registerYogaMeta = (n, renderNode, depth) => {
        if (!n)
            return;
        const isTextNode = renderNode?.kind === 'text';
        const kind = isTextNode ? 'text' : 'block';
        const key = isTextNode
            ? String(renderNode?.text ?? '').slice(0, 28)
            : String(renderNode?.key ?? renderNode?.tagName ?? 'block');
        yogaNodeMeta.set(n, { kind, key, depth: Number(depth || 0) | 0 });
        if (yogaTrace)
            yogaTrace.created++;
    };
    const traceInsert = (parentNode, childNode, slot, scope) => {
        const before = safeYogaCount(parentNode);
        const parentMeta = getYogaMeta(parentNode);
        const childMeta = getYogaMeta(childNode);
        try {
            parentNode.insertChild(childNode, slot);
        }
        catch (err) {
            if (yogaTrace)
                yogaTrace.insertErrors++;
            pushYogaTraceSample(`insert-error scope=${scope} parent=${parentMeta.key} child=${childMeta.key} slot=${slot} before=${before} err=${String(err)}`);
            throw err;
        }
        const after = safeYogaCount(parentNode);
        let slotChild = null;
        let slotChildMeta = null;
        if (parentNode && typeof parentNode.getChild === 'function' && slot >= 0) {
            try {
                slotChild = parentNode.getChild(slot);
            }
            catch {
                slotChild = null;
            }
            slotChildMeta = getYogaMeta(slotChild);
        }
        if (yogaTrace) {
            if (scope === 'root')
                yogaTrace.rootInserts++;
            else
                yogaTrace.nestedInserts++;
        }
        if (before >= 0 && after >= 0 && after !== before + 1) {
            if (yogaTrace)
                yogaTrace.countAnomalies++;
            pushYogaTraceSample(`count-anomaly scope=${scope} parent=${parentMeta.key} child=${childMeta.key} before=${before} after=${after} slot=${slot} slotChild=${slotChildMeta?.key ?? 'none'} pDepth=${parentMeta.depth} cDepth=${childMeta.depth}`);
        }
        else if (!slotChild) {
            pushYogaTraceSample(`slot-empty scope=${scope} parent=${parentMeta.key} child=${childMeta.key} slot=${slot} before=${before} after=${after}`);
        }
        else if (slotChild !== childNode) {
            pushYogaTraceSample(`slot-mismatch scope=${scope} parent=${parentMeta.key} child=${childMeta.key} slot=${slot} got=${slotChildMeta?.key ?? 'unknown'} before=${before} after=${after}`);
        }
        else if (scope === 'root' || childMeta.depth <= 2) {
            pushYogaTraceSample(`insert-ok scope=${scope} parent=${parentMeta.key} child=${childMeta.key} before=${before} after=${after} slot=${slot}`);
        }
    };
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
    function yogaForNode(node, depth = 0) {
        if (node.kind === 'text') {
            const yogaNode = Yoga.Node.create();
            registerYogaMeta(yogaNode, node, depth);
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
            const out = createYogaNodeForSliderLabel({ node, Yoga, measurer });
            registerYogaMeta(out?.yogaNode, node, depth);
            return out;
        }
        const yogaNode = Yoga.Node.create();
        registerYogaMeta(yogaNode, node, depth);
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
        if (node.tagName === 'iframe' && String(node.attrs?.['data-root'] ?? '') === '1') {
            // Top-level document is modeled as a synthetic iframe; give it a concrete
            // viewport-sized box so downstream direct backends see real dimensions.
            const rw = Math.max(1, viewportWidth - (16 + 16 + SCROLLBAR_PAD));
            const rh = Math.max(1, viewportHeight - (16 + 16));
            yogaNode.setWidth(rw);
            yogaNode.setHeight(rh);
            yogaNode.setMinWidth(rw);
            yogaNode.setMinHeight(rh);
            yogaNode.setFlexGrow(1);
            yogaNode.setFlexShrink(0);
            yogaNode.setAlignSelf(Yoga.ALIGN_STRETCH);
        }
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
        const childPairs = effectiveChildren.map((c) => yogaForNode(c, depth + 1));
        for (let i = 0; i < childPairs.length; i++) {
            const childRender = effectiveChildren[i];
            const childPair = childPairs[i];
            if (childRender && childRender.kind === 'block') {
                const m = i === childPairs.length - 1 ? 0 : gapAfter(childRender);
                childPair.yogaNode.setMargin(Yoga.EDGE_BOTTOM, m);
            }
            const slot = safeYogaCount(yogaNode);
            traceInsert(yogaNode, childPair.yogaNode, slot >= 0 ? slot : 0, 'nested');
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
    registerYogaMeta(rootYoga, { kind: 'block', key: 'root', tagName: 'root' }, 0);
    rootYoga.setFlexDirection(Yoga.FLEX_DIRECTION_COLUMN);
    rootYoga.setAlignItems(Yoga.ALIGN_STRETCH);
    rootYoga.setWidth(viewportWidth);
    rootYoga.setHeight(viewportHeight);
    rootYoga.setPadding(Yoga.EDGE_LEFT, 16);
    rootYoga.setPadding(Yoga.EDGE_TOP, 16);
    // Reserve an extra gutter so content doesn't touch the global scrollbar.
    rootYoga.setPadding(Yoga.EDGE_RIGHT, 16 + SCROLLBAR_PAD);
    rootYoga.setPadding(Yoga.EDGE_BOTTOM, 16);
    const pairs = renderNodes.map((n) => yogaForNode(n, 1));
    for (let i = 0; i < pairs.length; i++) {
        const renderNode = renderNodes[i];
        const pair = pairs[i];
        if (renderNode && renderNode.kind === 'block') {
            const m = i === pairs.length - 1 ? 0 : gapAfter(renderNode);
            pair.yogaNode.setMargin(Yoga.EDGE_BOTTOM, m);
        }
        const slot = safeYogaCount(rootYoga);
        traceInsert(rootYoga, pair.yogaNode, slot >= 0 ? slot : 0, 'root');
    }
    const rootCountPreLayout = safeYogaCount(rootYoga);
    pushYogaTraceSample(`pre-layout rootChildren=${rootCountPreLayout} pairs=${pairs.length} renderNodes=${renderNodes.length}`);
    rootYoga.calculateLayout(viewportWidth, viewportHeight, Yoga.DIRECTION_LTR);
    const rootCountPostLayout = safeYogaCount(rootYoga);
    pushYogaTraceSample(`post-layout rootChildren=${rootCountPostLayout}`);
    if (TRACE_POSITION_FLOW) {
        try {
            const c0 = rootYoga.getChildCount() > 0 ? rootYoga.getChild(0) : null;
            const rw = Number(rootYoga.getComputedWidth() || 0);
            const rh = Number(rootYoga.getComputedHeight() || 0);
            const c0x = c0 ? Number(c0.getComputedLeft() || 0) : 0;
            const c0y = c0 ? Number(c0.getComputedTop() || 0) : 0;
            const c0w = c0 ? Number(c0.getComputedWidth() || 0) : 0;
            const c0h = c0 ? Number(c0.getComputedHeight() || 0) : 0;
            console.log(`[pos-trace:build] vp=${viewportWidth}x${viewportHeight} root=${rw}x${rh} c0=${c0x},${c0y} ${c0w}x${c0h} children=${rootYoga.getChildCount()}`);
            if (yogaTrace) {
                console.log(`[pos-trace:yoga-summary] created=${yogaTrace.created} rootInserts=${yogaTrace.rootInserts} nestedInserts=${yogaTrace.nestedInserts} insertErrors=${yogaTrace.insertErrors} anomalies=${yogaTrace.countAnomalies} pre=${rootCountPreLayout} post=${rootCountPostLayout}`);
                if (yogaTrace.samples.length > 0)
                    console.log(`[pos-trace:yoga-samples] ${yogaTrace.samples.join(' | ')}`);
            }
        }
        catch {
            // Keep trace non-fatal.
        }
    }
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
    const freeType = typeof rootYoga.freeRecursive;
    if (yogaTraceEnabled)
        pushYogaTraceSample(`free type=${freeType}`);
    rootYoga.freeRecursive?.();
    return box;
}
function renderToPixi(app, box, sceneRoot, opts = {}) {
    const fullRepaint = opts?.fullRepaint !== false;
    const dirtyKeys = opts?.dirtyKeys instanceof Set ? opts.dirtyKeys : null;
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
    // Overlays are built immediate-mode; only clear on full repaint.
    if (fullRepaint) {
        overlayRoot.removeChildren();
    }
    const selectPopups = [];
    const temporalPopups = [];
    const radioGroups = collectRadioGroups(box);
    if (fullRepaint) {
        uiState.fieldBounds.clear();
        uiState.sliderBounds.clear();
        uiState.dialogDragBounds.clear();
        uiState.hoverRects.length = 0;
        uiState.hoverHandlers.clear();
        uiState.iframeRects.length = 0;
    }
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
    const directSnapshot = [];
    const pushDirectSnapshot = (item) => {
        if (!fullRepaint)
            return;
        if (!item)
            return;
        if (directSnapshot.length >= 1200)
            return;
        directSnapshot.push(item);
    };
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
    const dirtySubtree = new WeakMap();
    const hasKey = (k) => typeof k === 'string' && k.length > 0;
    const computeDirtySubtree = (node) => {
        if (fullRepaint)
            return true;
        const own = hasKey(node?.key) && dirtyKeys?.has(node.key);
        let child = false;
        const kids = Array.isArray(node?.children) ? node.children : [];
        for (let i = 0; i < kids.length; i++) {
            if (computeDirtySubtree(kids[i]))
                child = true;
        }
        const d = !!(own || child);
        dirtySubtree.set(node, d);
        return d;
    };
    computeDirtySubtree(box);
    const purgePerKeyCaches = (key) => {
        if (!hasKey(key))
            return;
        uiState.fieldBounds.delete(key);
        uiState.sliderBounds.delete(key);
        uiState.dialogDragBounds.delete(key);
        uiState.hoverHandlers.delete(key);
        uiState.hoverRects = uiState.hoverRects.filter((r) => r?.key !== key);
        uiState.iframeRects = uiState.iframeRects.filter((r) => r?.key !== key);
    };
    const requestPaintForNode = (nodeKey) => () => {
        if (hasKey(nodeKey)) {
            requestPaint?.(nodeKey);
            return;
        }
        // Keyless hover/active updates should never force a full scene rebuild.
        requestPresent?.();
    };

    function drawNode(node, parent, textCtx, absX = 0, absY = 0, dialogSink, dialogClampRect, path, orderIndex) {
        if (!fullRepaint && dirtySubtree.get(node) !== true) {
            return;
        }
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
        const nodeOwnDirty = fullRepaint || (hasKey(node.key) && dirtyKeys?.has(node.key));
        if (!fullRepaint && nodeOwnDirty && hasKey(node.key)) {
            purgePerKeyCaches(node.key);
        }
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
            if (nodeOwnDirty) {
                clearGraphics(g);
            }
            // Make sure the base graphics stays behind everything else.
            ensureChildAtAny(container, g, 0);
            g.zIndex = -10;
            let w = Math.max(0, node.width);
            let h = Math.max(0, node.height);
            if (w > 1 && h > 1) {
                pushDirectSnapshot({
                    x: nodeAbsX,
                    y: nodeAbsY,
                    w,
                    h,
                    depth: Math.max(0, Number(path.split('.').length - 1) | 0),
                    alpha: 1,
                    label: String(node.tagName || 'block'),
                    isText: false,
                });
            }
            let overlayLabel = null;
            // Headings: snap to whole pixels so the 1px border doesn't land on half pixels
            // (which can look like a faint extra 1px row outside the top edge).
            if (node.tagName === 'h1' || node.tagName === 'h2' || node.tagName === 'h3') {
                container.position.set(Math.round(node.x), Math.round(node.y));
                w = Math.round(w);
                h = Math.round(h);
            }
            if (node.tagName === 'hr') {
                if (nodeOwnDirty) {
                    renderHr({ graphics: g, w, theme });
                }
            }
            else if (node.tagName === 'barrow') {
                // Layout-only wrapper for [label][bar]. No visuals.
            }
            else if (node.tagName === 'searchrow') {
                // Layout-only wrapper for [search icon button][input]. No visuals.
            }
            else if (node.tagName === 'searchbutton') {
                if (nodeOwnDirty) {
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
                    requestPaint: requestPaintForNode(node.key),
                    });
                }
            }
            else if (node.tagName === 'progress' || node.tagName === 'meter') {
                if (nodeOwnDirty) {
                    renderProgressOrMeter({ node, graphics: g, w, h, theme });
                }
            }
            else if (node.tagName === 'sliderlabel') {
                if (nodeOwnDirty) {
                    renderSliderLabel({
                    node,
                    container,
                    theme,
                    sliderStates: uiState.sliders,
                    });
                }
            }
            else if (node.tagName === 'slider') {
                if (nodeOwnDirty) {
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
                    requestPaint: requestPaintForNode(node.key),
                    getPointerId: getEffectivePointerId,
                    });
                }
            }
            else if (node.tagName === 'timeinput' ||
                node.tagName === 'dateinput' ||
                node.tagName === 'monthinput' ||
                node.tagName === 'weekinput' ||
                node.tagName === 'datetimelocalinput') {
                if (nodeOwnDirty) {
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
                    requestPaint: requestPaintForNode(node.key),
                    popupSink: temporalPopups,
                    });
                }
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
                if (nodeOwnDirty) {
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
                    requestPaint: requestPaintForNode(node.key),
                    showCaret,
                    caretPointerId,
                    focusColor: focusColor ?? undefined,
                    getCursorColor,
                    getPointerId: getEffectivePointerId,
                    });
                }
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
                if (nodeOwnDirty) {
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
                    requestPaint: requestPaintForNode(node.key),
                    showCaret,
                    caretPointerId,
                    focusColor: focusColor ?? undefined,
                    getCursorColor,
                    getPointerId: getEffectivePointerId,
                    });
                }
            }
            else if (node.tagName === 'select') {
                // Ensure state exists so it persists across rerenders.
                if (node.key) {
                    const initIdx = Number(node.attrs?.['data-selected-index'] ?? '0');
                    getOrInitSelectState(uiState.selects, node.key, Number.isFinite(initIdx) ? initIdx : 0);
                }
                if (nodeOwnDirty) {
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
                    requestPaint: requestPaintForNode(node.key),
                    popupSink: selectPopups,
                    });
                }
            }
            else if (node.tagName === 'summary') {
                if (node.key) {
                    uiState.hoverRects.push({ key: node.key, kind: 'summary', cursor: 'pointer', x: nodeAbsX, y: nodeAbsY, w, h });
                }
                if (nodeOwnDirty) {
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
            }
            else if (node.tagName === 'dialog') {
                if (nodeOwnDirty) {
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
                    requestPaint: requestPaintForNode(node.key),
                    getPointerId: getEffectivePointerId,
                    });
                }
            }
            else if (node.tagName === 'img') {
                if (nodeOwnDirty) {
                    renderImg({ node, container, graphics: g, w, h, theme, requestRerender });
                }
            }
            else if (node.tagName === 'svg') {
                const svgMarkup = node.attrs?.['data-svg'] ?? '';
                // Reuse the same Graphics container; svg rendering adds its own Graphics.
                if (nodeOwnDirty) {
                    renderSvgElement({ svgMarkup, container, w, h, requestRerender });
                }
            }
            else if (node.tagName === 'canvas') {
                if (nodeOwnDirty) {
                    renderCanvasElement({ node, container, graphics: g, w, h, theme });
                }
            }
            else if (node.tagName === 'iframe') {
                if (nodeOwnDirty) {
                    renderIframePlaceholder({ node, container, graphics: g, w, h, theme });
                }
            }
            else if (node.tagName === 'color') {
                uiState.color.bounds = { x: nodeAbsX, y: nodeAbsY, w: Math.max(0, w), h: Math.max(0, h) };
                if (nodeOwnDirty) {
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
                    requestPaint: requestPaintForNode(node.key),
                    getPointerId: getEffectivePointerId,
                    setDraggingPointerId: (pid) => {
                        uiState.color.draggingPointerId = pid;
                    },
                    });
                }
            }
            else if (node.tagName === 'number') {
                const key = node.key;
                const ch = String(node.attrs?.channel ?? '').toLowerCase();
                const isCh = ch === 'r' || ch === 'g' || ch === 'b' || ch === 'a';
                if (key) {
                    if (nodeOwnDirty) {
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
                        requestPaint: requestPaintForNode(node.key),
                        numberHolds: uiState.numberHolds,
                        getPointerId: getEffectivePointerId,
                        });
                    }
                }
            }
            else if (node.tagName === 'button') {
                if (node.key) {
                    uiState.hoverRects.push({ key: node.key, kind: 'button', cursor: 'pointer', x: nodeAbsX, y: nodeAbsY, w, h });
                }
                if (nodeOwnDirty) {
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
            }
            else if (isHeadingTag(node.tagName)) {
                // Headings should not get the generic 1px element border.
            }
            else if (node.tagName === 'table') {
                if (nodeOwnDirty) {
                    renderTable({ graphics: g, w, h, boxBorder: theme.boxBorder });
                }
            }
            else if (node.tagName === 'td' || node.tagName === 'th') {
                if (nodeOwnDirty) {
                    renderCell({ nodeTag: node.tagName, graphics: g, w, h, theme });
                }
            }
            else {
                if (nodeOwnDirty) {
                    // Default block border: draw fully inside the box so it doesn't "bleed"
                    // into the outside margin area (which looks like an extra 1px row above).
                    const bw = Math.max(0, Math.round(w));
                    const bh = Math.max(0, Math.round(h));
                    g.rect(0, 0, bw, bh);
                    g.stroke({ width: 1, color: theme.boxBorder, alignment: 0 });
                }
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
                        key: iframeKey,
                        y: 0,
                        contentHeight: 0,
                        viewportHeight: 0,
                        draggingPointerId: null,
                        dragOffsetY: 0,
                        track: { x: 0, y: 0, w: SCROLLBAR_W, h: 0 },
                        thumb: { x: 0, y: 0, w: SCROLLBAR_W, h: 0 },
                        rect: { x: nodeAbsX, y: nodeAbsY, w: Math.max(0, w), h: Math.max(0, h) },
                    };
                st.key = iframeKey;
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
                    // Keep direct-cmd output in sync with existing iframe scrollbar visuals.
                    pushDirectSnapshot({
                        x: st.track.x,
                        y: st.track.y,
                        w: st.track.w,
                        h: st.track.h,
                        depth: Math.max(0, Number(path.split('.').length - 1) | 0),
                        alpha: 0.06,
                        label: 'scrollbar-track',
                    });
                    pushDirectSnapshot({
                        x: st.thumb.x,
                        y: st.thumb.y,
                        w: st.thumb.w,
                        h: st.thumb.h,
                        depth: Math.max(0, Number(path.split('.').length - 1) | 0),
                        alpha: 0.25,
                        label: 'scrollbar-thumb',
                    });
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
                        requestPaint?.(iframeKey || null);
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
            if (nodeOwnDirty) {
                t.text = node.text ?? '';
                t.style.fontFamily = theme.fontFamily;
                t.style.fontSize = theme.fontSize;
                t.style.fill = theme.text;
                t.style.fontWeight = textCtx.bold ? '700' : '400';
                t.style.wordWrap = true;
                t.style.wordWrapWidth = Math.max(0, Math.ceil(node.width) + WRAP_EPSILON_PX);
                t.position.set(0, TEXT_BASELINE_NUDGE_Y);
            }
            const tw = Math.max(1, Number(t.width || node.width || 0));
            const th = Math.max(1, Number(t.height || node.height || theme.fontSize || 12));
            pushDirectSnapshot({
                x: nodeAbsX,
                y: nodeAbsY + TEXT_BASELINE_NUDGE_Y,
                w: tw,
                h: th,
                depth: Math.max(0, Number(path.split('.').length - 1) | 0),
                alpha: 1,
                label: 'text',
                isText: true,
                text: String(node.text ?? ''),
                fontSize: Number(theme.fontSize || 12),
                color: Number(theme.text || 0x202020) >>> 0,
            });
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
    // Context menu overlay (authoritative state comes from browser_context).
    const menuCursorIds = [1, 2, 3, 4];
    for (const ownerPid of menuCursorIds) {
        let isOpen = false;
        let menuX = 0;
        let menuY = 0;
        try {
            isOpen = !!browserContext.isContextMenuOpen(ownerPid);
            if (isOpen) {
                menuX = Number(browserContext.getContextMenuX(ownerPid) ?? 0) || 0;
                menuY = Number(browserContext.getContextMenuY(ownerPid) ?? 0) || 0;
            }
        }
        catch {
            isOpen = false;
        }
        if (!isOpen)
            continue;
        const menu = new Container();
        menu.eventMode = 'static';
        menu.cursor = 'default';
        menu.position.set(menuX, menuY);
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
                let focusedKey = uiState.focusedKeyByPointer.get(ownerPid) ?? null;
                try {
                    const k = browserContext.getFocusedTarget(ownerPid);
                    if (typeof k === 'string' && k.length > 0)
                        focusedKey = k;
                }
                catch {
                    // Fallback to local focus state.
                }
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
                    try {
                        browserContext.setClipboardText(ownerPid, picked);
                    }
                    catch {
                        // Keep menu interaction resilient when browser_context is unavailable.
                    }
                }
                else if (label === 'Paste' && isTextField) {
                    let clip = '';
                    try {
                        const c = browserContext.getClipboardText(ownerPid);
                        if (typeof c === 'string')
                            clip = c;
                    }
                    catch {
                        // Keep menu interaction resilient when browser_context is unavailable.
                    }
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
                try {
                    browserContext.closeContextMenu(ownerPid);
                }
                catch {
                    // Fallback: leave legacy menu state untouched.
                }
                requestPaint?.();
            });
            menu.addChild(hit);
        });
        overlayRoot.addChild(menu);
    }
    if (fullRepaint) {
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
    }
    // Retained-mode renderer: we keep a stable scene graph rooted at `stage`.
    // Do not clear or re-add `stage` (it may be `sceneRoot` itself).
    globalThis.__trueosDirectSnapshot = directSnapshot;
    return directSnapshot;
}
export async function startGui() {
    const rootEl = RT_DOCUMENT?.getElementById?.('app') ?? RT_DOCUMENT?.body;
    const app = new Application();
    const initOpts = { background: '#ffffff', antialias: false, preference: 'webgpu' };
    if (RT_HAS_WINDOW_RESIZE)
        initOpts.resizeTo = RT_WINDOW;
    await app.init(initOpts);
    rootEl?.appendChild?.(app.canvas);

    const startCursorPlaneTick = () => {
        if (!(USE_DIRECT_CMD_BACKEND && USE_CURSOR_PLANE_TICK))
            return;
        if (typeof globalThis.__trueosCursorPlaneTimer !== 'number') {
            globalThis.__trueosCursorPlaneTimer = setInterval(() => {
                try {
                    renderCursorPlaneFrame({
                        viewportW: app.renderer.width,
                        viewportH: app.renderer.height,
                        browserContext,
                        getCursorColor,
                    });
                }
                catch {
                    // Keep UI path resilient if cursor-plane tick errors.
                }
            }, CURSOR_PLANE_TICK_MS);
        }
    };
    // We render on-demand (after state/layout changes) rather than continuously.
    // This saves substantial GPU time when the UI is static.
    app.ticker.stop();
    // Disable the browser context menu over the canvas.
    app.canvas.addEventListener('contextmenu', (e) => e.preventDefault());
    // Wheel scroll: scene-level global scrollbar.
    app.canvas.addEventListener('wheel', (e) => {
        const x = e.offsetX ?? 0;
        const y = e.offsetY ?? 0;
        let handled = false;
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
                    requestPaint?.(iframeKey || null);
                    e.preventDefault();
                    handled = true;
                }
            }
        }
        if (handled)
            return;
        const maxScroll = Math.max(0, uiState.scroll.contentHeight - uiState.scroll.viewportHeight);
        if (maxScroll <= 0)
            return;
        uiState.scroll.y = Math.max(0, Math.min(maxScroll, uiState.scroll.y + e.deltaY));
        requestPaint?.(GLOBAL_SCROLL_DIRTY_KEY);
        e.preventDefault();
    }, { passive: false });
    // Make sure the stage participates in hit testing.
    app.stage.eventMode = 'static';
    app.stage.hitArea = app.screen;
    // Global context menu + "outside click" behavior.
    // This must be registered once (retained scene); widget handlers can stopPropagation.
    app.stage.on('pointerdown', (ev) => {
        const pid = getEffectivePointerId(ev);
        const menuPid = getMenuOwnerPointerId(ev, pid);
        const gx = ev.global?.x ?? 0;
        const gy = ev.global?.y ?? 0;
        logCursorButtonEvent('down', pid, Number(ev?.button ?? 0) | 0, gx, gy);
        if (ev?.button === 2) {
            if (menuPid > 0) {
                try {
                    browserContext.openContextMenu(menuPid, gx, gy, null);
                }
                catch {
                    // Keep input path resilient when browser_context is unavailable.
                }
            }
            requestPaint?.();
            ev.preventDefault?.();
            return;
        }
        if (pid > 0) {
            try {
                browserContext.routePointerDown(pid, gx, gy, null, Number(ev?.button ?? 0) | 0);
            }
            catch {
                // Keep legacy local input path alive.
            }
        }
        // Left click closes only THIS pointer's menu (clicks from other pointers don't dismiss it).
        if (ev?.button !== 2) {
            if (menuPid > 0) {
                try {
                    if (browserContext.isContextMenuOpen(menuPid)) {
                        browserContext.closeContextMenu(menuPid);
                        requestPaint?.();
                    }
                }
                catch {
                    // Keep input path resilient when browser_context is unavailable.
                }
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
    const dragMeasureCanvas = RT_DOCUMENT?.createElement?.('canvas');
    if (!dragMeasureCanvas)
        throw new Error('canvas element not available');
    const dragMeasureCtx = dragMeasureCanvas.getContext('2d');
    if (!dragMeasureCtx)
        throw new Error('2D canvas not available');
    dragMeasureCtx.font = `${defaultTheme.fontSize}px ${defaultTheme.fontFamily}`;
    const dragMeasure = (s) => dragMeasureCtx.measureText(s).width;
    const dragLineHeight = defaultTheme.fontSize * 1.25;
    // Keep startup deterministic: always use the embedded HTML template.
    let html = INPUT_HTML;
    try {
        console.log(`[richui-html] len=${html.length} marker=${html.includes('Tree (Details + Checkbox)') ? 'rich' : 'small'}`);
    }
    catch {
        // Debug logging should never affect startup.
    }
    const doc = parse5.parse(html);
    const body = getBody(doc) ?? doc;
    const innerRenderNodes = toRenderTree(body, '0');
    logRichHtmlCoverage(body, innerRenderNodes);

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
    let posTraceFrame = 0;
    const dirtyWidgetKeys = new Set();
    let forceFullRepaint = true;
    const paint = () => {
        if (!lastLayout)
            return;
        clampScroll();
        const incrementalWidgetPaint = USE_DIRECT_CMD_BACKEND && !forceFullRepaint && dirtyWidgetKeys.size > 0;
        const directSnapshot = renderToPixi(app, lastLayout, sceneRoot, {
            fullRepaint: !incrementalWidgetPaint,
            dirtyKeys: dirtyWidgetKeys,
        });
        updateScrollbarVisuals();
        if (incrementalWidgetPaint) {
            app.renderer.render(app.stage);
            dirtyWidgetKeys.clear();
            forceFullRepaint = false;
            return;
        }
        if (Array.isArray(directSnapshot) && uiState.scroll.track.h > 0 && uiState.scroll.thumb.h > 0) {
            // Mirror existing global scrollbar visuals into direct-cmd snapshot stream.
            directSnapshot.push({
                x: uiState.scroll.track.x,
                y: uiState.scroll.track.y,
                w: uiState.scroll.track.w,
                h: uiState.scroll.track.h,
                depth: 0,
                alpha: 0.06,
                label: 'scrollbar-track',
            });
            directSnapshot.push({
                x: uiState.scroll.thumb.x,
                y: uiState.scroll.thumb.y,
                w: uiState.scroll.thumb.w,
                h: uiState.scroll.thumb.h,
                depth: 0,
                alpha: 0.25,
                label: 'scrollbar-thumb',
            });
        }
        try {
            console.log(`[richui-paint] sceneChildren=${sceneRoot.children.length} overlayChildren=${overlayRoot.children.length} hoverRects=${uiState.hoverRects.length} layoutNodes=${countLayoutNodes(lastLayout)}`);
        }
        catch {
            // Debug logging should never affect paint.
        }
        // Manual render (ticker is stopped).
        if (USE_DIRECT_CMD_BACKEND) {
            const hasDirectBlocks = Array.isArray(directSnapshot)
                && directSnapshot.some((it) => !it?.isText && Number(it?.w || 0) > 2 && Number(it?.h || 0) > 2);
            const usingYogaSnapshot = !!hasDirectBlocks;
            const pixiItems = usingYogaSnapshot
                ? directSnapshot
                : collectPixiSnapshotItems([sceneRoot, overlayUiRoot, overlayRoot], app.renderer.width, app.renderer.height);
            const backendLayout = buildDirectBackendLayoutFromItems(pixiItems, app.renderer.width, app.renderer.height);
            posTraceFrame++;
            if (TRACE_POSITION_FLOW) {
                const yoga = summarizeLayoutAbs(lastLayout);
                const snap = summarizeItems(pixiItems);
                const backend = summarizeLayoutAbs(backendLayout);
                const noisy = posTraceFrame <= 3 || (posTraceFrame % 30) === 1;
                const clustered = (snap.total > 0 && (snap.nearOrigin / snap.total) > 0.6)
                    || (backend.sized > 0 && (backend.nearOrigin / backend.sized) > 0.6);
                if (noisy || clustered) {
                    console.log(`[pos-trace] frame=${posTraceFrame} src=${usingYogaSnapshot ? 'yoga-snapshot' : 'pixi-bounds'} yogaSized=${yoga.sized}/${yoga.total} yogaNear0=${yoga.nearOrigin} yogaBBox=${yoga.minX.toFixed(1)},${yoga.minY.toFixed(1)}..${yoga.maxX.toFixed(1)},${yoga.maxY.toFixed(1)} snap=${snap.total} snapNear0=${snap.nearOrigin} snapBBox=${snap.minX.toFixed(1)},${snap.minY.toFixed(1)}..${snap.maxX.toFixed(1)},${snap.maxY.toFixed(1)} backendSized=${backend.sized}/${backend.total} backendNear0=${backend.nearOrigin} backendBBox=${backend.minX.toFixed(1)},${backend.minY.toFixed(1)}..${backend.maxX.toFixed(1)},${backend.maxY.toFixed(1)}`);
                    if (yoga.samples.length > 0)
                        console.log(`[pos-trace:yoga-samples] ${yoga.samples.join(' | ')}`);
                    if (yoga.zeroSamples.length > 0)
                        console.log(`[pos-trace:yoga-zero] ${yoga.zeroSamples.join(' | ')}`);
                    if (snap.samples.length > 0)
                        console.log(`[pos-trace:snapshot-samples] ${snap.samples.join(' | ')}`);
                    if (backend.samples.length > 0)
                        console.log(`[pos-trace:backend-samples] ${backend.samples.join(' | ')}`);
                }
            }
            const rendered = renderDirectCmdFrame({
                layout: backendLayout,
                pixiItems,
                allowFit: !usingYogaSnapshot,
                viewportW: app.renderer.width,
                viewportH: app.renderer.height,
                worldW: app.screen.width,
                worldH: app.screen.height,
                scrollY: uiState.scroll.y,
                clearRgb: 0xFFFFFF,
                browserContext,
                getCursorColor,
            });
            if (!rendered) {
                app.renderer.render(app.stage);
            }
        }
        else {
            app.renderer.render(app.stage);
        }
        dirtyWidgetKeys.clear();
        forceFullRepaint = false;
    };

    const getLayoutViewport = () => {
        const ww = Number(RT_WINDOW?.innerWidth ?? 0) || 0;
        const wh = Number(RT_WINDOW?.innerHeight ?? 0) || 0;
        const rw = Number(app?.renderer?.width ?? 0) || 0;
        const rh = Number(app?.renderer?.height ?? 0) || 0;
        return {
            w: Math.max(1, ww > 0 ? ww : rw, 1280),
            h: Math.max(1, wh > 0 ? wh : rh, 800),
        };
    };

    const rerender = async () => {
        const vp = getLayoutViewport();
        const layout = await buildLayoutTree(renderNodes, vp.w, vp.h);
        lastLayout = layout;
        uiState.scroll.contentHeight = computeScrollableContentHeight(layout);
        uiState.scroll.viewportHeight = vp.h;
        forceFullRepaint = true;
        paint();
    };
    requestRerender = () => {
        void rerender();
    };
    // Coalesce paints to at most once per frame.
    let paintScheduled = false;
    let presentScheduled = false;
    requestPresent = () => {
        if (presentScheduled || paintScheduled)
            return;
        presentScheduled = true;
        requestAnimationFrame(() => {
            presentScheduled = false;
            if (USE_DIRECT_CMD_BACKEND) {
                // In direct mode, requestPresent is typically triggered by cursor/hover churn.
                // Avoid full scene rebuilds here; cursor-plane tick owns cursor updates.
                if (USE_CURSOR_PLANE_TICK) {
                    // Present retained Graphics mutations (hover/active fills) without rebuilding scene.
                    app.renderer.render(app.stage);
                }
                else {
                    try {
                        renderCursorPlaneFrame({
                            viewportW: app.renderer.width,
                            viewportH: app.renderer.height,
                            browserContext,
                            getCursorColor,
                        });
                    }
                    catch {
                        // Keep present path resilient.
                    }
                }
            }
            else {
                app.renderer.render(app.stage);
            }
        });
    };
    requestPaint = (key = null) => {
        if (typeof key === 'string' && key.length > 0) {
            dirtyWidgetKeys.add(key);
        }
        else {
            forceFullRepaint = true;
        }
        if (paintScheduled)
            return;
        paintScheduled = true;
        requestAnimationFrame(() => {
            paintScheduled = false;
            paint();
        });
    };
    await rerender();
    // Start cursor-plane updates only after the first full scene paint.
    // This avoids brief startup flicker while text atlas and base frame settle.
    startCursorPlaneTick();
    // Cursor style shared between real + virtual cursor.
    // <details> chevron uses a 2px stroke; match that.
    const CURSOR_STROKE = 2;
    // Double the previous arm length.
    const CURSOR_HALF = 10;
    buildCrossShape(mouseCursorG, { half: CURSOR_HALF, strokeWidth: CURSOR_STROKE, color: getCursorColor(USER_POINTER_ID) });
    // Seed a primary cursor position.
    uiState.userCursorPos.set(USER_POINTER_ID, { x: app.renderer.width * 0.25, y: app.renderer.height * 0.5 });
    mouseCursorG.visible = true;
    {
        const p1 = uiState.userCursorPos.get(USER_POINTER_ID);
        if (p1)
            mouseCursorG.position.set(p1.x, p1.y);
    }
    const updateUserCursorOverlays = () => {
        // Prefer kernel-owned cursor position when available.
        let kernelButtons = 0;
        let kernelEventSeq = 0;
        let kernelWheelDelta = 0;
        let didStateChange = false;
        try {
            const x = Number(browserContext.getCursorX(USER_POINTER_ID));
            const y = Number(browserContext.getCursorY(USER_POINTER_ID));
            kernelButtons = Number(browserContext.getCursorButtons?.(USER_POINTER_ID) ?? 0) | 0;
            kernelEventSeq = Number(browserContext.getCursorEventSeq?.(USER_POINTER_ID) ?? 0) | 0;
            kernelWheelDelta = Number(browserContext.consumeCursorWheel?.(USER_POINTER_ID) ?? 0) | 0;
            const worldW = Math.max(1, Number(app.screen?.width ?? 1) || 1);
            const worldH = Math.max(1, Number(app.screen?.height ?? 1) || 1);
            const pixelW = Math.max(1, Number(app.renderer?.width ?? 1) || 1);
            const pixelH = Math.max(1, Number(app.renderer?.height ?? 1) || 1);
            const scaleDownX = worldW / pixelW;
            const scaleDownY = worldH / pixelH;
            // Ignore uninitialized kernel coordinates (commonly 0,0) so local pointer state stays visible.
            let wx = x;
            let wy = y;
            const looksWorld = Number.isFinite(x) && Number.isFinite(y) && x >= 1 && y >= 1 && x <= (worldW - 1) && y <= (worldH - 1);
            const looksPixel = Number.isFinite(x) && Number.isFinite(y) && x >= 1 && y >= 1 && x <= (pixelW - 1) && y <= (pixelH - 1);
            if (!looksWorld && looksPixel) {
                wx = x * scaleDownX;
                wy = y * scaleDownY;
            }
            const kernelLooksValid = Number.isFinite(wx) && Number.isFinite(wy) && wx >= 1 && wy >= 1 && wx <= (worldW - 1) && wy <= (worldH - 1);
            if (kernelLooksValid) {
                uiState.userCursorPos.set(USER_POINTER_ID, { x: wx, y: wy });
            }

            if (kernelWheelDelta !== 0) {
                const maxScroll = Math.max(0, uiState.scroll.contentHeight - uiState.scroll.viewportHeight);
                if (maxScroll > 0) {
                    const px = (-kernelWheelDelta) * 48;
                    uiState.scroll.y = Math.max(0, Math.min(maxScroll, uiState.scroll.y + px));
                    didStateChange = true;
                    requestPaint?.(GLOBAL_SCROLL_DIRTY_KEY);
                }
            }
        }
        catch {
            // Keep local fallback behavior when browser_context query is unavailable.
        }
        // Keep overlays and hover state in sync with uiState.userCursorPos.
        const p1 = uiState.userCursorPos.get(USER_POINTER_ID);
        if (p1) {
            mouseCursorG.visible = true;
            mouseCursorG.position.set(p1.x, p1.y);
            // Fallback bridge for cmd-stream cursor heartbeat overlay.
            globalThis.__trueosCursorDebug = {
                x: Number(p1.x) || 0,
                y: Number(p1.y) || 0,
                seq: kernelEventSeq,
                wheel: kernelWheelDelta,
                buttons: kernelButtons,
                visible: true,
            };
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
            const prevHitKey = uiState.hoveredKeyByPointer.get(USER_POINTER_ID) ?? null;
            const prevButtons = Number(uiState.kernelButtonsByPointer.get(USER_POINTER_ID) ?? 0) | 0;
            const { hitKey, hitCursor } = findHover(p1.x, p1.y);

            if (hitKey !== prevHitKey) {
                const prevHandlers = prevHitKey ? uiState.hoverHandlers.get(prevHitKey) : null;
                const nextHandlers = hitKey ? uiState.hoverHandlers.get(hitKey) : null;
                try {
                    prevHandlers?.out?.();
                    nextHandlers?.over?.();
                    didStateChange = true;
                }
                catch {
                    // Keep hover bridge resilient if widget handlers throw.
                }
            }

            const leftMask = 1;
            const prevDown = (prevButtons & leftMask) !== 0;
            const nowDown = (kernelButtons & leftMask) !== 0;
            if (!prevDown && nowDown) {
                const targetHandlers = hitKey ? uiState.hoverHandlers.get(hitKey) : null;
                try {
                    logCursorButtonEvent('down', USER_POINTER_ID, 0, p1.x, p1.y);
                    browserContext.routePointerDown(USER_POINTER_ID, p1.x, p1.y, null, 0);
                    targetHandlers?.down?.();
                    didStateChange = true;
                }
                catch {
                    // Keep kernel fallback resilient when browser_context is unavailable.
                }
            }
            else if (prevDown && !nowDown) {
                const targetHandlers = hitKey ? uiState.hoverHandlers.get(hitKey) : null;
                try {
                    logCursorButtonEvent('up', USER_POINTER_ID, 0, p1.x, p1.y);
                    browserContext.routePointerUp(USER_POINTER_ID, p1.x, p1.y, null);
                    targetHandlers?.up?.();
                    didStateChange = true;
                }
                catch {
                    // Keep kernel fallback resilient when browser_context is unavailable.
                }
            }

            uiState.hoveredKeyByPointer.set(USER_POINTER_ID, hitKey);
            uiState.hoveredCursorByPointer.set(USER_POINTER_ID, hitCursor);
            uiState.kernelButtonsByPointer.set(USER_POINTER_ID, kernelButtons);
            const isActive = uiState.textDrags.has(USER_POINTER_ID)
                || uiState.sliderDrags.has(USER_POINTER_ID)
                || uiState.dialogDrags.has(USER_POINTER_ID)
                || (kernelButtons & 0xFF) !== 0;
            mouseCursorG.rotation = hitCursor != null || isActive ? Math.PI / 4 : 0;
        }
        // Only present when UI/hover visuals changed.
        if (didStateChange) {
            requestPresent();
        }
    };
    // Keep cursor overlays alive even when scene repaint frequency is low.
    app.ticker.add(() => {
        updateUserCursorOverlays();
    });
    if (USE_DIRECT_CMD_BACKEND && USE_CURSOR_PLANE_TICK) {
        if (typeof globalThis.__trueosCursorUiSyncTimer !== 'number') {
            globalThis.__trueosCursorUiSyncTimer = setInterval(() => {
                try {
                    updateUserCursorOverlays();
                }
                catch {
                    // Keep cursor-state sync resilient if hooks fail.
                }
            }, CURSOR_PLANE_TICK_MS);
        }
    }
    // Cursor overlay state is updated from real pointer input and per-frame sync.
    // Mouse drag selection for <input>/<textarea>.
    // Also used for slider drag, dialog drag, and scrollbar thumb drag.
    app.stage.on('pointerup', (ev) => {
        const pid = getEffectivePointerId(ev);
        const gx = ev.global?.x ?? 0;
        const gy = ev.global?.y ?? 0;
        logCursorButtonEvent('up', pid, Number(ev?.button ?? 0) | 0, gx, gy);
        if (pid > 0) {
            try {
                browserContext.routePointerUp(pid, ev.global?.x ?? 0, ev.global?.y ?? 0, null);
            }
            catch {
                // Keep legacy local input path alive.
            }
        }
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
                    RT_GLOBAL.clearTimeout?.(h.timeoutId);
                if (h.intervalId != null)
                    RT_GLOBAL.clearInterval?.(h.intervalId);
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
                    requestPaint?.(ownerTemporalKey);
                }
            }
        }
        // Ensure pointerup-driven visuals (e.g. button state) are presented.
        requestPresent();
    });
    app.stage.on('pointerupoutside', (ev) => {
        const pid = getEffectivePointerId(ev);
        const gx = ev.global?.x ?? 0;
        const gy = ev.global?.y ?? 0;
        logCursorButtonEvent('upoutside', pid, Number(ev?.button ?? 0) | 0, gx, gy);
        if (pid > 0) {
            try {
                browserContext.routePointerUp(pid, ev.global?.x ?? 0, ev.global?.y ?? 0, null);
            }
            catch {
                // Keep legacy local input path alive.
            }
        }
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
                    RT_GLOBAL.clearTimeout?.(h.timeoutId);
                if (h.intervalId != null)
                    RT_GLOBAL.clearInterval?.(h.intervalId);
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
                    requestPaint?.(ownerTemporalKey);
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
        requestPaint?.(GLOBAL_SCROLL_DIRTY_KEY);
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
            uiState.userCursorPos.set(pidAny, { x: gx, y: gy });
            // Keep overlays/hover in sync as the real mouse moves.
            updateUserCursorOverlays();
        }
        const pid = getEffectivePointerId(ev);
        if (pid <= 0)
            return;
        try {
            browserContext.routePointerMove(pid, ev.global?.x ?? 0, ev.global?.y ?? 0, null);
        }
        catch {
            // Keep legacy local input path alive.
        }
        let didUpdate = false;
        const dirtyMoveKeys = new Set();
        let needsGlobalScrollPaint = false;
        let needsFullMovePaint = false;
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
                    if (typeof key === 'string' && key.length > 0)
                        dirtyMoveKeys.add(key);
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
                    if (typeof key === 'string' && key.length > 0)
                        dirtyMoveKeys.add(key);
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
                        needsFullMovePaint = true;
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
                    needsGlobalScrollPaint = true;
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
                if (typeof st.key === 'string' && st.key.length > 0)
                    dirtyMoveKeys.add(st.key);
                else
                    needsFullMovePaint = true;
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
                if (typeof drag.key === 'string' && drag.key.length > 0)
                    dirtyMoveKeys.add(drag.key);
            }
        }
        if (didUpdate) {
            for (const key of dirtyMoveKeys)
                requestPaint?.(key);
            if (needsGlobalScrollPaint)
                requestPaint?.(GLOBAL_SCROLL_DIRTY_KEY);
            if (needsFullMovePaint || (dirtyMoveKeys.size === 0 && !needsGlobalScrollPaint))
                requestPaint?.();
        }
    });
    // Keyboard input: very lightweight text editing for focused <input type=text|password>.
    RT_EVENT_TARGET?.addEventListener?.('keydown', (ev) => {
        const pid = uiState.keyboardOwnerPointerId;
        let key = uiState.focusedKeyByPointer.get(pid) ?? null;
        try {
            const k = browserContext.getFocusedTarget(pid);
            if (typeof k === 'string' && k.length > 0)
                key = k;
        }
        catch {
            // Keep local focus fallback.
        }
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
    RT_EVENT_TARGET?.addEventListener?.('resize', () => {
        void rerender();
    });
}
