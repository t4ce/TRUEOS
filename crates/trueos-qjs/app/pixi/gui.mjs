import { Application, Container, Graphics } from 'pixi.js';
import Yoga from 'yoga-layout';
import * as browserContext from 'trueos:browser_context';
import { defaultTheme } from '../browser/theme.mjs';
import { renderCursorPlaneFrame, renderDirectCmdFrame } from '../browser/cmd_backend.mjs';
import { clampWrappedLines, getCaretIndexFromPoint, wrapFieldTextWithIndices } from '../browser/widgets/textField.mjs';
import { applyYogaDefaultsProgressOrMeter } from '../browser/widgets/progressMeter.mjs';
import { applyYogaDefaultsSlider, createYogaNodeForSliderLabel, getOrInitSliderState as widgetGetOrInitSliderState, } from '../browser/widgets/slider.mjs';
import { getEffectiveDetailsChildren } from '../browser/widgets/detailsSummary.mjs';
import { applyYogaDefaultsDetails, applyYogaDefaultsSummary } from '../browser/widgets/detailsSummary.mjs';
import { applyYogaDefaultsHr } from '../browser/widgets/hr.mjs';
import { applyYogaDefaultsButton } from '../browser/widgets/button.mjs';
import { applyYogaDefaultsCell, applyYogaDefaultsTable, applyYogaDefaultsTr } from '../browser/widgets/table.mjs';
import { isHeadingTag } from '../browser/widgets/headings.mjs';
import { applyYogaDefaultsHeading } from '../browser/widgets/headings.mjs';
import { applyYogaDefaultsImg } from '../browser/widgets/img.mjs';
import { applyYogaDefaultsSvg } from '../browser/widgets/svgElement.mjs';
import { applyYogaDefaultsCanvas } from '../browser/widgets/canvasElement.mjs';
import { applyYogaDefaultsIframe } from '../browser/widgets/iframe.mjs';
import { applyYogaDefaultsInput } from '../browser/widgets/input.mjs';
import { applyYogaDefaultsTextarea } from '../browser/widgets/textarea.mjs';
import { applyYogaDefaultsBarrow } from '../browser/widgets/barrow.mjs';
import { applyYogaDefaultsSearchButton, applyYogaDefaultsSearchRow } from '../browser/widgets/search.mjs';
import { applyYogaDefaultsDialog, getOrInitDialogState } from '../browser/widgets/dialog.mjs';
import { applyYogaDefaultsNumber } from '../browser/widgets/number.mjs';
import { applyYogaDefaultsColor, sampleColorPickerAtLocal } from '../browser/widgets/color.mjs';
import { applyYogaDefaultsSelect, getOrInitSelectState } from '../browser/widgets/select.mjs';
import { applyYogaDefaultsTemporalInput, closeAllTemporalPopups } from '../browser/widgets/temporal.mjs';
import {
    SCROLLBAR_PAD,
    SCROLLBAR_W,
    USER_POINTER_ID,
    TRACE_POSITION_FLOW,
    TRACE_YOGA_LIFECYCLE,
    USE_CURSOR_PLANE_TICK,
    CURSOR_PLANE_TICK_MS,
    USE_WEBGPU_NATIVE_PAINT,
    GLOBAL_SCROLL_DIRTY_KEY,
    GLOBAL_MENU_DIRTY_KEY,
    RT_GLOBAL,
    RT_WINDOW,
    RT_DOCUMENT,
    RT_HAS_WINDOW_RESIZE,
    RT_EVENT_TARGET,
    uiState,
    getCursorColor,
    getEffectivePointerId,
    getMenuOwnerPointerId,
    logCursorButtonEvent,
    computeScrollableContentHeight,
    countLayoutNodes,
    normalizeWhitespace,
    getOrInitInputState,
    buildDefaultRenderNodes,
} from '../browser/ui.mjs';

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
const ROOT_SCROLL_DOMAIN = 'iframe:root';
let requestRerender = null;
let requestPaint = null;
let requestPresent = null;

