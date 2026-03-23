import * as parse5 from 'parse5';
import Yoga from 'yoga-layout';
import { extractCssSection, resolveNodeStyle } from './css.mjs';
import { BLOCK_TAGS, TEXT_LEVEL_SEMANTICS_TAGS } from './htmlDefaults.mjs';
import { LEFT_PAD, TOP_PAD, LINE_H, FONT_PX } from './theme.mjs';

const INDENT_PX = 12;
const OMIT_TAGS = new Set(['html', 'body', 'script', 'style', 'meta', 'link', 'li']);

function collapseWhitespace(s) {
  return String(s || '').replace(/\s+/g, ' ').trim();
}

function isElement(node) {
  return !!node && typeof node === 'object' && typeof node.tagName === 'string';
}

function isTextNode(node) {
  return !!node && typeof node === 'object' && node.nodeName === '#text' && typeof node.value === 'string';
}

function getNodeAttr(node, name) {
  const attrs = Array.isArray(node && node.attrs) ? node.attrs : [];
  for (let i = 0; i < attrs.length; i += 1) {
    const attr = attrs[i];
    if (!attr || typeof attr.name !== 'string') continue;
    if (attr.name === name) return String(attr.value || '');
  }
  return '';
}

function fail(options, code, message, details = null, cause = null) {
  if (options && typeof options.raiseBrowserError === 'function') {
    options.raiseBrowserError(code, message, details, cause);
  }
  if (cause) throw cause;
  throw new Error(message);
}

function describe(options, err) {
  if (options && typeof options.describeError === 'function') {
    return options.describeError(err);
  }
  if (err && typeof err.message === 'string' && err.message.trim()) return err.message.trim();
  return String(err || 'unknown error').trim() || 'unknown error';
}

function pushRow(rows, text, depth, kind = 'text', style = null, meta = null) {
  const t = collapseWhitespace(text);
  if (!t) return;
  const row = {
    depth: Math.max(0, Number(depth || 0) | 0),
    text: t,
    kind: String(kind || 'text'),
    style,
  };
  if (meta && typeof meta === 'object') {
    if (typeof meta.path === 'string' && meta.path) row.path = meta.path;
    if (typeof meta.targetPath === 'string' && meta.targetPath) row.targetPath = meta.targetPath;
    if (typeof meta.targetTag === 'string' && meta.targetTag) row.targetTag = meta.targetTag;
    if (typeof meta.href === 'string' && meta.href) row.href = meta.href;
  }
  rows.push(row);
}

function pushImageRow(rows, depth, widthPx, heightPx, style = null, meta = null, text = 'img') {
  const row = {
    depth: Math.max(0, Number(depth || 0) | 0),
    text: collapseWhitespace(text) || 'img',
    kind: 'image',
    style,
    widthPx: Math.max(1, Math.round(Number(widthPx || 0) || 1)),
    heightPx: Math.max(1, Math.round(Number(heightPx || 0) || 1)),
  };
  if (meta && typeof meta === 'object') {
    if (typeof meta.path === 'string' && meta.path) row.path = meta.path;
    if (typeof meta.src === 'string' && meta.src) row.src = meta.src;
  }
  rows.push(row);
}

function pushHrRow(rows, depth, style = null, meta = null) {
  const row = {
    depth: Math.max(0, Number(depth || 0) | 0),
    text: '',
    kind: 'hr',
    style,
    heightPx: 1 + (TOP_PAD * 2),
    ruleHeightPx: 1,
  };
  if (meta && typeof meta === 'object') {
    if (typeof meta.path === 'string' && meta.path) row.path = meta.path;
  }
  rows.push(row);
}

function parsePositiveAttrPx(node, name, fallback = 0) {
  const raw = String(getNodeAttr(node, name) || '').trim();
  if (!raw) return Math.max(0, Number(fallback || 0) || 0);
  const value = Number.parseFloat(raw);
  if (!Number.isFinite(value) || value <= 0) {
    return Math.max(0, Number(fallback || 0) || 0);
  }
  return value;
}

