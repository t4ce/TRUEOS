function toNdcX(x, viewportW) {
    const w = Math.max(1, Number(viewportW || 1));
    return (2.0 * (Number(x || 0) / w)) - 1.0;
}

function toNdcY(y, viewportH) {
    const h = Math.max(1, Number(viewportH || 1));
    return 1.0 - (2.0 * (Number(y || 0) / h));
}

function pushVertex12(verts, x, y, r, g, b, a, viewportW, viewportH) {
    verts.push({
        x: toNdcX(x, viewportW),
        y: toNdcY(y, viewportH),
        r: Math.max(0, Math.min(255, Number(r || 0) | 0)),
        g: Math.max(0, Math.min(255, Number(g || 0) | 0)),
        b: Math.max(0, Math.min(255, Number(b || 0) | 0)),
        a: Math.max(0, Math.min(255, Number(a == null ? 255 : a) | 0)),
    });
}

function pushRectPx(verts, x0, y0, x1, y1, r, g, b, a, viewportW, viewportH) {
    const lx = Math.min(x0, x1);
    const rx = Math.max(x0, x1);
    const ty = Math.min(y0, y1);
    const by = Math.max(y0, y1);
    if (!(rx > lx && by > ty)) return;

    pushVertex12(verts, lx, by, r, g, b, a, viewportW, viewportH);
    pushVertex12(verts, rx, by, r, g, b, a, viewportW, viewportH);
    pushVertex12(verts, rx, ty, r, g, b, a, viewportW, viewportH);

    pushVertex12(verts, lx, by, r, g, b, a, viewportW, viewportH);
    pushVertex12(verts, rx, ty, r, g, b, a, viewportW, viewportH);
    pushVertex12(verts, lx, ty, r, g, b, a, viewportW, viewportH);
}

function pushBorderPx(verts, x0, y0, x1, y1, bw, r, g, b, a, viewportW, viewportH) {
    const w = Math.max(1, Number(bw || 1));
    pushRectPx(verts, x0, y0, x1, y0 + w, r, g, b, a, viewportW, viewportH);
    pushRectPx(verts, x0, y1 - w, x1, y1, r, g, b, a, viewportW, viewportH);
    pushRectPx(verts, x0, y0, x0 + w, y1, r, g, b, a, viewportW, viewportH);
    pushRectPx(verts, x1 - w, y0, x1, y1, r, g, b, a, viewportW, viewportH);
}

function packVertices12(verts) {
    if (!Array.isArray(verts) || verts.length === 0) return null;
    const out = new Uint8Array(verts.length * 12);
    const dv = new DataView(out.buffer);
    let off = 0;
    for (let i = 0; i < verts.length; i++) {
        const v = verts[i] || {};
        dv.setFloat32(off + 0, Number(v.x || 0), true);
        dv.setFloat32(off + 4, Number(v.y || 0), true);
        out[off + 8] = Number(v.r || 0) & 0xff;
        out[off + 9] = Number(v.g || 0) & 0xff;
        out[off + 10] = Number(v.b || 0) & 0xff;
        out[off + 11] = Number(v.a == null ? 255 : v.a) & 0xff;
        off += 12;
    }
    return out;
}

function clampU8(v) {
    return Math.max(0, Math.min(255, Number(v || 0) | 0));
}

function splitRgb(rgb) {
    const c = Number(rgb == null ? 0 : rgb) >>> 0;
    return {
        r: (c >>> 16) & 0xff,
        g: (c >>> 8) & 0xff,
        b: c & 0xff,
    };
}

function mixRgb(a, b, t) {
    const aa = splitRgb(a);
    const bb = splitRgb(b);
    const k = Math.max(0, Math.min(1, Number(t || 0)));
    return {
        r: clampU8(Math.round(aa.r + (bb.r - aa.r) * k)),
        g: clampU8(Math.round(aa.g + (bb.g - aa.g) * k)),
        b: clampU8(Math.round(aa.b + (bb.b - aa.b) * k)),
    };
}

