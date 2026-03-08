import * as parse5 from 'parse5';
import Yoga from 'yoga-layout';
import * as lightningcss from 'trueos:lightningcss';
import * as lyon from 'trueos:lyon';
import { createFpsOverlay } from './fps.mjs';
import { INDENT, LEFT_PAD, TOP_PAD, LINE_H } from './theme.mjs';

const G = (typeof globalThis !== 'undefined') ? globalThis : this;
const VOID_TAGS = new Set([
  'area', 'base', 'br', 'col', 'embed', 'hr', 'img', 'input',
  'link', 'meta', 'param', 'source', 'track', 'wbr',
]);

const WHEEL_STEP_PX = 32;

let cachedHtml = '';
let cacheKey = '';
let cachedDoc = null;
let cursorReadSeq = 0;
let scrollY = 0;
let cachedCssObjects = [];
let lyonRoundRectRuns = [];
let cachedLyonDemo = null;
let cachedLyonMeshes = null;
const fpsOverlay = createFpsOverlay();

function getLyonDemo() {
  if (cachedLyonDemo) return cachedLyonDemo;
  if (!lyon || lyon.isAvailable !== true || typeof lyon.demoShapes !== 'function') {
    return null;
  }
  try {
    const d = lyon.demoShapes();
    cachedLyonDemo = d && d.ok === true ? d : null;
  } catch (_) {
    cachedLyonDemo = null;
  }
  return cachedLyonDemo;
}

function getLyonMeshes() {
  if (cachedLyonMeshes) return cachedLyonMeshes;
  if (!lyon || lyon.isAvailable !== true) return null;

  if (typeof lyon.demoMeshes === 'function') {
    try {
      const m = lyon.demoMeshes();
      if (m && m.ok === true && Array.isArray(m.meshes) && m.meshes.length > 0) {
        cachedLyonMeshes = m;
        return m;
      }
    } catch (_) {}
  }

  const demo = getLyonDemo();
  if (!demo || demo.triangleTessOk !== true) return null;
  const verts = Array.isArray(demo.triangleVertices) ? demo.triangleVertices : [];
  const idx = Array.isArray(demo.triangleIndices) ? demo.triangleIndices : [];
  if (((verts.length / 2) | 0) <= 0 || idx.length < 3) return null;

  cachedLyonMeshes = {
    ok: true,
    meshCount: 1,
    meshes: [{
      name: 'roundRectThin',
      vertices: verts,
      indices: idx,
      vertexCount: (verts.length / 2) | 0,
      indexCount: idx.length | 0,
      triangleCount: (idx.length / 3) | 0,
    }],
  };
  return cachedLyonMeshes;
}

function rebuildLyonRuns(vw, vh) {
  lyonRoundRectRuns.length = 0;
  const meshDemo = getLyonMeshes();
  if (!meshDemo || meshDemo.ok !== true || !Array.isArray(meshDemo.meshes)) return;

  // Packet format per mesh:
  // [x, y, scale, r, g, b, a, vertex_count, index_count, ...verts_xy, ...indices]
  const pushMesh = (x, y, scale, color, verts, idx) => {
    const vcount = (verts.length / 2) | 0;
    const icount = idx.length | 0;
    if (vcount <= 0 || icount < 3) return;
    lyonRoundRectRuns.push(x, y, scale, color[0], color[1], color[2], color[3], vcount, icount);
    for (let i = 0; i < vcount * 2; i++) {
      lyonRoundRectRuns.push(Number(verts[i] || 0));
    }
    for (let i = 0; i < icount; i++) {
      lyonRoundRectRuns.push(Number(idx[i] || 0));
    }
  };

  const palette = [
    [20, 120, 230, 255],
    [16, 170, 120, 255],
    [210, 130, 24, 255],
    [180, 70, 140, 255],
  ];
  const scales = [3.8, 2.4, 1.9, 1.6];
  const anchorX = Math.max(530, Math.round(vw * 0.56));
  const anchorY = Math.max(72, Math.round(vh * 0.14));
  const cols = 2;
  const slotX = 200;
  const slotY = 120;

  for (let i = 0; i < meshDemo.meshes.length; i++) {
    const m = meshDemo.meshes[i];
    const verts = Array.isArray(m && m.vertices) ? m.vertices : [];
    const idx = Array.isArray(m && m.indices) ? m.indices : [];
    if (((verts.length / 2) | 0) <= 0 || idx.length < 3) continue;

    const col = i % cols;
    const row = (i / cols) | 0;
    const x = anchorX + (col * slotX);
    const y = anchorY + (row * slotY);
    const color = palette[i % palette.length];
    const scale = scales[i % scales.length];
    pushMesh(x, y, scale, color, verts, idx);
  }
}