function parseSvgViewBoxDimensions(node) {
  const raw = String(getNodeAttr(node, 'viewBox') || '').trim();
  if (!raw) return { width: 0, height: 0 };
  const parts = raw.split(/[\s,]+/).map((part) => Number(part));
  if (parts.length < 4 || !parts.every((value) => Number.isFinite(value))) {
    return { width: 0, height: 0 };
  }
  return {
    width: Math.max(0, Math.round(parts[2] || 0)),
    height: Math.max(0, Math.round(parts[3] || 0)),
  };
}

function inlineSvgNodeToDataUrl(node) {
  if (!node || typeof node !== 'object') return '';
  try {
    const svg = typeof parse5.serializeOuter === 'function'
      ? String(parse5.serializeOuter(node) || '')
      : String(parse5.serialize(node) || '');
    const trimmed = svg.trim();
    if (!trimmed) return '';
    return `data:image/svg+xml;utf8,${encodeURIComponent(trimmed)}`;
  } catch (_) {
    return '';
  }
}

function shouldOmitElement(tagName) {
  return OMIT_TAGS.has(String(tagName || '').toLowerCase());
}

function shouldRenderTagLines(tagName) {
  const tag = String(tagName || '').toLowerCase();
  if (tag === 'p') return false;
  if (TEXT_LEVEL_SEMANTICS_TAGS.includes(tag)) return false;
  return BLOCK_TAGS.has(tag);
}

function detectLabelMarkerKind(node) {
  const kids = Array.isArray(node && node.childNodes) ? node.childNodes : [];
  for (let i = 0; i < kids.length; i += 1) {
    const kid = kids[i];
    if (!isElement(kid) || String(kid.tagName || '').toLowerCase() !== 'input') continue;
    const type = String(getNodeAttr(kid, 'type') || '').trim().toLowerCase();
    if (type === 'checkbox') return 'checkbox-text';
    if (type === 'radio') return 'radio-text';
  }
  return 'text';
}

function collectRows(node, depth, rows, cssSection, parentMeta = null, path = 'root', ancestors = []) {
  if (!node || typeof node !== 'object') return;

  if (isTextNode(node)) {
    const parent = String(parentMeta && parentMeta.tag || '').toLowerCase();
    const kind = parent === 'title'
      ? 'title-text'
      : (parent === 'summary'
        ? 'summary-text'
        : (parent === 'label' && parentMeta && typeof parentMeta.markerKind === 'string'
          ? parentMeta.markerKind
          : (parent === 'li'
            ? 'li-text'
            : (parent === 'a'
              ? 'link-text'
              : (parent === 'button' ? 'button-text' : 'text')))));
    pushRow(rows, node.value, depth, kind, parentMeta && parentMeta.style ? parentMeta.style : null, {
      path: String(path || 'root'),
      targetPath: parent === 'a' || parent === 'button' ? String(parentMeta && parentMeta.path || '') : '',
      targetTag: parent,
      href: parent === 'a' ? String(parentMeta && parentMeta.href || '') : '',
    });
    return;
  }

  if (isElement(node)) {
    const tag = String(node.tagName || '').toLowerCase();
    const style = resolveNodeStyle(node, path, cssSection, ancestors, parentMeta && parentMeta.style ? parentMeta.style : null);
    if (tag === 'hr') {
      pushHrRow(rows, depth, style, { path: String(path || 'root') });
      return;
    }
    if (tag === 'img') {
      const widthPx = parsePositiveAttrPx(node, 'width', 160);
      const heightPx = parsePositiveAttrPx(node, 'height', widthPx > 0 ? widthPx : 120);
      pushImageRow(
        rows,
        depth,
        widthPx,
        heightPx,
        style,
        { path: String(path || 'root'), src: String(getNodeAttr(node, 'src') || '') },
        String(getNodeAttr(node, 'alt') || 'img'),
      );
      return;
    }
    if (tag === 'svg') {
      const viewBox = parseSvgViewBoxDimensions(node);
      const widthPx = parsePositiveAttrPx(node, 'width', viewBox.width > 0 ? viewBox.width : 160);
      const heightPx = parsePositiveAttrPx(node, 'height', viewBox.height > 0 ? viewBox.height : (widthPx > 0 ? widthPx : 120));
      pushImageRow(
        rows,
        depth,
        widthPx,
        heightPx,
        style,
        { path: String(path || 'root'), src: inlineSvgNodeToDataUrl(node) },
        String(getNodeAttr(node, 'aria-label') || getNodeAttr(node, 'title') || 'svg'),
      );
      return;
    }
    const renderTagLines = !shouldOmitElement(tag) && shouldRenderTagLines(tag);
    const kids = Array.isArray(node.childNodes) ? node.childNodes : [];
    const nextAncestors = ancestors.concat([{ node, path }]);
    const nextParentMeta = {
      tag,
      path: String(path || 'root'),
      style,
      href: tag === 'a' ? String(getNodeAttr(node, 'href') || '') : '',
      markerKind: tag === 'label' ? detectLabelMarkerKind(node) : 'text',
    };
    for (let i = 0; i < kids.length; i += 1) {
      collectRows(kids[i], renderTagLines ? depth + 1 : depth, rows, cssSection, nextParentMeta, `${path}.${i}`, nextAncestors);
    }
    return;
  }

  const kids = Array.isArray(node.childNodes) ? node.childNodes : [];
  for (let i = 0; i < kids.length; i += 1) {
    collectRows(kids[i], depth, rows, cssSection, parentMeta, `${path}.${i}`, ancestors);
  }
}