function classifySurface(label) {
    const ll = String(label || '').toLowerCase();
    if (ll.includes('dialog')) return 'dialog';
    if (ll.includes('select') || ll.includes('list') || ll.includes('option')) return 'select';
    if (ll.includes('week') || ll.includes('month') || ll.includes('date') || ll.includes('time') || ll.includes('temporal')) return 'temporal';
    if (ll.includes('button')) return 'button';
    if (ll.includes('input') || ll.includes('textarea') || ll.includes('field')) return 'input';
    return 'surface';
}

function paletteForSurface(kind) {
    if (kind === 'dialog') return { fill: 0xf8fbff, stroke: 0x5f7ca2, accent: 0x9ab7db };
    if (kind === 'select') return { fill: 0xfafcff, stroke: 0x6a7e98, accent: 0xadc5e7 };
    if (kind === 'temporal') return { fill: 0xfbfdff, stroke: 0x607992, accent: 0xa6c0e0 };
    if (kind === 'button') return { fill: 0xf2f6fb, stroke: 0x5c748f, accent: 0x9fb6d2 };
    if (kind === 'input') return { fill: 0xfdfeff, stroke: 0x74859a, accent: 0xc3d2e4 };
    return { fill: 0xf8fafc, stroke: 0x7b8898, accent: 0xc8d2de };
}

function drawPixiSnapshotItems(verts, items, viewportW, viewportH) {
    if (!Array.isArray(items) || items.length === 0) return 0;
    let drew = 0;
    for (let i = 0; i < items.length; i++) {
        const it = items[i] || {};
        if (it.isText) continue;
        const x = Number(it.x || 0);
        const y = Number(it.y || 0);
        const w = Number(it.w || 0);
        const h = Number(it.h || 0);
        if (!(w > 1 && h > 1)) continue;

        const label = String(it.label || '');
        const kind = classifySurface(label);
        const palette = paletteForSurface(kind);
        const depth = Math.max(0, Number(it.depth || 0));
        const alpha = Math.max(0.1, Math.min(1, Number(it.alpha == null ? 1 : it.alpha)));

        // Soft shadow pass.
        const shadowA = clampU8(Math.round((kind === 'dialog' ? 44 : 34) * alpha));
        const sh = kind === 'dialog' ? 2 : 1;
        pushRectPx(verts, x + sh, y + sh + 1, x + w + sh, y + h + sh + 1, 0, 0, 0, shadowA, viewportW, viewportH);

        // Base fill gets slightly cooler with depth to improve stacking readability.
        const depthMix = Math.min(0.28, depth * 0.035);
        const fillRgb = mixRgb(palette.fill, 0xe7edf5, depthMix);
        pushRectPx(verts, x, y, x + w, y + h, fillRgb.r, fillRgb.g, fillRgb.b, clampU8(Math.round(228 * alpha)), viewportW, viewportH);

        // Top sheen strip simulates subtle sprite-like highlight.
        const topH = Math.max(2, Math.min(10, Math.round(h * 0.24)));
        pushRectPx(verts, x + 1, y + 1, x + w - 1, y + 1 + topH, 255, 255, 255, clampU8(Math.round(18 * alpha)), viewportW, viewportH);

        // Accent edge helps popup/dialog/select boundaries read clearly.
        const accent = splitRgb(palette.accent);
        pushRectPx(verts, x + 1, y + 1, x + w - 1, y + 2, accent.r, accent.g, accent.b, clampU8(Math.round(130 * alpha)), viewportW, viewportH);

        const stroke = splitRgb(palette.stroke);
        const borderW = kind === 'dialog' || kind === 'select' || kind === 'temporal' ? 2 : 1;
        pushBorderPx(verts, x, y, x + w, y + h, borderW, stroke.r, stroke.g, stroke.b, clampU8(Math.round(235 * alpha)), viewportW, viewportH);
        drew++;
    }
    return drew;
}

function ensureDirectAtlasTex(cmd) {
    if (!cmd || typeof cmd.createAtlasTexture !== 'function') return 0;
    if (globalThis.__trueosDirectAtlasTex > 0) return Number(globalThis.__trueosDirectAtlasTex) | 0;
    const id = Number(cmd.createAtlasTexture(1) || 0) | 0;
    if (id > 0) globalThis.__trueosDirectAtlasTex = id;
    return id;
}