function appendLyonDemoLines(out) {
  out.push('/* DEMOS (lyon) */');

  const catalogFn = G.__trueosLyonCatalogLines;
  if (typeof catalogFn === 'function') {
    let catalog = null;
    try {
      catalog = catalogFn();
    } catch (_) {
      catalog = null;
    }
    if (Array.isArray(catalog)) {
      for (let i = 0; i < catalog.length; i++) {
        out.push(String(catalog[i] || ''));
      }
    }
  }

  // One-shot probe: ask lyon to build/tessellate a rounded-rect path and report results.
  if (!lyon || lyon.isAvailable !== true || typeof lyon.demoShapes !== 'function') {
    out.push('lyon backend unavailable');
    out.push('');
    return;
  }

  const demo = getLyonDemo();
  if (!demo) {
    out.push(`lyon demo failed: ${String((demo && demo.error) || 'unknown error')}`);
    out.push('');
    return;
  }

  out.push('Live Demo Metrics');
  out.push(`${INDENT}path: rounded rectangle (44x28), radius=6, stroke=1`);
  const meshDemo = getLyonMeshes();
  if (meshDemo && meshDemo.ok === true && Array.isArray(meshDemo.meshes)) {
    out.push(`${INDENT}mesh demo: count=${Number(meshDemo.meshCount || meshDemo.meshes.length || 0) | 0}`);
    for (let i = 0; i < meshDemo.meshes.length; i++) {
      const m = meshDemo.meshes[i] || {};
      const name = String(m.name || `mesh${i}`);
      const v = Number(m.vertexCount || 0) | 0;
      const idx = Number(m.indexCount || 0) | 0;
      const tris = Number(m.triangleCount || 0) | 0;
      out.push(`${INDENT}${name}: verts=${v} idx=${idx} tris=${tris}`);
    }
  } else if (meshDemo && meshDemo.ok === false) {
    out.push(`${INDENT}mesh demo failed: ${String(meshDemo.error || 'unknown')}`);
  }
  if (demo.triangleTessOk === true) {
    const verts = Number(demo.triangleTessVertices || 0) | 0;
    const idx = Number(demo.triangleTessIndices || 0) | 0;
    const tris = Number(demo.triangleTessTriangles || 0) | 0;
    out.push(`${INDENT}tess result: verts=${verts} idx=${idx} tris=${tris}`);
  } else {
    out.push(`${INDENT}tess failed: ${String(demo.triangleTessError || 'unknown')}`);
  }

  out.push(`${INDENT}line: len=${Number(demo.lineLength || 0).toFixed(2)} left=${Number(demo.lineLeftLen || 0).toFixed(2)} right=${Number(demo.lineRightLen || 0).toFixed(2)}`);
  out.push(`${INDENT}quad: len=${Number(demo.quadLength || 0).toFixed(2)} approx=${Number(demo.quadApproxLength || 0).toFixed(2)} baseline=${Number(demo.quadBaselineLen || 0).toFixed(2)}`);
  out.push(`${INDENT}cubic: approx=${Number(demo.cubicApproxLength || 0).toFixed(2)} baseline=${Number(demo.cubicBaselineLen || 0).toFixed(2)}`);
  out.push(`${INDENT}triangle: area=${Number(demo.triangleArea || 0).toFixed(2)} signed=${Number(demo.triangleSignedArea || 0).toFixed(2)}`);
  out.push('');
}

function computeViewport() {
  const W = G.window || G;
  const vw = Math.max(1, Number(W.innerWidth || 1280));
  const vh = Math.max(1, Number(W.innerHeight || 800));
  return { vw, vh };
}

function collapseWhitespace(s) {
  return String(s || '').replace(/\s+/g, ' ').trim();
}

function isElement(node) {
  return !!node && typeof node === 'object' && typeof node.tagName === 'string';
}

function isTextNode(node) {
  return !!node && typeof node === 'object' && node.nodeName === '#text' && typeof node.value === 'string';
}