function readNodeText(node) {
  if (!node || typeof node !== 'object') return '';
  if (isTextNode(node)) return String(node.value || '');
  const kids = Array.isArray(node.childNodes) ? node.childNodes : [];
  let out = '';
  for (let i = 0; i < kids.length; i += 1) {
    out += readNodeText(kids[i]);
  }
  return out;
}

function estimateTextWidthPx(text, fontSizePx = FONT_PX, host = null) {
  const value = String(text || '');
  const fontPx = Math.max(1, Number(fontSizePx || FONT_PX) || FONT_PX);
  const baseFontPx = Math.max(1, Number(host && host.__trueosBrowserDefaultFontPx || FONT_PX) || FONT_PX);
  const widthTable = Array.isArray(host && host.__trueosBrowserTextWidthByChar)
    ? host.__trueosBrowserTextWidthByChar
    : null;
  const scale = fontPx / baseFontPx;
  if (!value) return Math.max(4, Math.round(baseFontPx * 0.5 * scale));
  if (!widthTable || widthTable.length < 256) {
    const glyphPx = Math.max(6, Math.round(fontPx * 0.56));
    return Math.max(glyphPx, value.length * glyphPx);
  }
  let total = 0;
  for (let i = 0; i < value.length; i += 1) {
    const code = value.charCodeAt(i);
    if (code === 10 || code === 13) continue;
    if (code >= 0 && code < 256) total += Number(widthTable[code] || 0);
    else total += Number(widthTable[63] || 0);
  }
  return Math.max(1, Math.round(total * scale));
}

function collectThemeLayoutInteractives(node, cssSection, parentStyle = null, path = 'root', ancestors = [], out = []) {
  if (!node || typeof node !== 'object') return out;
  if (isElement(node)) {
    const tag = String(node.tagName || '').toLowerCase();
    const style = resolveNodeStyle(node, path, cssSection, ancestors, parentStyle);
    const nextAncestors = ancestors.concat([{ node, path }]);
    const href = String(getNodeAttr(node, 'href') || '');
    if (tag === 'button' || (tag === 'a' && href)) {
      const caption = collapseWhitespace(readNodeText(node)) || String(getNodeAttr(node, 'value') || href || tag);
      out.push({
        path,
        tag,
        kind: tag === 'button' ? 'button' : 'link',
        caption,
        href,
        style,
      });
    }
    const kids = Array.isArray(node.childNodes) ? node.childNodes : [];
    for (let i = 0; i < kids.length; i += 1) {
      collectThemeLayoutInteractives(kids[i], cssSection, style, `${path}.${i}`, nextAncestors, out);
    }
    return out;
  }
  const kids = Array.isArray(node.childNodes) ? node.childNodes : [];
  for (let i = 0; i < kids.length; i += 1) {
    collectThemeLayoutInteractives(kids[i], cssSection, parentStyle, `${path}.${i}`, ancestors, out);
  }
  return out;
}