function drawPixiSnapshotText(cmd, atlasTex, items) {
    if (!cmd || atlasTex <= 0 || typeof cmd.drawAtlasText !== 'function') return 0;
    if (!Array.isArray(items) || items.length === 0) return 0;
    let n = 0;
    for (let i = 0; i < items.length; i++) {
        const it = items[i] || {};
        if (!it.isText) continue;
        const txt = String(it.text || '');
        if (txt.length <= 0) continue;
        const x = Number(it.x || 0) | 0;
        const y = Number(it.y || 0) | 0;
        const fs = Math.max(10, Math.min(44, Number(it.fontSize || 12) | 0));
        const color = Number(it.color == null ? 0x202020 : it.color) >>> 0;
        // Global readability polish: subtle drop shadow before main glyph pass.
        cmd.drawAtlasText(atlasTex, 1, x + 1, y + 1, txt, fs, 0x0f1722, 118);
        cmd.drawAtlasText(atlasTex, 1, x, y, txt, fs, color, 255);
        n++;
    }
    return n;
}

function countLayout(layout) {
    let blockCount = 0;
    let textCount = 0;
    let sizedBlocks = 0;
    let zeroBlocks = 0;

    const walk = (n) => {
        if (!n || typeof n !== 'object') return;
        if (n.kind === 'block') {
            blockCount++;
            const w = Number(n.width || 0);
            const h = Number(n.height || 0);
            if (w > 2 && h > 2) sizedBlocks++;
            else zeroBlocks++;
        }
        if (n.kind === 'text') textCount++;
        const c = Array.isArray(n.children) ? n.children : [];
        for (let i = 0; i < c.length; i++) walk(c[i]);
    };

    const kids = Array.isArray(layout && layout.children) ? layout.children : [];
    for (let i = 0; i < kids.length; i++) walk(kids[i]);

    return { blockCount, textCount, sizedBlocks, zeroBlocks };
}

const DIRECT_GLOBAL_CURSORS = [
    { id: 1, color: 0x111111, posX: 0.31, posY: 0.58 },
    { id: 2, color: 0x2563EB, posX: 0.36, posY: 0.54 },
    { id: 3, color: 0x16A34A, posX: 0.42, posY: 0.62 },
    { id: 4, color: 0xDC2626, posX: 0.47, posY: 0.57 },
];

const DIRECT_AI_CURSOR = {
    color: 0x7C3AED,
    centerX: 0.75,
    centerY: 0.25,
    radius: 120,
    speed: 0.9,
    phase: 0.0,
};

const DIRECT_MENU_LABELS = ['Copy', 'Paste', 'Close'];
const DIRECT_MENU_ITEM_W = 140;
const DIRECT_MENU_ITEM_H = 28;
const DIRECT_MENU_PAD = 6;
const DIRECT_MENU_BORDER_W = 2;

function directCursorRuntimeMap() {
    if (!(globalThis.__trueosDirectCursorRuntime instanceof Map)) {
        globalThis.__trueosDirectCursorRuntime = new Map();
    }
    return globalThis.__trueosDirectCursorRuntime;
}

function directCursorTiltMap() {
    if (!(globalThis.__trueosDirectCursorTilt instanceof Map)) {
        globalThis.__trueosDirectCursorTilt = new Map();
    }
    return globalThis.__trueosDirectCursorTilt;
}

function directMenuClickSeqMap() {
    if (!(globalThis.__trueosDirectMenuClickSeq instanceof Map)) {
        globalThis.__trueosDirectMenuClickSeq = new Map();
    }
    return globalThis.__trueosDirectMenuClickSeq;
}