function attrsToString(node) {
  if (!node || !Array.isArray(node.attrs) || node.attrs.length <= 0) return '';
  const out = [];
  for (let i = 0; i < node.attrs.length; i++) {
    const a = node.attrs[i];
    const k = String(a && a.name || '').trim();
    if (!k) continue;
    const rawV = String(a && a.value != null ? a.value : '');
    const v = rawV.replace(/"/g, '&quot;');
    out.push(v.length > 0 ? `${k}="${v}"` : k);
  }
  return out.length > 0 ? ` ${out.join(' ')}` : '';
}

function getAttr(node, name) {
  if (!node || !Array.isArray(node.attrs)) return '';
  const key = String(name || '').toLowerCase();
  for (let i = 0; i < node.attrs.length; i++) {
    const a = node.attrs[i];
    if (String(a && a.name || '').toLowerCase() !== key) continue;
    return String(a && a.value != null ? a.value : '');
  }
  return '';
}

function parseInlineStyleToKernelObject(styleText) {
  if (!styleText) return null;
  if (!lightningcss || typeof lightningcss.parseInlineStyle !== 'function') {
    return null;
  }
  const parsed = lightningcss.parseInlineStyle(String(styleText));
  if (!parsed || parsed.ok !== true) return null;
  return {
    kind: 'inline',
    source: String(styleText),
    css: String(parsed.css || ''),
    declarations: Array.isArray(parsed.declarations) ? parsed.declarations : [],
  };
}

function parseStylesheetToKernelObject(cssText) {
  if (!cssText) return null;
  if (!lightningcss || typeof lightningcss.parseStylesheet !== 'function') {
    return {
      kind: 'stylesheet',
      source: String(cssText),
      css: String(cssText),
      declarations: [],
      parsed: false,
    };
  }
  const parsed = lightningcss.parseStylesheet(String(cssText));
  if (!parsed || parsed.ok !== true) {
    return {
      kind: 'stylesheet',
      source: String(cssText),
      css: String(cssText),
      declarations: [],
      parsed: false,
    };
  }
  return {
    kind: 'stylesheet',
    source: String(cssText),
    css: String(parsed.css || ''),
    declarations: [],
    parsed: true,
  };
}

function nodeTextContent(node) {
  if (!node || typeof node !== 'object') return '';
  if (isTextNode(node)) return String(node.value || '');
  const kids = Array.isArray(node.childNodes) ? node.childNodes : [];
  let out = '';
  for (let i = 0; i < kids.length; i++) {
    out += nodeTextContent(kids[i]);
  }
  return out;
}

function collectCssObjects(node, path, out) {
  if (!node || typeof node !== 'object') return;
  if (isElement(node)) {
    const tag = String(node.tagName || '').toLowerCase();
    const styleText = getAttr(node, 'style');
    const parsed = parseInlineStyleToKernelObject(styleText);
    if (parsed) {
      out.push({
        path,
        tag,
        style: parsed,
      });
    }

    if (tag === 'style') {
      const cssText = nodeTextContent(node);
      const sheet = parseStylesheetToKernelObject(cssText);
      if (sheet) {
        out.push({
          path,
          tag,
          style: sheet,
        });
      }
    }

    if (tag === 'link') {
      const rel = String(getAttr(node, 'rel') || '').toLowerCase();
      if (rel.includes('stylesheet')) {
        const href = String(getAttr(node, 'href') || '');
        out.push({
          path,
          tag,
          style: {
            kind: 'external',
            source: href,
            css: '',
            declarations: [],
            parsed: false,
            unresolved: true,
          },
        });
      }
    }
  }

  const kids = Array.isArray(node.childNodes) ? node.childNodes : [];
  for (let i = 0; i < kids.length; i++) {
    collectCssObjects(kids[i], `${path}.${i}`, out);
  }
}

function formatCssText(cssText, depth) {
  const raw = String(cssText || '').trim();
  if (!raw) return [];
  const lines = [];
  let cur = '';
  let d = Math.max(0, Number(depth || 0) | 0);
  for (let i = 0; i < raw.length; i++) {
    const ch = raw[i];
    if (ch === '{') {
      if (cur.trim()) lines.push(`${INDENT.repeat(d)}${cur.trim()} {`);
      else lines.push(`${INDENT.repeat(d)}{`);
      cur = '';
      d += 1;
      continue;
    }
    if (ch === '}') {
      if (cur.trim()) lines.push(`${INDENT.repeat(d)}${cur.trim()}`);
      cur = '';
      d = Math.max(0, d - 1);
      lines.push(`${INDENT.repeat(d)}}`);
      continue;
    }
    if (ch === ';') {
      cur += ';';
      if (cur.trim()) lines.push(`${INDENT.repeat(d)}${cur.trim()}`);
      cur = '';
      continue;
    }
    cur += ch;
  }
  if (cur.trim()) lines.push(`${INDENT.repeat(d)}${cur.trim()}`);
  return lines;
}

function appendCssLines(out, cssObjects) {
  out.push('');
  out.push('/* CSS */');
  if (!Array.isArray(cssObjects) || cssObjects.length <= 0) {
    out.push('(no styles found)');
    return;
  }

  for (let i = 0; i < cssObjects.length; i++) {
    const it = cssObjects[i];
    const path = String(it && it.path || '');
    const tag = String(it && it.tag || '');
    const style = it && it.style || null;
    const kind = String(style && style.kind || 'unknown');
    out.push(`[${i}] ${path} <${tag}> ${kind}`);

    if (kind === 'external') {
      const href = String(style && style.source || '');
      out.push(`${INDENT}href: ${href || '(missing href)'}`);
      continue;
    }

    const css = String(style && style.css || '');
    const cssLines = formatCssText(css, 1);
    if (cssLines.length <= 0) {
      out.push(`${INDENT}(empty css)`);
      continue;
    }
    for (let j = 0; j < cssLines.length; j++) {
      out.push(cssLines[j]);
    }
  }
}

function formatNode(node, depth, out) {
  if (!node) return;

  if (isTextNode(node)) {
    const t = collapseWhitespace(node.value);
    if (t) out.push(`${INDENT.repeat(depth)}${t}`);
    return;
  }

  if (!isElement(node)) return;

  const tag = String(node.tagName || '').toLowerCase();
  if (!tag) return;

  const open = `<${tag}${attrsToString(node)}>`;
  const close = `</${tag}>`;
  const kids = Array.isArray(node.childNodes) ? node.childNodes : [];

  if (VOID_TAGS.has(tag)) {
    out.push(`${INDENT.repeat(depth)}${open}`);
    return;
  }

  if (kids.length === 1 && isTextNode(kids[0])) {
    const inlineText = collapseWhitespace(kids[0].value);
    if (inlineText) {
      out.push(`${INDENT.repeat(depth)}${open}${inlineText}${close}`);
      return;
    }
  }

  out.push(`${INDENT.repeat(depth)}${open}`);
  for (let i = 0; i < kids.length; i++) {
    formatNode(kids[i], depth + 1, out);
  }
  out.push(`${INDENT.repeat(depth)}${close}`);
}

function formatHtmlToLines(html) {
  const doc = parse5.parse(String(html || ''));
  const out = [];
  const cssObjects = [];
  const kids = Array.isArray(doc && doc.childNodes) ? doc.childNodes : [];

  appendLyonDemoLines(out);

  for (let i = 0; i < kids.length; i++) {
    const k = kids[i];
    if (!k) continue;
    if (String(k.nodeName || '').toLowerCase() === '#documentType') {
      const name = collapseWhitespace(k.name || 'html') || 'html';
      out.push(`<!DOCTYPE ${name}>`);
      continue;
    }
    collectCssObjects(k, `root.${i}`, cssObjects);
    formatNode(k, 0, out);
  }

  if (out.length <= 0) out.push('(empty document)');
  appendCssLines(out, cssObjects);
  cachedCssObjects = cssObjects;
  G.__trueosKernelCssObjects = cssObjects;
  return out;
}

function currentLines() {
  const html = String(G.__trueosUiHtml || '');
  if (html === cachedHtml && cachedDoc && Array.isArray(cachedDoc.lines)) return cachedDoc.lines;
  cachedHtml = html;
  const lines = formatHtmlToLines(html);
  if (cachedDoc) {
    cachedDoc.lines = lines;
    cachedDoc.baseRuns = null;
  }
  return lines;
}

function buildBaseRuns(lines, vw, lineH) {
  const baseRuns = [];

  const root = Yoga.Node.create();
  root.setFlexDirection(Yoga.FLEX_DIRECTION_COLUMN);
  root.setAlignItems(Yoga.ALIGN_FLEX_START);
  root.setWidth(vw);
  root.setPadding(Yoga.EDGE_LEFT, LEFT_PAD);
  root.setPadding(Yoga.EDGE_TOP, TOP_PAD);

  const nodes = [];
  for (let i = 0; i < lines.length; i++) {
    const n = Yoga.Node.create();
    n.setHeight(lineH);
    n.setMinHeight(lineH);
    n.setWidth(Math.max(1, vw - (LEFT_PAD * 2)));
    root.insertChild(n, i);
    nodes.push(n);
  }

  root.calculateLayout(vw, NaN, Yoga.DIRECTION_LTR);

  for (let i = 0; i < lines.length; i++) {
    const y = Math.round(Number(nodes[i].getComputedTop() || 0)) + 2;
    const x = Math.round(Number(nodes[i].getComputedLeft() || 0));
    const t = String(lines[i] || '');
    if (!t) continue;
    baseRuns.push(x, y, t);
  }

  const contentH = Math.max(1, Math.round(Number(root.getComputedHeight() || 0)) + 2);

  root.freeRecursive();
  return { baseRuns, contentH };
}

function ensureDoc(vw) {
  const lines = currentLines();
  const lineH = Math.max(14, Number(LINE_H || 16));
  const nextKey = `${cachedHtml.length}:${vw}:${lineH}`;
  if (cachedDoc && cacheKey === nextKey && Array.isArray(cachedDoc.baseRuns)) return cachedDoc;

  const laid = buildBaseRuns(lines, vw, lineH);
  cachedDoc = {
    lines,
    lineH,
    viewportW: vw,
    baseRuns: laid.baseRuns,
    contentH: Math.max(1, Number(laid.contentH || 1)),
  };
  cacheKey = nextKey;
  return cachedDoc;
}

function paint() {
  const { vw, vh } = computeViewport();
  rebuildLyonRuns(vw, vh);
  const doc = ensureDoc(vw);
  const maxScroll = Math.max(0, Math.round(Number(doc.contentH || vh) - vh));
  if (scrollY > maxScroll) scrollY = maxScroll;

  const textRuns = [];
  for (let i = 0; i + 2 < doc.baseRuns.length; i += 3) {
    const x = Number(doc.baseRuns[i + 0] || 0);
    const y0 = Number(doc.baseRuns[i + 1] || 0);
    const t = String(doc.baseRuns[i + 2] || '');
    if (!t) continue;
    const y = Math.round(y0 - scrollY);
    if (y < -doc.lineH || y > vh) continue;
    textRuns.push(x, y, t);
  }

  fpsOverlay.appendRuns(textRuns, vw);

  if (typeof G.__trueosDrawLayoutRects === 'function') {
    G.__trueosDrawLayoutRects([], vw, vh, textRuns, lyonRoundRectRuns);
  }
  return true;
}

function relayoutAndPaint() {
  cacheKey = '';
  return paint();
}

function onWheelDelta(deltaY) {
  const { vw, vh } = computeViewport();
  const doc = ensureDoc(vw);
  const maxScroll = Math.max(0, Math.round(Number(doc.contentH || vh) - vh));
  if (maxScroll <= 0) return false;
  const next = Math.max(0, Math.min(maxScroll, Math.round(scrollY + Number(deltaY || 0))));
  if (next === scrollY) return false;
  scrollY = next;
  paint();
  return true;
}

function pumpCursorEvents() {
  const fn = G.__trueosReadCursorEventsSince;
  if (typeof fn !== 'function') return 0;

  let packed = null;
  try {
    packed = fn(Number(cursorReadSeq || 0));
  } catch (_) {
    return 0;
  }
  if (!Array.isArray(packed) || packed.length < 3) return 0;

  const nextSeq = Number(packed[0] || cursorReadSeq || 0);
  const wrote = Math.max(0, Number(packed[2] || 0) | 0);
  let updated = 0;
  let p = 3;
  for (let i = 0; i < wrote; i++) {
    if (p + 5 >= packed.length) break;
    const wheel = Number(packed[p + 4] || 0) | 0;
    if (wheel !== 0) {
      const dy = Number(wheel) * -WHEEL_STEP_PX;
      if (onWheelDelta(dy)) updated += 1;
    }
    p += 6;
  }
  cursorReadSeq = nextSeq;
  return updated;
}

function startWheelPump() {
  if (typeof G.setInterval === 'function') {
    try {
      G.setInterval(pumpCursorEvents, 16);
      return;
    } catch (_) {}
  }
  if (typeof G.requestAnimationFrame === 'function') {
    const step = () => {
      pumpCursorEvents();
      try { G.requestAnimationFrame(step); } catch (_) {}
    };
    try { G.requestAnimationFrame(step); } catch (_) {}
  }
}

G.__trueosBrowser = {
  relayoutAndPaint,
  setScroll(y) {
    scrollY = Math.max(0, Math.round(Number(y || 0)));
    paint();
  },
};

if (typeof (globalThis.window || globalThis).addEventListener === 'function') {
  (globalThis.window || globalThis).addEventListener('resize', relayoutAndPaint);
}

relayoutAndPaint();
startWheelPump();