function buildInteractiveRowRects(rows, rowX, rowY, host) {
  const out = Object.create(null);
  for (let i = 0; i < rows.length; i += 1) {
    const row = rows[i];
    const targetPath = typeof row.targetPath === 'string' ? row.targetPath : '';
    if (!targetPath) continue;
    const nextRect = {
      x: Math.round(Number(rowX[i] ?? LEFT_PAD)),
      y: Math.round(Number(rowY[i] ?? (i * LINE_H))),
      width: Math.max(1, estimateTextWidthPx(String(row.text || ''), FONT_PX, host)),
      height: LINE_H,
    };
    const prev = out[targetPath];
    if (!prev) {
      out[targetPath] = nextRect;
      continue;
    }
    const minX = Math.min(prev.x, nextRect.x);
    const minY = Math.min(prev.y, nextRect.y);
    const maxX = Math.max(prev.x + prev.width, nextRect.x + nextRect.width);
    const maxY = Math.max(prev.y + prev.height, nextRect.y + nextRect.height);
    out[targetPath] = {
      x: minX,
      y: minY,
      width: Math.max(1, maxX - minX),
      height: Math.max(1, maxY - minY),
    };
  }
  return out;
}

function alignThemeLayoutToRows(themeLayout, rows, rowX, rowY, host) {
  if (!themeLayout || typeof themeLayout !== 'object') return themeLayout;
  const interactives = Array.isArray(themeLayout.interactives) ? themeLayout.interactives : [];
  if (interactives.length <= 0) return themeLayout;
  const rowRects = buildInteractiveRowRects(rows, rowX, rowY, host);
  const nextInteractives = [];
  const nextButtons = [];
  const nextByPath = Object.create(null);
  for (let i = 0; i < interactives.length; i += 1) {
    const entry = interactives[i];
    const rect = rowRects[String(entry && entry.path || '')] || null;
    const next = rect ? { ...entry, ...rect } : { ...entry };
    nextInteractives.push(next);
    if (next.kind === 'button') nextButtons.push(next);
    if (typeof next.path === 'string' && next.path) nextByPath[next.path] = next;
  }
  return {
    ...themeLayout,
    interactives: nextInteractives,
    buttons: nextButtons,
    byPath: nextByPath,
  };
}