function ensureDirectCursorPublicApi() {
    if (!globalThis.__trueosDirectCursorCtl) {
        globalThis.__trueosDirectCursorCtl = {
            setState(id, hovered, active) {
                const key = Number(id || 0) | 0;
                if (key <= 0)
                    return;
                const byId = directCursorTiltMap();
                const target = (hovered || active) ? (Math.PI * 0.25) : 0.0;
                byId.set(key, {
                    target,
                    active: !!active,
                    hovered: !!hovered,
                    rot: Number(byId.get(key)?.rot || 0),
                });
            },
        };
    }
    if (typeof globalThis.__pixi_smoke_set_cursor_hover !== 'function') {
        globalThis.__pixi_smoke_set_cursor_hover = (id, hovered) => {
            const key = Number(id || 0) | 0;
            const byId = directCursorTiltMap();
            const prev = byId.get(key);
            globalThis.__trueosDirectCursorCtl.setState(key, !!hovered, !!(prev && prev.active));
        };
    }
    if (typeof globalThis.__pixi_smoke_set_cursor_active !== 'function') {
        globalThis.__pixi_smoke_set_cursor_active = (id, active) => {
            const key = Number(id || 0) | 0;
            const byId = directCursorTiltMap();
            const prev = byId.get(key);
            globalThis.__trueosDirectCursorCtl.setState(key, !!(prev && prev.hovered), !!active);
        };
    }
}

function stepCursorTilt(id, dt) {
    const key = Number(id || 0) | 0;
    const byId = directCursorTiltMap();
    const rec = byId.get(key) || { target: 0.0, rot: 0.0 };
    const rot0 = Number(rec.rot || 0.0);
    const target = Number(rec.target || 0.0);
    const speed = 14.0;
    const k = Math.max(0.0, Math.min(1.0, Number(dt || 0) * speed));
    const rot = rot0 + (target - rot0) * k;
    rec.rot = rot;
    rec.target = target;
    byId.set(key, rec);
    return rot;
}

function pushRotQuadPx(verts, cx, cy, w, h, rot, r, g, b, a, viewportW, viewportH) {
    const hw = Number(w || 0) * 0.5;
    const hh = Number(h || 0) * 0.5;
    if (!(hw > 0 && hh > 0))
        return;
    const c = Math.cos(Number(rot || 0));
    const s = Math.sin(Number(rot || 0));
    const p0x = cx + (-hw * c - -hh * s);
    const p0y = cy + (-hw * s + -hh * c);
    const p1x = cx + (hw * c - -hh * s);
    const p1y = cy + (hw * s + -hh * c);
    const p2x = cx + (hw * c - hh * s);
    const p2y = cy + (hw * s + hh * c);
    const p3x = cx + (-hw * c - hh * s);
    const p3y = cy + (-hw * s + hh * c);

    pushVertex12(verts, p0x, p0y, r, g, b, a, viewportW, viewportH);
    pushVertex12(verts, p1x, p1y, r, g, b, a, viewportW, viewportH);
    pushVertex12(verts, p2x, p2y, r, g, b, a, viewportW, viewportH);
    pushVertex12(verts, p0x, p0y, r, g, b, a, viewportW, viewportH);
    pushVertex12(verts, p2x, p2y, r, g, b, a, viewportW, viewportH);
    pushVertex12(verts, p3x, p3y, r, g, b, a, viewportW, viewportH);
}

function getOrInitDirectCursorState(rt, cursor, viewportW, viewportH) {
    let st = rt.get(cursor.id | 0);
    if (st) {
        return st;
    }
    const sx = Math.max(0, Number(viewportW || 0) * Number(cursor.posX || 0));
    const sy = Math.max(0, Number(viewportH || 0) * Number(cursor.posY || 0));
    st = { x: sx, y: sy, tx: sx, ty: sy, seen: false };
    rt.set(cursor.id | 0, st);
    return st;
}

