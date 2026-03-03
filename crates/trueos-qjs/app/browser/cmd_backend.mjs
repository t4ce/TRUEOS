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

        const label = String(it.label || '').toLowerCase();
        let br = 132;
        let bg = 146;
        let bb = 164;
        if (label.includes('button')) {
            br = 95;
            bg = 118;
            bb = 152;
        } else if (label.includes('input') || label.includes('textarea') || label.includes('select')) {
            br = 140;
            bg = 148;
            bb = 158;
        } else if (label.includes('dialog')) {
            br = 96;
            bg = 124;
            bb = 162;
        }

        pushBorderPx(verts, x, y, x + w, y + h, 1, br, bg, bb, 255, viewportW, viewportH);
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

function drawCursorCross(verts, x, y, color, viewportW, viewportH) {
    const col = Number(color == null ? 0x111111 : color) >>> 0;
    const r = (col >>> 16) & 0xff;
    const g = (col >>> 8) & 0xff;
    const b = col & 0xff;
    const arm = 10;
    const half = 1;
    pushRectPx(verts, x - arm, y - half, x + arm, y + half, r, g, b, 255, viewportW, viewportH);
    pushRectPx(verts, x - half, y - arm, x + half, y + arm, r, g, b, 255, viewportW, viewportH);
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

    const counts = countLayout(layout);
    const verts = [];
    const pixiDrawn = drawPixiSnapshotItems(verts, pixiItems, viewportW, viewportH);

    let cx = Number(viewportW * 0.25);
    let cy = Number(viewportH * 0.5);
    try {
        if (browserContext) {
            const x = Number(browserContext.getCursorX(1));
            const y = Number(browserContext.getCursorY(1));
            if (Number.isFinite(x) && Number.isFinite(y)) {
                cx = x;
                cy = y;
            }
        }
    } catch {
        // Keep fallback cursor position.
    }
    drawCursorCross(verts, cx, cy, getCursorColor(1), viewportW, viewportH);

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