function applyThemeLayoutYoga(entries, vw, context, host) {
  let root = null;
  try {
    root = Yoga.Node.create();
    root.setFlexDirection(Yoga.FLEX_DIRECTION_COLUMN);
    root.setAlignItems(Yoga.ALIGN_FLEX_START);
    root.setWidth(vw);
    root.setPadding(Yoga.EDGE_LEFT, LEFT_PAD);
    root.setPadding(Yoga.EDGE_TOP, 0);

    const nodes = [];
    for (let i = 0; i < entries.length; i += 1) {
      const entry = entries[i];
      const style = entry && entry.style && typeof entry.style === 'object' ? entry.style : {};
      const node = Yoga.Node.create();
      node.setMargin(Yoga.EDGE_LEFT, Number(style.marginLeftPx || 0));
      node.setMargin(Yoga.EDGE_TOP, Number(style.marginTopPx || 0));
      node.setMargin(Yoga.EDGE_RIGHT, Number(style.marginRightPx || 0));
      node.setMargin(Yoga.EDGE_BOTTOM, Number(style.marginBottomPx || 0));
      node.setMeasureFunc(() => {
        const fontPx = Math.max(1, Number(style.fontSizePx || FONT_PX) || FONT_PX);
        const paddingLeft = Math.max(0, Number(style.paddingLeftPx || 0));
        const paddingTop = Math.max(0, Number(style.paddingTopPx || 0));
        const paddingRight = Math.max(0, Number(style.paddingRightPx || 0));
        const paddingBottom = Math.max(0, Number(style.paddingBottomPx || 0));
        const textW = estimateTextWidthPx(entry && entry.caption ? entry.caption : entry.tag, fontPx, host);
        const lineH = Math.max(LINE_H, Math.ceil(fontPx * 1.35));
        return {
          width: textW + paddingLeft + paddingRight,
          height: lineH + paddingTop + paddingBottom,
        };
      });
      root.insertChild(node, i);
      nodes.push(node);
    }

    root.calculateLayout(vw, NaN, Yoga.DIRECTION_LTR);

    const interactives = [];
    const buttons = [];
    const byPath = Object.create(null);
    for (let i = 0; i < nodes.length; i += 1) {
      const entry = entries[i];
      const rect = {
        x: Math.round(Number(nodes[i].getComputedLeft() || 0)),
        y: Math.round(Number(nodes[i].getComputedTop() || 0)),
        width: Math.max(1, Math.round(Number(nodes[i].getComputedWidth() || 0))),
        height: Math.max(1, Math.round(Number(nodes[i].getComputedHeight() || 0))),
      };
      const layout = {
        ...rect,
        path: entry.path,
        tag: entry.tag,
        kind: entry.kind,
        caption: entry.caption,
        href: entry.href,
      };
      interactives.push(layout);
      if (entry.kind === 'button') buttons.push(layout);
      byPath[entry.path] = layout;
    }

    return {
      interactives,
      buttons,
      byPath,
      contentW: Math.max(1, Math.round(Number(root.getComputedWidth() || vw || 1))),
      contentH: Math.max(1, Math.round(Number(root.getComputedHeight() || 0))),
      viewportWidth: vw,
      context,
    };
  } finally {
    if (root) {
      try { root.freeRecursive(); } catch (_) {}
    }
  }
}

function buildThemeLayout(pageModel, vw, context, host) {
  const entries = collectThemeLayoutInteractives(pageModel.dom, pageModel.css, null, 'root', []);
  if (entries.length <= 0) {
    return {
      interactives: [],
      buttons: [],
      byPath: Object.create(null),
      contentW: vw,
      contentH: 0,
      viewportWidth: vw,
      context,
    };
  }
  return alignThemeLayoutToRows(
    applyThemeLayoutYoga(entries, vw, context, host),
    pageModel.rows,
    pageModel.rowX,
    pageModel.rowY,
    host,
  );
}