function drawCursorMenus(verts, menuText, browserContext, viewportW, viewportH) {
    if (!browserContext)
        return;
    const rt = directCursorRuntimeMap();
    const menuClickSeq = directMenuClickSeqMap();

    for (let i = 0; i < DIRECT_GLOBAL_CURSORS.length; i++) {
        const c = DIRECT_GLOBAL_CURSORS[i];
        const st = rt.get(c.id);
        if (!st || !browserContext.isContextMenuOpen)
            continue;

        let isOpen = false;
        try {
            isOpen = !!browserContext.isContextMenuOpen(c.id);
        }
        catch {
            isOpen = false;
        }
        if (!isOpen)
            continue;

        let menuX = 0;
        let menuY = 0;
        try {
            menuX = Number(browserContext.getContextMenuX ? browserContext.getContextMenuX(c.id) : 0);
            menuY = Number(browserContext.getContextMenuY ? browserContext.getContextMenuY(c.id) : 0);
        }
        catch {
            menuX = 0;
            menuY = 0;
        }

        const menuW = DIRECT_MENU_ITEM_W + DIRECT_MENU_PAD * 2;
        const menuH = DIRECT_MENU_LABELS.length * DIRECT_MENU_ITEM_H + DIRECT_MENU_PAD * 2;
        menuX = Math.max(0, Math.min(viewportW - menuW, menuX));
        menuY = Math.max(0, Math.min(viewportH - menuH, menuY));

        pushRectPx(verts, menuX, menuY, menuX + menuW, menuY + menuH, 255, 255, 255, 255, viewportW, viewportH);
        const owner = splitRgb(c.color);
        const bw = DIRECT_MENU_BORDER_W;
        pushRectPx(verts, menuX, menuY, menuX + menuW, menuY + bw, owner.r, owner.g, owner.b, 255, viewportW, viewportH);
        pushRectPx(verts, menuX, menuY + menuH - bw, menuX + menuW, menuY + menuH, owner.r, owner.g, owner.b, 255, viewportW, viewportH);
        pushRectPx(verts, menuX, menuY, menuX + bw, menuY + menuH, owner.r, owner.g, owner.b, 255, viewportW, viewportH);
        pushRectPx(verts, menuX + menuW - bw, menuY, menuX + menuW, menuY + menuH, owner.r, owner.g, owner.b, 255, viewportW, viewportH);

        let hoveredItem = -1;
        const px = Number(st.x || 0);
        const py = Number(st.y || 0);
        for (let item = 0; item < DIRECT_MENU_LABELS.length; item++) {
            const rowX = menuX + DIRECT_MENU_PAD;
            const rowY = menuY + DIRECT_MENU_PAD + item * DIRECT_MENU_ITEM_H;
            const rowHover = px >= rowX && px <= (rowX + DIRECT_MENU_ITEM_W) && py >= rowY && py <= (rowY + DIRECT_MENU_ITEM_H);
            if (rowHover)
                hoveredItem = item;
            const fill = rowHover ? 0xF2F2F2 : 0xFFFFFF;
            const fr = (fill >>> 16) & 0xff;
            const fg = (fill >>> 8) & 0xff;
            const fb = fill & 0xff;
            pushRectPx(verts, rowX, rowY, rowX + DIRECT_MENU_ITEM_W, rowY + DIRECT_MENU_ITEM_H, fr, fg, fb, 255, viewportW, viewportH);
            menuText.push({
                x: (rowX + 8) | 0,
                y: (rowY + ((DIRECT_MENU_ITEM_H - 12) * 0.5)) | 0,
                text: DIRECT_MENU_LABELS[item],
                color: 0x202020,
                size: 12,
            });
        }

        if (browserContext.getPointerDownSeq) {
            let seq = 0;
            let button = 0;
            try {
                seq = Number(browserContext.getPointerDownSeq(c.id) || 0) | 0;
                button = Number(browserContext.getPointerDownButton ? browserContext.getPointerDownButton(c.id) : 0) | 0;
            }
            catch {
                seq = 0;
                button = 0;
            }
            const prevSeq = Number(menuClickSeq.get(c.id) || 0) | 0;
            if (seq !== prevSeq) {
                menuClickSeq.set(c.id, seq);
                if (hoveredItem >= 0 && button !== 2) {
                    let target = null;
                    try {
                        target = (browserContext.getFocusedTarget && browserContext.getFocusedTarget(c.id))
                            || (browserContext.getContextMenuTarget && browserContext.getContextMenuTarget(c.id))
                            || (browserContext.getHoveredTarget && browserContext.getHoveredTarget(c.id))
                            || null;
                    }
                    catch {
                        target = null;
                    }

                    if (hoveredItem === 0 && browserContext.setClipboardText && target != null) {
                        browserContext.setClipboardText(c.id, String(target));
                    }
                    else if (hoveredItem === 1 && browserContext.getClipboardText) {
                        const clip = browserContext.getClipboardText(c.id) ?? '';
                        if (clip.length > 0 && browserContext.setClipboardText) {
                            browserContext.setClipboardText(c.id, clip);
                        }
                    }

                    if (browserContext.closeContextMenu) {
                        browserContext.closeContextMenu(c.id);
                    }
                }
            }
        }
    }
}