function collectLayoutSnapshotRecords(root, viewportW, viewportH, maxItems = 1200) {
    const out = [];
    if (!root || typeof root !== 'object')
        return out;
    uiState.fieldBounds.clear();
    uiState.sliderBounds.clear();
    uiState.dialogDragBounds.clear();
    uiState.hoverRects.length = 0;
    uiState.hoverHandlers.clear();
    uiState.iframeRects.length = 0;
    const stack = [];
    const intersectClip = (a, b) => {
        if (!a)
            return b;
        if (!b)
            return a;
        const x0 = Math.max(Number(a.x || 0), Number(b.x || 0));
        const y0 = Math.max(Number(a.y || 0), Number(b.y || 0));
        const x1 = Math.min(Number(a.x || 0) + Number(a.w || 0), Number(b.x || 0) + Number(b.w || 0));
        const y1 = Math.min(Number(a.y || 0) + Number(a.h || 0), Number(b.y || 0) + Number(b.h || 0));
        return { x: x0, y: y0, w: Math.max(0, x1 - x0), h: Math.max(0, y1 - y0) };
    };
    const subtreeHeight = (node) => {
        let max = 0;
        const walk = (n, ay) => {
            if (!n || typeof n !== 'object')
                return;
            const ny = ay + Number(n.y || 0);
            const nh = Number(n.height || 0);
            max = Math.max(max, ny + nh);
            const kids = Array.isArray(n.children) ? n.children : [];
            for (let i = 0; i < kids.length; i++) {
                walk(kids[i], ny);
            }
        };
        const kids = Array.isArray(node?.children) ? node.children : [];
        for (let i = 0; i < kids.length; i++) {
            walk(kids[i], 0);
        }
        return max;
    };
    const kids = Array.isArray(root.children) ? root.children : [];
    for (let i = kids.length - 1; i >= 0; i--) {
        stack.push({
            node: kids[i],
            ax: Number(root.x || 0),
            ay: Number(root.y || 0),
            depth: 0,
            parentLabel: 'root',
            scrollDomain: '',
            scrollY: 0,
            clip: { x: 0, y: 0, w: viewportW, h: viewportH },
        });
    }

    while (stack.length > 0 && out.length < maxItems) {
        const cur = stack.pop();
        const n = cur?.node;
        if (!n || typeof n !== 'object')
            continue;
        const ax = Number(cur?.ax || 0);
        const ay = Number(cur?.ay || 0);
        const x = ax + Number(n.x || 0);
        const y = ay + Number(n.y || 0) - Number(cur?.scrollY || 0);
        const w = Number(n.width || 0);
        const h = Number(n.height || 0);
        const depth = Number(cur?.depth || 0) | 0;
        const key = typeof n.key === 'string' ? n.key : null;
        const tag = String(n.tagName || '').toLowerCase();
        const clip = cur?.clip || null;

        const cX0 = clip ? Number(clip.x || 0) : 0;
        const cY0 = clip ? Number(clip.y || 0) : 0;
        const cX1 = clip ? (cX0 + Number(clip.w || 0)) : viewportW;
        const cY1 = clip ? (cY0 + Number(clip.h || 0)) : viewportH;
        const offRight = x > cX1 + 12;
        const offBottom = y > cY1 + 12;
        const offLeft = (x + w) < cX0 - 12;
        const offTop = (y + h) < cY0 - 12;
        if (offRight || offBottom || offLeft || offTop)
            continue;

        if (n.kind === 'block' && w > 1 && h > 1) {
            const rx = clip ? Math.max(x, cX0) : x;
            const ry = clip ? Math.max(y, cY0) : y;
            const rw = clip ? Math.max(0, Math.min(x + w, cX1) - rx) : w;
            const rh = clip ? Math.max(0, Math.min(y + h, cY1) - ry) : h;
            if (rw > 1 && rh > 1) {
                out.push({
                    x: rx,
                    y: ry,
                    w: rw,
                    h: rh,
                    depth,
                    alpha: 1,
                    label: String(tag || 'block'),
                    scrollWithGlobal: false,
                    scrollDomain: String(cur?.scrollDomain || ''),
                });
            }

            if (key && (tag === 'input' || tag === 'textarea')) {
                if (tag === 'input') {
                    getOrInitInputState(key, n.attrs);
                }
                else if (!uiState.inputs.has(key)) {
                    uiState.inputs.set(key, { value: String(n.attrs?.value ?? '') });
                }
                const innerLeft = 8;
                const innerTop = tag === 'textarea' ? 8 : Math.max(4, Math.floor(h * 0.2));
                const innerWidth = Math.max(0, w - (innerLeft * 2));
                const maxLines = tag === 'textarea' ? Math.max(1, Math.floor((h - innerTop * 2) / (defaultTheme.fontSize * 1.25))) : 1;
                const type = String(n.attrs?.type ?? '').toLowerCase();
                uiState.fieldBounds.set(key, {
                    x,
                    y,
                    innerLeft,
                    innerTop,
                    innerWidth,
                    maxLines,
                    isPassword: type === 'password',
                });
                uiState.hoverRects.push({ key, kind: tag, cursor: 'text', x, y, w, h });
            }

            if (key && tag === 'select') {
                const initIdx = Number(n.attrs?.['data-selected-index'] ?? '0');
                getOrInitSelectState(uiState.selects, key, Number.isFinite(initIdx) ? initIdx : 0);
            }

            if (key && tag === 'slider') {
                const innerPad = Math.max(8, Math.floor(h * 0.24));
                uiState.sliderBounds.set(key, { x, y, w, h, innerPad });
                uiState.hoverRects.push({ key, kind: 'slider', cursor: 'pointer', x, y, w, h });
            }

            if (key && tag === 'iframe') {
                uiState.iframeRects.push({ key, x, y, w, h });
                const contentH = Math.max(h, subtreeHeight(n));
                const prev = uiState.iframeScroll.get(key) || {
                    y: 0,
                    contentHeight: contentH,
                    viewportHeight: h,
                    draggingPointerId: null,
                    dragOffsetY: 0,
                    track: { x: 0, y: 0, w: SCROLLBAR_W, h: 0 },
                    thumb: { x: 0, y: 0, w: SCROLLBAR_W, h: 0 },
                    key,
                };
                prev.key = key;
                prev.viewportHeight = h;
                prev.contentHeight = contentH;
                const maxScroll = Math.max(0, contentH - h);
                prev.y = Math.max(0, Math.min(maxScroll, Number(prev.y || 0)));
                const pad = 3;
                const trackW = Math.max(6, Math.min(SCROLLBAR_W, Math.floor(w * 0.06)));
                const trackX = Math.max(x + 1, x + w - trackW - pad);
                const trackY = y + pad;
                const trackH = Math.max(8, h - pad * 2);
                const thumbRatio = Math.max(0.20, Math.min(1.0, h / Math.max(h, contentH)));
                const thumbH = maxScroll <= 0 ? trackH : Math.max(trackH * 0.20, trackH * thumbRatio);
                const travel = Math.max(0, trackH - thumbH);
                const ratio = maxScroll <= 0 ? 0 : prev.y / maxScroll;
                const thumbY = trackY + travel * ratio;
                prev.track = { x: trackX, y: trackY, w: trackW, h: trackH };
                prev.thumb = { x: trackX, y: thumbY, w: trackW, h: thumbH };
                out.push({
                    x: trackX,
                    y: trackY,
                    w: trackW,
                    h: trackH,
                    depth: depth + 1,
                    alpha: 0.08,
                    label: 'iframe-scrollbar-track',
                    scrollWithGlobal: false,
                    scrollDomain: String(cur?.scrollDomain || ''),
                });
                out.push({
                    x: trackX,
                    y: thumbY,
                    w: trackW,
                    h: thumbH,
                    depth: depth + 1,
                    alpha: 0.35,
                    label: 'iframe-scrollbar-thumb',
                    scrollWithGlobal: false,
                    scrollDomain: String(cur?.scrollDomain || ''),
                });
                uiState.iframeScroll.set(key, prev);
            }

            if (key && (tag === 'button' || tag === 'select' || tag === 'summary' || tag === 'searchbutton' || tag === 'color' || tag === 'number' || tag === 'dialog')) {
                uiState.hoverRects.push({ key, kind: tag, cursor: 'pointer', x, y, w, h });
            }

            if (key && tag === 'dialog') {
                getOrInitDialogState(uiState.dialogs, key);
                uiState.dialogDragBounds.set(key, {
                    minX: 0,
                    maxX: Math.max(0, viewportW - w),
                    minY: 0,
                    maxY: Math.max(0, viewportH - h),
                });
            }
        }
        else if (n.kind === 'text') {
            const txt = String(n.text || '');
            if (txt.length > 0) {
                out.push({
                    x,
                    y,
                    w: Math.max(1, w),
                    h: Math.max(1, h),
                    depth,
                    alpha: 1,
                    label: String(cur?.parentLabel || 'text'),
                    isText: true,
                    text: txt,
                    fontSize: Number(defaultTheme.fontSize || 12),
                    color: Number(defaultTheme.text || 0x202020) >>> 0,
                    scrollWithGlobal: false,
                    scrollDomain: String(cur?.scrollDomain || ''),
                });
            }
        }

        const childList = Array.isArray(n.children) ? n.children : [];
        const parentLabel = n.kind === 'block' ? String(n.tagName || 'block') : String(cur?.parentLabel || 'text');
        let childScrollDomain = String(cur?.scrollDomain || '');
        let childScrollY = Number(cur?.scrollY || 0);
        let childClip = clip;
        if (key && tag === 'iframe') {
            childScrollDomain = `iframe:${key}`;
            childScrollY = Number(uiState.iframeScroll.get(key)?.y || 0);
            childClip = intersectClip(clip, { x, y, w, h });
        }
        for (let i = childList.length - 1; i >= 0; i--) {
            stack.push({
                node: childList[i],
                ax: x,
                ay: y,
                depth: depth + 1,
                parentLabel,
                scrollDomain: childScrollDomain,
                scrollY: childScrollY,
                clip: childClip,
            });
        }
    }

    return out;
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

function applyContextMenuAction(ownerPid, label) {
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
    const isTextField = focusedKey != null
        && uiState.fieldBounds.has(focusedKey)
        && focusedState != null
        && typeof focusedState.value === 'string';

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
    if (label === 'Paste' && focusedKey) {
        requestPaint?.(focusedKey);
    }
    else {
        requestPaint?.(GLOBAL_MENU_DIRTY_KEY);
    }
    return true;
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
export async function startGui() {
    const rootEl = RT_DOCUMENT?.getElementById?.('app') ?? RT_DOCUMENT?.body;
    const app = new Application();
    const initOpts = {
        background: '#ffffff',
        backgroundColor: 0xFFFFFF,
        clearBeforeRender: true,
        antialias: false,
        preference: 'webgpu',
    };
    if (RT_HAS_WINDOW_RESIZE)
        initOpts.resizeTo = RT_WINDOW;
    await app.init(initOpts);
    // In non-browser runtimes resizeTo may be unavailable/ineffective; force a sane surface.
    if (!(Number(app.renderer?.width || 0) > 1 && Number(app.renderer?.height || 0) > 1)) {
        const w = Math.max(1, Number(RT_WINDOW?.innerWidth ?? 0) || 1280);
        const h = Math.max(1, Number(RT_WINDOW?.innerHeight ?? 0) || 800);
        app.renderer.resize(w, h);
    }
    if (app.renderer?.background) {
        app.renderer.background.color = 0xFFFFFF;
        app.renderer.background.alpha = 1;
    }
    try {
        const rendererType = String(app.renderer?.type ?? app.renderer?.context?.type ?? 'unknown');
        console.log(`[webgpu-native] init enabled=${USE_WEBGPU_NATIVE_PAINT ? 1 : 0} pref=${String(initOpts.preference || 'none')} renderer=${rendererType} size=${Number(app.renderer?.width || 0)}x${Number(app.renderer?.height || 0)}`);
    }
    catch {
        // Keep bring-up trace non-fatal.
    }
    rootEl?.appendChild?.(app.canvas);
    const useCursorPlaneTick = USE_CURSOR_PLANE_TICK && !USE_WEBGPU_NATIVE_PAINT;
    const nativePaintTrace = {
        frame: 0,
        lastChildren: -1,
    };

    const startCursorPlaneTick = () => {
        if (!useCursorPlaneTick)
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
                }
            }
            e.preventDefault();
            return;
        }
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
                    const isOpenNow = !!browserContext.isContextMenuOpen?.(menuPid);
                    console.log(`[context-menu] open pid=${menuPid} x=${Math.round(gx)} y=${Math.round(gy)} ok=${isOpenNow ? 1 : 0}`);
                }
                catch (err) {
                    console.log(`[context-menu] open-fail pid=${menuPid} x=${Math.round(gx)} y=${Math.round(gy)} err=${String(err)}`);
                }
            }
            requestPaint?.(GLOBAL_MENU_DIRTY_KEY);
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
        if (ev?.button === 0 && pid > 0) {
            // Local fallback interaction mapping for direct-only record path.
            let hitSlider = null;
            const sliderEntries = Array.from(uiState.sliderBounds.entries());
            for (let i = sliderEntries.length - 1; i >= 0; i--) {
                const [k, b] = sliderEntries[i];
                if (gx >= b.x && gx <= b.x + b.w && gy >= b.y && gy <= b.y + b.h) {
                    hitSlider = { key: k, b };
                    break;
                }
            }
            if (hitSlider) {
                uiState.sliderDrags.set(pid, { key: hitSlider.key });
                const localX = gx - hitSlider.b.x;
                const innerW = Math.max(1, hitSlider.b.w - hitSlider.b.innerPad * 2);
                const r = (localX - hitSlider.b.innerPad) / innerW;
                const st = widgetGetOrInitSliderState(uiState.sliders, hitSlider.key, undefined);
                st.value = Math.max(0, Math.min(1, r));
                requestPaint?.(hitSlider.key);
            }

            let hitField = null;
            const fieldEntries = Array.from(uiState.fieldBounds.entries());
            for (let i = fieldEntries.length - 1; i >= 0; i--) {
                const [k, b] = fieldEntries[i];
                const bw = Math.max(1, b.innerWidth + b.innerLeft * 2);
                const bh = Math.max(1, b.innerTop * 2 + Math.max(1, b.maxLines) * dragLineHeight);
                if (gx >= b.x && gx <= b.x + bw && gy >= b.y && gy <= b.y + bh) {
                    hitField = { key: k, b };
                    break;
                }
            }
            if (hitField) {
                uiState.focusedKeyByPointer.set(pid, hitField.key);
                uiState.keyboardOwnerPointerId = pid;
                const st = uiState.inputs.get(hitField.key);
                if (st && typeof st.value === 'string') {
                    const shown = hitField.b.isPassword ? '•'.repeat(st.value.length) : st.value;
                    const lines = clampWrappedLines(wrapFieldTextWithIndices(shown, Math.max(0, hitField.b.innerWidth), dragMeasure), hitField.b.maxLines);
                    const localX = gx - hitField.b.x - hitField.b.innerLeft;
                    const localY = gy - hitField.b.y - hitField.b.innerTop;
                    const idx = getCaretIndexFromPoint({
                        fullText: shown,
                        lines,
                        localX,
                        localY,
                        lineHeight: dragLineHeight,
                        measure: dragMeasure,
                    });
                    if (!st.selections)
                        st.selections = new Map();
                    st.selections.set(pid, { start: idx, end: idx });
                    uiState.textDrags.set(pid, { key: hitField.key, anchor: idx });
                }
                requestPaint?.(hitField.key);
            }

            let hitSelect = null;
            for (let i = uiState.hoverRects.length - 1; i >= 0; i--) {
                const r = uiState.hoverRects[i];
                if (r?.kind !== 'select')
                    continue;
                if (gx >= r.x && gx <= r.x + r.w && gy >= r.y && gy <= r.y + r.h) {
                    hitSelect = r;
                    break;
                }
            }
            if (hitSelect && typeof hitSelect.key === 'string') {
                const st = uiState.selects.get(hitSelect.key);
                if (st) {
                    st.open = !st.open;
                    requestPaint?.(hitSelect.key);
                }
            }

            let hitDialog = null;
            for (let i = uiState.hoverRects.length - 1; i >= 0; i--) {
                const r = uiState.hoverRects[i];
                if (r?.kind !== 'dialog')
                    continue;
                if (gx >= r.x && gx <= r.x + r.w && gy >= r.y && gy <= r.y + Math.min(36, r.h)) {
                    hitDialog = r;
                    break;
                }
            }
            if (hitDialog && typeof hitDialog.key === 'string') {
                const ds = getOrInitDialogState(uiState.dialogs, hitDialog.key);
                uiState.dialogDrags.set(pid, {
                    key: hitDialog.key,
                    startGX: gx,
                    startGY: gy,
                    originX: Number(ds.x || 0),
                    originY: Number(ds.y || 0),
                });
                uiState.dialogSelectedBy.set(hitDialog.key, pid);
                uiState.dialogZ.set(hitDialog.key, uiState.dialogZCounter++);
                requestPaint?.(hitDialog.key);
            }
        }
        // Left click closes only THIS pointer's menu (clicks from other pointers don't dismiss it).
        if (ev?.button !== 2) {
            if (menuPid > 0) {
                try {
                    if (browserContext.isContextMenuOpen(menuPid)) {
                        browserContext.closeContextMenu(menuPid);
                        requestPaint?.(GLOBAL_MENU_DIRTY_KEY);
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
    const overlayUiRoot = new Container();
    overlayUiRoot.eventMode = 'static';
    // Overlay sits above the scene, but must not steal input.
    const overlayRoot = new Container();
    overlayRoot.eventMode = 'none';
    const nativeBg = new Graphics();
    nativeBg.eventMode = 'none';
    app.stage.addChild(nativeBg);
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
    const { renderNodes } = buildDefaultRenderNodes();
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
    let lastDirectSnapshot = null;
    let lastDirectSnapshotScrollY = 0;
    let lastDirectSnapshotScrollByDomain = {};

    const captureScrollByDomain = () => {
        const out = {};
        out[ROOT_SCROLL_DOMAIN] = Number(uiState.scroll.y || 0);
        for (const [k, st] of uiState.iframeScroll.entries()) {
            const key = String(k || '');
            if (key.length <= 0)
                continue;
            out[`iframe:${key}`] = Number(st?.y || 0);
        }
        return out;
    };

    const computeScrollDeltaByDomain = (prev, cur) => {
        const out = {};
        const keys = new Set([...Object.keys(prev || {}), ...Object.keys(cur || {})]);
        for (const k of keys) {
            out[k] = Number((cur?.[k] ?? 0) - (prev?.[k] ?? 0));
        }
        return out;
    };

    const withScrollbars = (items) => {
        const base = Array.isArray(items) ? items : [];
        const out = [];
        for (let i = 0; i < base.length; i++) {
            const it = base[i] || {};
            out.push(it);
        }
        return out;
    };

    const paint = () => {
        if (!lastLayout)
            return;
        clampScroll();
        if (USE_WEBGPU_NATIVE_PAINT) {
            nativePaintTrace.frame++;
            nativeBg.clear();
            nativeBg.rect(0, 0, app.renderer.width, app.renderer.height);
            nativeBg.fill({ color: 0xFFFFFF, alpha: 1 });
            updateScrollbarVisuals();
            const childCount = Number(app.stage?.children?.length || 0) | 0;
            const shouldLog = nativePaintTrace.frame <= 10
                || (nativePaintTrace.frame % 120) === 0
                || childCount !== nativePaintTrace.lastChildren;
            if (shouldLog) {
                console.log(`[webgpu-native] paint frame=${nativePaintTrace.frame} viewport=${app.renderer.width}x${app.renderer.height} stageChildren=${childCount} overlayChildren=${overlayRoot.children.length} hoverRects=${uiState.hoverRects.length} nativeBlocks=0`);
            }
            nativePaintTrace.lastChildren = childCount;
            app.renderer.render(app.stage);
            dirtyWidgetKeys.clear();
            forceFullRepaint = false;
            return;
        }
        const hasScrollDirty = dirtyWidgetKeys.has(GLOBAL_SCROLL_DIRTY_KEY);
        const hasMenuDirty = dirtyWidgetKeys.has(GLOBAL_MENU_DIRTY_KEY);
        const singleDirtyKey = dirtyWidgetKeys.size === 1 ? Array.from(dirtyWidgetKeys)[0] : null;
        const hasIframeScrollDirty = typeof singleDirtyKey === 'string' && singleDirtyKey.length > 0 && uiState.iframeScroll.has(singleDirtyKey);
        const scrollOnlyDirectPaint = !forceFullRepaint
            && dirtyWidgetKeys.size === 1
            && hasScrollDirty
            && Array.isArray(lastDirectSnapshot)
            && lastDirectSnapshot.length > 0;

        const menuOnlyDirectPaint = !forceFullRepaint
            && dirtyWidgetKeys.size === 1
            && hasMenuDirty
            && Array.isArray(lastDirectSnapshot)
            && lastDirectSnapshot.length > 0;

        if (scrollOnlyDirectPaint || menuOnlyDirectPaint) {
            updateScrollbarVisuals();
            const records = withScrollbars(lastDirectSnapshot);
            const nowScrollByDomain = captureScrollByDomain();
            const deltaByDomain = computeScrollDeltaByDomain(lastDirectSnapshotScrollByDomain, nowScrollByDomain);
            const deltaY = menuOnlyDirectPaint ? 0 : (uiState.scroll.y - lastDirectSnapshotScrollY);
            const rendered = renderDirectCmdFrame({
                records,
                viewportW: app.renderer.width,
                viewportH: app.renderer.height,
                worldW: app.screen.width,
                worldH: app.screen.height,
                scrollY: uiState.scroll.y,
                scrollDeltaY: deltaY,
                scrollDeltaByDomain: deltaByDomain,
                clearRgb: 0xFFFFFF,
                browserContext,
                getCursorColor,
                onMenuAction: applyContextMenuAction,
            });
            if (!rendered) {
                app.renderer.render(app.stage);
            }
            dirtyWidgetKeys.clear();
            forceFullRepaint = false;
            lastDirectSnapshotScrollY = uiState.scroll.y;
            lastDirectSnapshotScrollByDomain = nowScrollByDomain;
            return;
        }

        const directSnapshot = collectLayoutSnapshotRecords(lastLayout, app.renderer.width, app.renderer.height);
        updateScrollbarVisuals();
        const directSnapshotWithScrollbars = Array.isArray(directSnapshot)
            ? withScrollbars(directSnapshot)
            : directSnapshot;
        // Manual render (ticker is stopped).
        const records = directSnapshotWithScrollbars;
        posTraceFrame++;
        if (TRACE_POSITION_FLOW) {
            const yoga = summarizeLayoutAbs(lastLayout);
            const snap = summarizeItems(records);
            const noisy = posTraceFrame <= 3 || (posTraceFrame % 30) === 1;
            const clustered = (snap.total > 0 && (snap.nearOrigin / snap.total) > 0.6)
                || (yoga.sized > 0 && (yoga.nearOrigin / yoga.sized) > 0.6);
            if (noisy || clustered) {
                console.log(`[pos-trace] frame=${posTraceFrame} src=records yogaSized=${yoga.sized}/${yoga.total} yogaNear0=${yoga.nearOrigin} yogaBBox=${yoga.minX.toFixed(1)},${yoga.minY.toFixed(1)}..${yoga.maxX.toFixed(1)},${yoga.maxY.toFixed(1)} rec=${snap.total} recNear0=${snap.nearOrigin} recBBox=${snap.minX.toFixed(1)},${snap.minY.toFixed(1)}..${snap.maxX.toFixed(1)},${snap.maxY.toFixed(1)}`);
                if (yoga.samples.length > 0)
                    console.log(`[pos-trace:yoga-samples] ${yoga.samples.join(' | ')}`);
                if (yoga.zeroSamples.length > 0)
                    console.log(`[pos-trace:yoga-zero] ${yoga.zeroSamples.join(' | ')}`);
                if (snap.samples.length > 0)
                    console.log(`[pos-trace:record-samples] ${snap.samples.join(' | ')}`);
            }
        }
        const rendered = renderDirectCmdFrame({
            records,
            viewportW: app.renderer.width,
            viewportH: app.renderer.height,
            worldW: app.screen.width,
            worldH: app.screen.height,
            scrollY: uiState.scroll.y,
            scrollDeltaY: 0,
            scrollDeltaByDomain: {},
            clearRgb: 0xFFFFFF,
            browserContext,
            getCursorColor,
            onMenuAction: applyContextMenuAction,
        });
        if (!rendered) {
            app.renderer.render(app.stage);
        }
        try {
            const recordCount = Array.isArray(records) ? records.length : 0;
            console.log(`[richui-paint] backend=direct records=${recordCount} overlayChildren=${overlayRoot.children.length} hoverRects=${uiState.hoverRects.length} layoutNodes=${countLayoutNodes(lastLayout)}`);
        }
        catch {
            // Debug logging should never affect paint.
        }
        lastDirectSnapshot = Array.isArray(records) ? records.map((it) => ({ ...(it || {}) })) : null;
        lastDirectSnapshotScrollY = uiState.scroll.y;
        lastDirectSnapshotScrollByDomain = captureScrollByDomain();
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
    let rerenderScheduled = false;
    requestRerender = () => {
        if (rerenderScheduled)
            return;
        rerenderScheduled = true;
        requestAnimationFrame(() => {
            rerenderScheduled = false;
            void rerender();
        });
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
            if (USE_WEBGPU_NATIVE_PAINT) {
                nativeBg.clear();
                nativeBg.rect(0, 0, app.renderer.width, app.renderer.height);
                nativeBg.fill({ color: 0xFFFFFF, alpha: 1 });
                app.renderer.render(app.stage);
                return;
            }
            // Avoid Pixi stage presents because they can race/overdraw
            // cmd-stream output during bring-up and cause transient text artifacts.
            // Cursor visuals are handled by the cursor-plane tick.
            if (!useCursorPlaneTick) {
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
        const KERNEL_CURSOR_IDS = [1, 2, 3, 4];
        // HID mouse button bits are typically: 0=left, 1=right, 2=middle, 3=back, 4=forward.
        // DOM/Pixi button codes use: 0=left, 1=middle, 2=right, 3=back, 4=forward.
        const KERNEL_BIT_TO_BUTTON = [0, 2, 1, 3, 4];
        let didStateChange = false;
        const worldW = Math.max(1, Number(app.screen?.width ?? 1) || 1);
        const worldH = Math.max(1, Number(app.screen?.height ?? 1) || 1);
        const pixelW = Math.max(1, Number(app.renderer?.width ?? 1) || 1);
        const pixelH = Math.max(1, Number(app.renderer?.height ?? 1) || 1);
        const debugById = {};

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

        for (let i = 0; i < KERNEL_CURSOR_IDS.length; i++) {
            const pid = KERNEL_CURSOR_IDS[i];
            let kernelButtons = 0;
            let kernelEventSeq = 0;
            let kernelWheelDelta = 0;
            let hasKernelState = false;

            try {
                const x = Number(browserContext.getCursorX(pid));
                const y = Number(browserContext.getCursorY(pid));
                kernelButtons = Number(browserContext.getCursorButtons?.(pid) ?? 0) | 0;
                kernelEventSeq = Number(browserContext.getCursorEventSeq?.(pid) ?? 0) | 0;
                kernelWheelDelta = Number(browserContext.consumeCursorWheel?.(pid) ?? 0) | 0;
                hasKernelState = true;

                // Ignore uninitialized kernel coordinates (commonly 0,0) so local pointer state stays visible.
                // Kernel coordinates can be reported in either "world" or renderer pixel space.
                // When both look valid, pick the mapping closest to the recent UI cursor anchor.
                const looksWorld = Number.isFinite(x) && Number.isFinite(y) && x >= 1 && y >= 1 && x <= (worldW - 1) && y <= (worldH - 1);
                const looksPixel = Number.isFinite(x) && Number.isFinite(y) && x >= 1 && y >= 1 && x <= (pixelW - 1) && y <= (pixelH - 1);
                const scaledX = looksWorld ? (x / worldW) * pixelW : Number.NaN;
                const scaledY = looksWorld ? (y / worldH) * pixelH : Number.NaN;
                const rawValid = looksPixel;
                const scaledValid = Number.isFinite(scaledX) && Number.isFinite(scaledY)
                    && scaledX >= 1 && scaledY >= 1 && scaledX <= (pixelW - 1) && scaledY <= (pixelH - 1);
                let px = x;
                let py = y;
                if (rawValid && scaledValid) {
                    const prev = uiState.userCursorPos.get(pid);
                    const anchorX = Number((pid === USER_POINTER_ID && uiState.lastMouse?.has)
                        ? uiState.lastMouse.x
                        : (prev?.x ?? x));
                    const anchorY = Number((pid === USER_POINTER_ID && uiState.lastMouse?.has)
                        ? uiState.lastMouse.y
                        : (prev?.y ?? y));
                    const dRaw = Math.abs(x - anchorX) + Math.abs(y - anchorY);
                    const dScaled = Math.abs(scaledX - anchorX) + Math.abs(scaledY - anchorY);
                    if (dScaled < dRaw) {
                        px = scaledX;
                        py = scaledY;
                    }
                }
                else if (scaledValid) {
                    px = scaledX;
                    py = scaledY;
                }
                const kernelLooksValid = Number.isFinite(px) && Number.isFinite(py) && px >= 1 && py >= 1 && px <= (pixelW - 1) && py <= (pixelH - 1);
                if (kernelLooksValid) {
                    uiState.userCursorPos.set(pid, { x: px, y: py });
                }

                // Keep global scroll ownership simple for now: primary cursor wheel.
                if (pid === USER_POINTER_ID && kernelWheelDelta !== 0) {
                    const maxScroll = Math.max(0, uiState.scroll.contentHeight - uiState.scroll.viewportHeight);
                    if (maxScroll > 0) {
                        const pxWheel = (-kernelWheelDelta) * 48;
                        uiState.scroll.y = Math.max(0, Math.min(maxScroll, uiState.scroll.y + pxWheel));
                        didStateChange = true;
                        requestPaint?.(GLOBAL_SCROLL_DIRTY_KEY);
                    }
                }
            }
            catch {
                // Keep local fallback behavior when browser_context query is unavailable.
                hasKernelState = false;
            }

            const pos = uiState.userCursorPos.get(pid);
            if (!pos)
                continue;

            if (pid === USER_POINTER_ID) {
                mouseCursorG.visible = true;
                mouseCursorG.position.set(pos.x, pos.y);
                // Back-compat single-cursor bridge for existing logs/tooling.
                globalThis.__trueosCursorDebug = {
                    x: Number(pos.x) || 0,
                    y: Number(pos.y) || 0,
                    seq: kernelEventSeq,
                    wheel: kernelWheelDelta,
                    buttons: kernelButtons,
                    visible: true,
                };
            }
            debugById[pid] = {
                x: Number(pos.x) || 0,
                y: Number(pos.y) || 0,
                seq: kernelEventSeq,
                wheel: kernelWheelDelta,
                buttons: kernelButtons,
                visible: true,
            };

            const prevHitKey = uiState.hoveredKeyByPointer.get(pid) ?? null;
            const prevButtons = Number(uiState.kernelButtonsByPointer.get(pid) ?? 0) | 0;
            const { hitKey, hitCursor } = findHover(pos.x, pos.y);

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

            const targetHandlers = hitKey ? uiState.hoverHandlers.get(hitKey) : null;
            let sawAnyUp = false;
            if (hasKernelState) {
                for (let bit = 0; bit < KERNEL_BIT_TO_BUTTON.length; bit++) {
                    const mask = (1 << bit);
                    const prevDown = (prevButtons & mask) !== 0;
                    const nowDown = (kernelButtons & mask) !== 0;
                    if (prevDown === nowDown)
                        continue;
                    const button = KERNEL_BIT_TO_BUTTON[bit] ?? bit;
                    if (nowDown) {
                        try {
                            logCursorButtonEvent('down', pid, button, pos.x, pos.y);
                            if (button === 2) {
                                // Mirror stage right-click behavior for kernel-fed cursors.
                                browserContext.openContextMenu(pid, pos.x, pos.y, null);
                                requestPaint?.(GLOBAL_MENU_DIRTY_KEY);
                            }
                            else {
                                browserContext.routePointerDown(pid, pos.x, pos.y, null, button);
                                if (button === 0)
                                    targetHandlers?.down?.();
                            }
                            didStateChange = true;
                        }
                        catch {
                            // Keep kernel fallback resilient when browser_context is unavailable.
                        }
                    }
                    else {
                        sawAnyUp = true;
                        try {
                            logCursorButtonEvent('up', pid, button, pos.x, pos.y);
                            if (button === 0) {
                                targetHandlers?.up?.();
                            }
                            didStateChange = true;
                        }
                        catch {
                            // Keep kernel fallback resilient when browser_context is unavailable.
                        }
                    }
                }

                if (sawAnyUp) {
                    try {
                        browserContext.routePointerUp(pid, pos.x, pos.y, null);
                    }
                    catch {
                        // Keep kernel fallback resilient when browser_context is unavailable.
                    }
                }
                uiState.kernelButtonsByPointer.set(pid, kernelButtons);
            }

            uiState.hoveredKeyByPointer.set(pid, hitKey);
            uiState.hoveredCursorByPointer.set(pid, hitCursor);

            if (pid === USER_POINTER_ID) {
                const isActive = uiState.textDrags.has(pid)
                    || uiState.sliderDrags.has(pid)
                    || uiState.dialogDrags.has(pid)
                    || (kernelButtons & 0xFF) !== 0;
                mouseCursorG.rotation = hitCursor != null || isActive ? Math.PI / 4 : 0;
            }
        }

        globalThis.__trueosCursorDebugById = debugById;
        // Only present when UI/hover visuals changed.
        if (didStateChange) {
            requestPresent();
        }
    };
    // Keep cursor overlays alive even when scene repaint frequency is low.
    app.ticker.add(() => {
        updateUserCursorOverlays();
    });
    if (useCursorPlaneTick || USE_WEBGPU_NATIVE_PAINT) {
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
