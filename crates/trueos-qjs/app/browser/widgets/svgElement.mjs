import { clearGraphics, getOrCreateGraphics } from '../pixiReuse.mjs';
function stripUnsupportedSvgText(svg) {
    // Pixi's SVG parser warns that <text> is unsupported.
    return String(svg)
        .replace(/<\s*tspan\b[^>]*>[\s\S]*?<\s*\/\s*tspan\s*>/gi, '')
        .replace(/<\s*text\b[^>]*>[\s\S]*?<\s*\/\s*text\s*>/gi, '');
}
function parseViewBox(svg) {
    const m = String(svg).match(/viewBox\s*=\s*"\s*([-0-9.]+)\s+([-0-9.]+)\s+([-0-9.]+)\s+([-0-9.]+)\s*"/i);
    if (!m)
        return null;
    const minX = Number(m[1]);
    const minY = Number(m[2]);
    const w = Number(m[3]);
    const h = Number(m[4]);
    if (![minX, minY, w, h].every((n) => Number.isFinite(n)) || w <= 0 || h <= 0)
        return null;
    return { minX, minY, w, h };
}
export function applyYogaDefaultsSvg(yogaNode, node, Yoga) {
    yogaNode.setPadding(Yoga.EDGE_LEFT, 0);
    yogaNode.setPadding(Yoga.EDGE_RIGHT, 0);
    yogaNode.setPadding(Yoga.EDGE_TOP, 0);
    yogaNode.setPadding(Yoga.EDGE_BOTTOM, 0);
    // SVG behaves like a replaced element; default size similar to <canvas>.
    const wAttr = Number(node.attrs?.width ?? '0');
    const hAttr = Number(node.attrs?.height ?? '0');
    const hasW = Number.isFinite(wAttr) && wAttr > 0;
    const hasH = Number.isFinite(hAttr) && hAttr > 0;
    const w = hasW ? wAttr : 300;
    const h = hasH ? hAttr : 150;
    if (hasW || hasH) {
        yogaNode.setAlignSelf(Yoga.ALIGN_FLEX_START);
        yogaNode.setFlexGrow(0);
        yogaNode.setFlexShrink(0);
    }
    yogaNode.setWidth(w);
    yogaNode.setHeight(h);
    yogaNode.setMinWidth(Math.min(120, w));
    yogaNode.setMinHeight(Math.min(80, h));
}
export function renderSvgElement(opts) {
    const { svgMarkup, container, w, h } = opts;
    const svgString = stripUnsupportedSvgText(svgMarkup);
    const svgG = getOrCreateGraphics(container, '__svg');
    // Re-render only when the markup changes.
    const prev = svgG.__svgString;
    const prevW = svgG.__w;
    const prevH = svgG.__h;
    const needsRebuild = prev !== svgString;
    svgG.scale.set(1);
    svgG.position.set(0, 0);
    const svgFn = svgG.svg;
    if (typeof svgFn === 'function') {
        if (needsRebuild || prevW !== w || prevH !== h) {
            clearGraphics(svgG);
            let res;
            try {
                res = svgFn.call(svgG, svgString);
            }
            catch {
                res = null;
            }
            if (res && typeof res.then === 'function') {
                res.then(() => void 0).catch(() => void 0);
            }
            svgG.__svgString = svgString;
            svgG.__w = w;
            svgG.__h = h;
        }
        // Fit viewBox into the Yoga box (default preserveAspectRatio: xMidYMid meet).
        const vb = parseViewBox(svgString);
        if (vb) {
            const sx = w / vb.w;
            const sy = h / vb.h;
            const s = Math.min(sx, sy);
            const drawW = vb.w * s;
            const drawH = vb.h * s;
            svgG.scale.set(s);
            svgG.position.set(-vb.minX * s + (w - drawW) / 2, -vb.minY * s + (h - drawH) / 2);
        }
        return;
    }
    // Fallback: nothing.
}