function drawAllCursors(verts, browserContext, getCursorColor, viewportW, viewportH, dt, t, menuText) {
    ensureDirectCursorPublicApi();
    const rt = directCursorRuntimeMap();
    for (let i = 0; i < DIRECT_GLOBAL_CURSORS.length; i++) {
        const c = DIRECT_GLOBAL_CURSORS[i];
        const st = getOrInitDirectCursorState(rt, c, viewportW, viewportH);

        if (browserContext) {
            let bx = Number.NaN;
            let by = Number.NaN;
            try {
                bx = Number(browserContext.getCursorX ? browserContext.getCursorX(c.id) : Number.NaN);
                by = Number(browserContext.getCursorY ? browserContext.getCursorY(c.id) : Number.NaN);
            } catch {
                // Keep cursor fallback positions.
            }

            let hovered = false;
            let focused = false;
            let menuOpen = false;
            try {
                hovered = !!(browserContext.getHoveredTarget && browserContext.getHoveredTarget(c.id));
                focused = !!(browserContext.getFocusedTarget && browserContext.getFocusedTarget(c.id));
                menuOpen = !!(browserContext.isContextMenuOpen && browserContext.isContextMenuOpen(c.id));
            } catch {
                // Keep signal probing resilient.
            }

            const hasPos = Number.isFinite(bx) && Number.isFinite(by);
            const hasSignal = hovered || focused || menuOpen || (hasPos && (bx !== 0 || by !== 0));
            if (hasPos && (hasSignal || st.seen)) {
                st.tx = bx;
                st.ty = by;
                st.seen = true;
            }
            globalThis.__trueosDirectCursorCtl.setState(c.id, hovered, focused || menuOpen);
        }

        const followK = Math.max(0.0, Math.min(1.0, Number(dt || 0) * 18.0));
        st.x = st.x + (st.tx - st.x) * followK;
        st.y = st.y + (st.ty - st.y) * followK;
        const rot = stepCursorTilt(c.id, dt);

        let color = Number(c.color || 0x111111) >>> 0;
        try {
            color = Number(getCursorColor(c.id)) >>> 0;
        } catch {
            // Keep per-cursor fallback color.
        }
        drawCursorCross(verts, st.x, st.y, color, viewportW, viewportH, rot);
    }

    // Dedicated animated AI cursor.
    {
        const ai = DIRECT_AI_CURSOR;
        const a = Number(t || 0) * ai.speed + ai.phase;
        const cx = viewportW * ai.centerX;
        const cy = viewportH * ai.centerY;
        const x = cx + Math.cos(a) * ai.radius;
        const y = cy + Math.sin(a) * ai.radius;
        const rot = Math.sin(a * 1.7) * 0.35;
        drawCursorCross(verts, x, y, ai.color, viewportW, viewportH, rot);
    }

    drawCursorMenus(verts, menuText, browserContext, viewportW, viewportH);
}

function drawCursorCross(verts, x, y, color, viewportW, viewportH, rot = 0) {
    const col = Number(color == null ? 0x111111 : color) >>> 0;
    const r = (col >>> 16) & 0xff;
    const g = (col >>> 8) & 0xff;
    const b = col & 0xff;
    const arm = 10;
    const stroke = 2;
    pushRotQuadPx(verts, x, y, arm * 2.0, stroke, rot, r, g, b, 255, viewportW, viewportH);
    pushRotQuadPx(verts, x, y, stroke, arm * 2.0, rot, r, g, b, 255, viewportW, viewportH);
}

