import { Buffer, BufferUsage, Mesh, MeshGeometry, Rectangle, Shader, colorBitGl, compileHighShaderGlProgram, localUniformBitGl, roundPixelsBitGl, } from 'pixi.js';
import { getOrCreateGraphics, getOrCreateText, clearGraphics } from '../../pixi/architecture/pixiReuse.mjs';
let sharedVertexColorBuffer = null;
function getSharedVertexColorBuffer() {
    if (sharedVertexColorBuffer)
        return sharedVertexColorBuffer;
    sharedVertexColorBuffer = new Buffer({
        data: VERTEX_COLORS,
        label: 'attribute-color-picker-colors',
        shrinkToFit: false,
        usage: BufferUsage.VERTEX | BufferUsage.COPY_DST,
    });
    return sharedVertexColorBuffer;
}
export function applyYogaDefaultsColor(yogaNode, node, Yoga) {
    yogaNode.setPadding(Yoga.EDGE_LEFT, 0);
    yogaNode.setPadding(Yoga.EDGE_RIGHT, 0);
    yogaNode.setPadding(Yoga.EDGE_TOP, 0);
    yogaNode.setPadding(Yoga.EDGE_BOTTOM, 0);
    const wAttr = Number(node.attrs?.width ?? '0');
    const hAttr = Number(node.attrs?.height ?? '0');
    const hasW = Number.isFinite(wAttr) && wAttr > 0;
    const hasH = Number.isFinite(hAttr) && hAttr > 0;
    const w = hasW ? wAttr : 240;
    const h = hasH ? hAttr : 200;
    // If author provided explicit dimensions, avoid stretch.
    if (hasW || hasH) {
        yogaNode.setAlignSelf(Yoga.ALIGN_FLEX_START);
        yogaNode.setFlexGrow(0);
        yogaNode.setFlexShrink(0);
    }
    yogaNode.setWidth(w);
    yogaNode.setHeight(h);
    yogaNode.setMinWidth(Math.min(240, w));
    yogaNode.setMinHeight(Math.min(200, h));
}
function clamp255(n) {
    if (!Number.isFinite(n))
        return 0;
    return Math.max(0, Math.min(255, Math.round(n)));
}
function toHex2(n) {
    return clamp255(n).toString(16).padStart(2, '0');
}
function pointInTri(px, py, ax, ay, bx, by, cx, cy) {
    // Same-side technique.
    const v0x = cx - ax;
    const v0y = cy - ay;
    const v1x = bx - ax;
    const v1y = by - ay;
    const v2x = px - ax;
    const v2y = py - ay;
    const dot00 = v0x * v0x + v0y * v0y;
    const dot01 = v0x * v1x + v0y * v1y;
    const dot02 = v0x * v2x + v0y * v2y;
    const dot11 = v1x * v1x + v1y * v1y;
    const dot12 = v1x * v2x + v1y * v2y;
    const invDen = 1 / (dot00 * dot11 - dot01 * dot01);
    const u = (dot11 * dot02 - dot01 * dot12) * invDen;
    const v = (dot00 * dot12 - dot01 * dot02) * invDen;
    return u >= 0 && v >= 0 && u + v <= 1;
}
function barycentric(px, py, ax, ay, bx, by, cx, cy) {
    const v0x = bx - ax;
    const v0y = by - ay;
    const v1x = cx - ax;
    const v1y = cy - ay;
    const v2x = px - ax;
    const v2y = py - ay;
    const den = v0x * v1y - v1x * v0y;
    if (Math.abs(den) < 1e-9)
        return { w0: 1, w1: 0, w2: 0 };
    const w1 = (v2x * v1y - v1x * v2y) / den;
    const w2 = (v0x * v2y - v2x * v0y) / den;
    const w0 = 1 - w1 - w2;
    return { w0, w1, w2 };
}
const solidOutBitGl = {
    name: 'solid-out',
    fragment: {
        main: /* glsl */ `
      outColor = vec4(1.0);
    `,
    },
};
let cachedShader = null;
function getColorMeshShader() {
    if (cachedShader)
        return cachedShader;
    const glProgram = compileHighShaderGlProgram({
        name: 'color-picker-vertex-color',
        // NOTE: Do NOT include globalUniformsBitGl here.
        // compileHighShaderGlProgram already provides the global uniforms in its template; adding it again
        // causes GLSL compile errors like 'uProjectionMatrix redefinition'.
        bits: [localUniformBitGl, roundPixelsBitGl, colorBitGl, solidOutBitGl],
    });
    cachedShader = new Shader({
        glProgram,
        // No additional resources; MeshPipe will bind global + local uniforms.
        resources: {},
    });
    return cachedShader;
}
function makeHexPositions(cx, cy, r) {
    const out = new Float32Array(12);
    const angles = [-90, -30, 30, 90, 150, 210];
    for (let i = 0; i < 6; i++) {
        const a = (angles[i] * Math.PI) / 180;
        out[i * 2 + 0] = cx + Math.cos(a) * r;
        out[i * 2 + 1] = cy + Math.sin(a) * r;
    }
    return out;
}
// Order matches makeHexPositions: R, RG/2, G, GB/2, B, BR/2
const VERTEX_COLORS = new Uint8Array([
    255, 0, 0, 255,
    128, 128, 0, 255,
    0, 255, 0, 255,
    0, 128, 128, 255,
    0, 0, 255, 255,
    128, 0, 128, 255,
]);
const TRI_INDICES = new Uint32Array([
    0, 1, 2,
    0, 2, 3,
    0, 3, 4,
    0, 4, 5,
]);
export function sampleColorPickerAtLocal(opts) {
    const { lx, ly, w, h } = opts;
    const pad = 10;
    const meshW = Math.max(0, w - pad * 2);
    const meshH = Math.max(0, h - pad * 2);
    const cx = pad + meshW / 2;
    const cy = pad + meshH / 2;
    const r = Math.max(0, Math.min(meshW, meshH) / 2 - 2);
    const positions = makeHexPositions(cx, cy, r);
    // Find the triangle containing the point and barycentrically interpolate colors.
    for (let ti = 0; ti < TRI_INDICES.length; ti += 3) {
        const i0 = TRI_INDICES[ti + 0];
        const i1 = TRI_INDICES[ti + 1];
        const i2 = TRI_INDICES[ti + 2];
        const ax = positions[i0 * 2 + 0];
        const ay = positions[i0 * 2 + 1];
        const bx = positions[i1 * 2 + 0];
        const by = positions[i1 * 2 + 1];
        const cx2 = positions[i2 * 2 + 0];
        const cy2 = positions[i2 * 2 + 1];
        if (!pointInTri(lx, ly, ax, ay, bx, by, cx2, cy2))
            continue;
        const bc = barycentric(lx, ly, ax, ay, bx, by, cx2, cy2);
        const c0o = i0 * 4;
        const c1o = i1 * 4;
        const c2o = i2 * 4;
        const rr = bc.w0 * VERTEX_COLORS[c0o + 0] + bc.w1 * VERTEX_COLORS[c1o + 0] + bc.w2 * VERTEX_COLORS[c2o + 0];
        const gg = bc.w0 * VERTEX_COLORS[c0o + 1] + bc.w1 * VERTEX_COLORS[c1o + 1] + bc.w2 * VERTEX_COLORS[c2o + 1];
        const bb = bc.w0 * VERTEX_COLORS[c0o + 2] + bc.w1 * VERTEX_COLORS[c1o + 2] + bc.w2 * VERTEX_COLORS[c2o + 2];
        return { r: clamp255(rr), g: clamp255(gg), b: clamp255(bb) };
    }
    return null;
}
export function renderColorPicker(opts) {
    const { node, container, graphics: g, w, h, theme, rgb, setRgb, alpha, setAlpha, pick, setPick, requestPaint, getPointerId, setDraggingPointerId, } = opts;
    const sw = 1;
    const inset = sw / 2;
    g.rect(inset, inset, Math.max(0, w - sw), Math.max(0, h - sw));
    g.fill(0xffffff);
    g.stroke({ width: sw, color: theme.control.border, alignment: 0 });
    const pad = 10;
    const meshW = Math.max(0, w - pad * 2);
    const meshH = Math.max(0, h - pad * 2);
    const cx = pad + meshW / 2;
    const cy = pad + meshH / 2;
    const r = Math.max(0, Math.min(meshW, meshH) / 2 - 2);
    const positions = makeHexPositions(cx, cy, r);
    const sizeKey = `${Math.round(w)}x${Math.round(h)}`;
    const getByLabel = container.getChildByLabel;
    let mesh = (getByLabel ? getByLabel.call(container, '__mesh') : container.children.find((c) => c?.label === '__mesh'));
    if (!mesh) {
        const uvs = new Float32Array(positions.length); // unused, but required by the template
        const geom = new MeshGeometry({ positions, uvs, indices: TRI_INDICES });
        geom.addAttribute('aColor', {
            buffer: getSharedVertexColorBuffer(),
            format: 'unorm8x4',
            stride: 4,
            offset: 0,
        });
        mesh = new Mesh({ geometry: geom, shader: getColorMeshShader() });
        mesh.label = '__mesh';
        container.addChild(mesh);
        mesh.__sizeKey = sizeKey;
    }
    else if (mesh.__sizeKey !== sizeKey) {
        // Only rebuild geometry when the control's size changes (typically on layout changes / resize).
        const uvs = new Float32Array(positions.length);
        const geom = new MeshGeometry({ positions, uvs, indices: TRI_INDICES });
        geom.addAttribute('aColor', {
            buffer: getSharedVertexColorBuffer(),
            format: 'unorm8x4',
            stride: 4,
            offset: 0,
        });
        try {
            mesh.geometry?.destroy?.();
        }
        catch {
            // Best-effort cleanup.
        }
        mesh.geometry = geom;
        mesh.__sizeKey = sizeKey;
    }
    // Events: avoid handler buildup on retained Mesh.
    mesh.removeAllListeners();
    mesh.eventMode = 'static';
    mesh.cursor = 'crosshair';
    mesh.hitArea = new Rectangle(pad, pad, meshW, meshH);
    mesh.on('pointerdown', (ev) => {
        if (ev?.button === 2)
            return;
        const pid = getPointerId(ev);
        if (pid <= 0)
            return;
        const p = container.toLocal(ev.global);
        const lx = p?.x ?? 0;
        const ly = p?.y ?? 0;
        const s = sampleColorPickerAtLocal({ lx, ly, w, h });
        if (!s)
            return;
        setPick({ x: lx, y: ly });
        setRgb(s);
        setDraggingPointerId(pid);
        requestPaint?.();
        ev.stopPropagation?.();
    });
    // Hexagon outline: 2px black border around the blended mesh.
    {
        const border = getOrCreateGraphics(container, '__border');
        clearGraphics(border);
        border.moveTo(positions[0], positions[1]);
        for (let i = 1; i < 6; i++) {
            border.lineTo(positions[i * 2 + 0], positions[i * 2 + 1]);
        }
        border.closePath();
        border.stroke({ width: 2, color: 0x000000 });
    }
    // Selected point marker + swatch.
    const overlay = getOrCreateGraphics(container, '__overlay');
    clearGraphics(overlay);
    const swatchW = 44;
    const swatchH = 18;
    const sx = Math.max(pad, w - pad - swatchW);
    const sy = pad;
    overlay.rect(sx, sy, swatchW, swatchH);
    overlay.fill({
        color: (clamp255(rgb.r) << 16) | (clamp255(rgb.g) << 8) | clamp255(rgb.b),
        alpha: Math.max(0, Math.min(1, clamp255(alpha) / 255)),
    });
    overlay.rect(sx + 0.5, sy + 0.5, swatchW - 1, swatchH - 1);
    overlay.stroke({ width: 1, color: theme.control.border, alignment: 0 });
    if (pick) {
        overlay.circle(pick.x, pick.y, 4);
        overlay.stroke({ width: 2, color: 0xffffff });
        overlay.circle(pick.x, pick.y, 4);
        overlay.stroke({ width: 1, color: 0x000000 });
    }
    const hex = `#${toHex2(rgb.r)}${toHex2(rgb.g)}${toHex2(rgb.b)}${toHex2(alpha)}`.toUpperCase();
    const t = getOrCreateText(container, '__label', (tt) => {
        tt.style = {
            fontFamily: theme.fontFamily,
            fontSize: Math.max(10, Math.floor(theme.fontSize * 0.75)),
            fill: theme.mutedText,
            fontWeight: '400',
            wordWrap: false,
        };
    });
    t.text = hex;
    t.position.set(pad, Math.max(pad, h - pad - t.height));
    // Keep alpha in [0..255] if a setter is provided.
    if (setAlpha)
        setAlpha(clamp255(alpha));
    // Container does not need to be interactive; the Mesh carries the hit area.
}