function applyYoga(rows, vw, context, host, options) {
  let root = null;
  try {
    root = Yoga.Node.create();
    root.setFlexDirection(Yoga.FLEX_DIRECTION_COLUMN);
    root.setAlignItems(Yoga.ALIGN_FLEX_START);
    root.setWidth(vw);
    root.setPadding(Yoga.EDGE_LEFT, LEFT_PAD);
    root.setPadding(Yoga.EDGE_TOP, 0);

    const nodes = [];
    const rowX = [];
    const rowY = [];
    for (let i = 0; i < rows.length; i += 1) {
      const r = rows[i];
      const indent = r.depth * INDENT_PX;
      const n = Yoga.Node.create();
      if (r.kind === 'image') {
        const imageW = Math.max(1, Math.round(Number(r.widthPx || 0) || 1));
        const imageH = Math.max(1, Math.round(Number(r.heightPx || 0) || 1));
        const maxW = Math.max(1, vw - (LEFT_PAD * 2) - indent);
        n.setWidth(Math.min(imageW, maxW));
        n.setHeight(imageH);
        n.setMinHeight(imageH);
        n.setMargin(Yoga.EDGE_LEFT, indent);
      } else if (r.kind === 'hr') {
        const hrH = Math.max(1, Math.round(Number(r.heightPx || 0) || 1));
        n.setWidth(Math.max(1, vw - (LEFT_PAD * 2) - indent));
        n.setHeight(hrH);
        n.setMinHeight(hrH);
        n.setMargin(Yoga.EDGE_LEFT, indent);
      } else if (r.kind === 'title-text') {
        n.setHeight(LINE_H);
        n.setMinHeight(LINE_H);
        const textW = Math.max(1, estimateTextWidthPx(String(r.text || ''), FONT_PX, host));
        const innerRowW = Math.max(1, vw - (LEFT_PAD * 2));
        const centeredLeft = Math.max(0, Math.floor((innerRowW - textW) * 0.5));
        n.setWidth(textW);
        n.setMargin(Yoga.EDGE_LEFT, centeredLeft);
      } else {
        n.setHeight(LINE_H);
        n.setMinHeight(LINE_H);
        n.setWidth(Math.max(1, vw - (LEFT_PAD * 2) - indent));
        n.setMargin(Yoga.EDGE_LEFT, indent);
      }
      root.insertChild(n, i);
      nodes.push(n);
    }

    root.calculateLayout(vw, NaN, Yoga.DIRECTION_LTR);
    for (let i = 0; i < nodes.length; i += 1) {
      rowX.push(Math.round(Number(nodes[i].getComputedLeft() || 0)));
      rowY.push(Math.round(Number(nodes[i].getComputedTop() || 0)));
    }

    const contentH = Math.max(1, Math.round(Number(root.getComputedHeight() || 0)));
    let contentW = Math.max(1, Math.round(Number(vw || 1)));
    for (let i = 0; i < nodes.length; i += 1) {
      const right = Math.round(Number(nodes[i].getComputedLeft() || 0) + Number(nodes[i].getComputedWidth() || 0));
      if (right > contentW) contentW = right;
    }
    return { rowX, rowY, contentW, contentH };
  } catch (err) {
    fail(
      options,
      'TRUEOS_BROWSER_LAYOUT_FAILED',
      `Yoga layout failed while building ${context}`,
      { context, reason: describe(options, err), viewportWidth: vw, rowCount: Array.isArray(rows) ? rows.length : 0 },
      err,
    );
  } finally {
    if (root) {
      try { root.freeRecursive(); } catch (_) {}
    }
  }
}

export function buildWorldPageModel(parsed, options = {}) {
  const context = String(options.context || 'document');
  const doc = parsed && typeof parsed === 'object' ? parsed : parse5.parse('');
  let cssSection = null;
  try {
    cssSection = typeof extractCssSection === 'function' ? extractCssSection(doc) : null;
  } catch (err) {
    fail(
      options,
      'TRUEOS_BROWSER_CSS_PARSE_FAILED',
      `CSS extraction failed while building ${context}`,
      { context, reason: describe(options, err) },
      err,
    );
  }

  const rows = [];
  try {
    collectRows(doc, 0, rows, cssSection, null, 'root', []);
  } catch (err) {
    fail(
      options,
      'TRUEOS_BROWSER_DOM_BUILD_FAILED',
      `DOM row collection failed while building ${context}`,
      { context, reason: describe(options, err) },
      err,
    );
  }

  return {
    kind: 'world-page-model',
    context,
    dom: doc,
    css: cssSection,
    rows,
  };
}

export function realizePageModel(pageModel, vw, options = {}) {
  const context = String(options.context || pageModel && pageModel.context || 'document');
  const host = options.host || null;
  const layout = applyYoga(pageModel.rows, vw, context, host, options);
  const shapedPageModel = {
    ...pageModel,
    rowX: layout.rowX,
    rowY: layout.rowY,
  };
  let themeLayout = null;
  try {
    themeLayout = buildThemeLayout(shapedPageModel, vw, context, host);
  } catch (err) {
    fail(
      options,
      'TRUEOS_BROWSER_THEME_LAYOUT_FAILED',
      `Theme layout build failed while building ${context}`,
      { context, reason: describe(options, err) },
      err,
    );
  }
  return {
    kind: 'realized-page-layout',
    context,
    viewportWidth: vw,
    rowX: layout.rowX,
    rowY: layout.rowY,
    contentW: layout.contentW,
    contentH: layout.contentH,
    themeLayout,
  };
}