export function renderDirectCmdFrame(opts = {}) {
    const cmd = globalThis.__trueosCmdStream;
    if (!cmd || typeof cmd.beginFrame !== 'function' || typeof cmd.endFrame !== 'function' || typeof cmd.drawTrianglesU8 !== 'function') {
        return false;
    }

    const layout = opts.layout || null;
    if (!layout) return false;

    const viewportW = Math.max(1, Number(opts.viewportW || 1) | 0);
    const viewportH = Math.max(1, Number(opts.viewportH || 1) | 0);
    const scrollY = Math.max(0, Number(opts.scrollY || 0));
    const clearRgb = (opts.clearRgb == null) ? 0xFFFFFF : (Number(opts.clearRgb) >>> 0);
    const browserContext = opts.browserContext || null;
    const pixiItems = Array.isArray(opts.pixiItems) ? opts.pixiItems : [];
    const getCursorColor = typeof opts.getCursorColor === 'function' ? opts.getCursorColor : (() => 0x111111);
    const nowMs = Date.now();
    const prevMs = Number(globalThis.__trueosDirectCursorLastMs || nowMs);
    let dt = (nowMs - prevMs) / 1000.0;
    if (!Number.isFinite(dt) || dt <= 0)
        dt = 0.05;
    dt = Math.max(0.001, Math.min(0.25, dt));
    globalThis.__trueosDirectCursorLastMs = nowMs;
    const t = Number(globalThis.__trueosDirectCursorTime || 0) + dt;
    globalThis.__trueosDirectCursorTime = t;

    const counts = countLayout(layout);
    const verts = [];
    const menuText = [];
    const pixiDrawn = drawPixiSnapshotItems(verts, pixiItems, viewportW, viewportH);

    drawAllCursors(verts, browserContext, getCursorColor, viewportW, viewportH, dt, t, menuText);

    const bytes = packVertices12(verts);
    try {
        console.log(`[direct-backend] blocks=${counts.blockCount} sized=${counts.sizedBlocks} zero=${counts.zeroBlocks} text=${counts.textCount} pixi=${pixiItems.length}/${pixiDrawn} verts=${verts.length} bytes=${bytes ? bytes.byteLength : 0} scrollY=${scrollY} vp=${viewportW}x${viewportH}`);
    } catch {
        // Keep render path resilient if logging fails.
    }

    if (typeof cmd.setViewport === 'function') cmd.setViewport(viewportW, viewportH);
    if (typeof cmd.setClearRgb === 'function') cmd.setClearRgb(clearRgb);
    cmd.beginFrame();
    if (bytes && bytes.byteLength > 0) {
        if (typeof cmd.setBlendEnabled === 'function') cmd.setBlendEnabled(false);
        cmd.drawTrianglesU8(bytes);
    }

    const atlasTex = ensureDirectAtlasTex(cmd);
    if (typeof cmd.setBlendEnabled === 'function') cmd.setBlendEnabled(true);
    if (typeof cmd.setBlendMode === 'function') cmd.setBlendMode(0);
    const textDrawn = drawPixiSnapshotText(cmd, atlasTex, pixiItems);
    for (let i = 0; i < menuText.length; i++) {
        const row = menuText[i] || {};
        const x = Number(row.x || 0) | 0;
        const y = Number(row.y || 0) | 0;
        const txt = String(row.text || '');
        if (txt.length <= 0)
            continue;
        const fs = Math.max(10, Math.min(44, Number(row.size || 12) | 0));
        const color = Number(row.color == null ? 0x202020 : row.color) >>> 0;
        cmd.drawAtlasText(atlasTex, 1, x + 1, y + 1, txt, fs, 0x0f1722, 98);
        cmd.drawAtlasText(atlasTex, 1, x, y, txt, fs, color, 255);
    }
    try {
        if (textDrawn > 0) {
            console.log(`[direct-backend] atlas-text drawn=${textDrawn} atlas=${atlasTex}`);
        }
    } catch {
        // Keep render path resilient if logging fails.
    }

    cmd.endFrame();
    return true;
}
