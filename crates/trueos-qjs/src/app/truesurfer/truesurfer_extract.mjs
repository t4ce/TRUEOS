import { cssColorToRgbInt, resolveInlineStyle } from './css.mjs';

const TRUESURFER_SUBSET_PROFILE = Object.freeze({
  includeHead: true,
  includeTitle: true,
  includeBody: true,
  includeStyles: true,
  includeScripts: true,
  includeBodyHierarchy: true,
  maxBodyHierarchyRoots: 10,
  maxBodyHierarchyChildrenPerNode: 5,
  maxBodyHierarchyDepth: 4,
  bodyTags: Object.freeze([
    'div', 'p', 'span', 'a', 'ul', 'ol', 'li', 'table', 'thead', 'tbody',
    'tr', 'td', 'th', 'section', 'article', 'header', 'footer', 'main', 'nav',
    'img', 'form', 'input', 'button', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6',
  ]),
});

const COMMON_BODY_TAGS = new Set(TRUESURFER_SUBSET_PROFILE.bodyTags);
const BODY_HIERARCHY_ROOT_LIMIT = TRUESURFER_SUBSET_PROFILE.maxBodyHierarchyRoots;
const BODY_HIERARCHY_CHILD_LIMIT = TRUESURFER_SUBSET_PROFILE.maxBodyHierarchyChildrenPerNode;
const BODY_HIERARCHY_DEPTH_LIMIT = TRUESURFER_SUBSET_PROFILE.maxBodyHierarchyDepth;
const GADGET_SNAPSHOT_LIMIT = 48;
const UI3_SCENE_NODE_LIMIT = 96;
const UI3_SCENE_VIEWPORT_WIDTH = 1920;
const UI3_SCENE_VIEWPORT_HEIGHT = 1080;
const DEFAULT_GADGET_FONT_PX = 14;
const DEFAULT_GADGET_LINE_HEIGHT_PX = 20;
const LINK_GADGET_PAD_X = 10;
const LINK_GADGET_PAD_Y = 6;
const LINK_GADGET_TEXT_COLOR_RGB = 0x1d4ed8;
const BODY_STYLE_TAG_RE = /<style\b[^>]*>([\s\S]*?)<\/style>/gi;
const DEFAULT_HEAD_SHELL = '<head></head>';
const DEFAULT_BODY_SHELL = '<body></body>';
const COMMON_BODY_TAGS_FALLBACK = new Set([
  'div', 'p', 'span', 'a', 'ul', 'ol', 'li', 'table', 'thead', 'tbody',
  'tr', 'td', 'th', 'section', 'article', 'header', 'footer', 'main', 'nav',
  'img', 'form', 'input', 'button', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6',
]);
const VOID_HTML_TAGS = new Set([
  'area', 'base', 'br', 'col', 'embed', 'hr', 'img', 'input', 'link', 'meta',
  'param', 'source', 'track', 'wbr',
]);

function emptyStyleIndex() {
  return {
    styleTable: [],
    nodeStyleRefs: [],
    styleSlotCount: 0,
    nodeRefCount: 0,
    inlineStyleCount: 0,
    stylesheetCount: 0,
    ruleCount: 0,
    elementCount: 0,
  };
}

function safeString(value) {
  if (typeof value === 'string') {
    return value;
  }
  if (value === null || value === undefined) {
    return '';
  }
  return String(value);
}

function escapeHtmlText(text) {
  return safeString(text)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;');
}

function decodeBasicEntities(text) {
  return safeString(text)
    .replace(/&nbsp;/gi, ' ')
    .replace(/&quot;/gi, '"')
    .replace(/&#39;|&apos;/gi, "'")
    .replace(/&lt;/gi, '<')
    .replace(/&gt;/gi, '>')
    .replace(/&amp;/gi, '&');
}

function collapseWhitespace(text) {
  return safeString(text).replace(/\s+/g, ' ').trim();
}

function stripTags(text) {
  return safeString(text).replace(/<[^>]+>/g, '');
}

function extractAttrValue(tagHtml, attrName) {
  const quoted = new RegExp(`${attrName}\\s*=\\s*(["'])([\\s\\S]*?)\\1`, 'i');
  const quotedMatch = quoted.exec(tagHtml);
  if (quotedMatch) {
    return safeString(quotedMatch[2]);
  }
  const bare = new RegExp(`${attrName}\\s*=\\s*([^\\s>]+)`, 'i');
  const bareMatch = bare.exec(tagHtml);
  return bareMatch ? safeString(bareMatch[1]) : '';
}

function findTagOpen(sourceLower, tagName, startIndex = 0) {
  const needle = `<${tagName}`;
  let searchIndex = startIndex;
  while (searchIndex >= 0 && searchIndex < sourceLower.length) {
    const idx = sourceLower.indexOf(needle, searchIndex);
    if (idx < 0) {
      return -1;
    }
    const nextCode = sourceLower.charCodeAt(idx + needle.length) || 0;
    const boundary = !((nextCode >= 48 && nextCode <= 57) || (nextCode >= 97 && nextCode <= 122) || nextCode === 45 || nextCode === 58);
    if (boundary) {
      return idx;
    }
    searchIndex = idx + needle.length;
  }
  return -1;
}

function findTagClose(source, openStart) {
  let inQuote = '';
  for (let index = openStart; index < source.length; index += 1) {
    const ch = source[index];
    if (inQuote) {
      if (ch === inQuote) {
        inQuote = '';
      }
      continue;
    }
    if (ch === '"' || ch === "'") {
      inQuote = ch;
      continue;
    }
    if (ch === '>') {
      return index;
    }
  }
  return -1;
}

function extractTagBlock(source, tagName) {
  const html = safeString(source);
  const lower = html.toLowerCase();
  const openStart = findTagOpen(lower, tagName);
  if (openStart < 0) {
    return null;
  }
  const openEnd = findTagClose(html, openStart);
  if (openEnd < 0) {
    return null;
  }
  const closeNeedle = `</${tagName}>`;
  const closeStart = lower.indexOf(closeNeedle, openEnd + 1);
  if (closeStart < 0) {
    return {
      openTag: html.slice(openStart, openEnd + 1),
      inner: '',
      outer: html.slice(openStart, openEnd + 1),
    };
  }
  return {
    openTag: html.slice(openStart, openEnd + 1),
    inner: html.slice(openEnd + 1, closeStart),
    outer: html.slice(openStart, closeStart + closeNeedle.length),
  };
}

function extractTitleFromHead(headHtml) {
  const match = /<title\b[^>]*>([\s\S]*?)<\/title>/i.exec(safeString(headHtml));
  if (!match) {
    return '';
  }
  return collapseWhitespace(decodeBasicEntities(stripTags(match[1])));
}

function extractFaviconHref(sourceHtml) {
  const html = safeString(sourceHtml);
  if (!html) {
    return '';
  }
  const linkTagRe = /<link\b[^>]*>/gi;
  let match;
  while ((match = linkTagRe.exec(html))) {
    const tagHtml = safeString(match[0]);
    const rel = safeString(extractAttrValue(tagHtml, 'rel'))
      .toLowerCase()
      .split(/\s+/)
      .map((part) => part.trim())
      .filter(Boolean);
    if (
      !rel.includes('icon')
      && !rel.includes('shortcut')
      && !rel.includes('apple-touch-icon')
    ) {
      continue;
    }
    const href = collapseWhitespace(decodeBasicEntities(extractAttrValue(tagHtml, 'href')));
    if (href) {
      return href;
    }
  }
  return '';
}

function stripCommentAndRawTextNoise(html) {
  return safeString(html)
    .replace(/<!--[\s\S]*?-->/g, '')
    .replace(/<script\b[^>]*>[\s\S]*?<\/script>/gi, '')
    .replace(/<style\b[^>]*>[\s\S]*?<\/style>/gi, '');
}

function extractBodyBackgroundColorFromStyles(sourceHtml) {
  const html = safeString(sourceHtml);
  if (!html) {
    return 0;
  }
  let backgroundColorRgb = 0;
  let match;
  while ((match = BODY_STYLE_TAG_RE.exec(html))) {
    const cssText = safeString(match[1]);
    const bodyRuleRe = /(^|[^a-z0-9_-])body\s*\{([\s\S]*?)\}/gi;
    let bodyRuleMatch;
    while ((bodyRuleMatch = bodyRuleRe.exec(cssText))) {
      const declarations = safeString(bodyRuleMatch[2]);
      const backgroundDeclRe = /(background-color|background)\s*:\s*([^;]+)\s*;?/gi;
      let backgroundDeclMatch;
      while ((backgroundDeclMatch = backgroundDeclRe.exec(declarations))) {
        const nextRgb = cssColorToRgbInt(backgroundDeclMatch[2]);
        if (nextRgb !== 0) {
          backgroundColorRgb = nextRgb;
        }
      }
    }
  }
  return backgroundColorRgb;
}

function extractBodyBackgroundColorRgb(sourceHtml, bodyOpenTag) {
  let backgroundColorRgb = extractBodyBackgroundColorFromStyles(sourceHtml);
  const inlineBodyStyle = resolveInlineStyle('body', 'body', extractAttrValue(bodyOpenTag, 'style'), null);
  const inlineRgb = cssColorToRgbInt(inlineBodyStyle && inlineBodyStyle.backgroundColor);
  if (inlineRgb !== 0) {
    backgroundColorRgb = inlineRgb;
  }
  return backgroundColorRgb;
}

function collectBodyHierarchy(bodyHtml, limit = BODY_HIERARCHY_ROOT_LIMIT) {
  const source = stripCommentAndRawTextNoise(bodyHtml);
  const tokenRe = /<\/?([a-zA-Z0-9:-]+)\b[^>]*>|([^<]+)/g;
  const stack = [{
    tag: 'body',
    emitted: true,
    hierarchyDepth: -1,
    childCount: 0,
  }];
  const out = [];
  let nextNodeId = 1;
  let match;

  while ((match = tokenRe.exec(source))) {
    const textChunk = match[2];
    if (textChunk != null) {
      const text = collapseWhitespace(decodeBasicEntities(textChunk));
      const parent = stack[stack.length - 1] || null;
      if (text && parent && parent.emitted && parent.hierarchyDepth >= 0) {
        out.push({ nodeId: nextNodeId, depth: parent.hierarchyDepth + 1, tag: '#text', text });
        nextNodeId += 1;
      }
      continue;
    }

    const fullTag = match[0];
    const tagName = safeString(match[1]).toLowerCase();
    const isClose = fullTag[1] === '/';
    const isSelfClosing = /\/\s*>$/.test(fullTag) || VOID_HTML_TAGS.has(tagName);

    if (isClose) {
      for (let index = stack.length - 1; index >= 1; index -= 1) {
        if (stack[index].tag === tagName) {
          stack.length = index;
          break;
        }
      }
      continue;
    }

    const parent = stack[stack.length - 1] || stack[0];
    const respectedTag = COMMON_BODY_TAGS.has(tagName) || COMMON_BODY_TAGS_FALLBACK.has(tagName);
    const nextHierarchyDepth = (parent && parent.emitted)
      ? parent.hierarchyDepth + 1
      : BODY_HIERARCHY_DEPTH_LIMIT + 1;
    const rootCandidate = nextHierarchyDepth === 0;
    const withinBreadthLimit = rootCandidate
      ? parent.childCount < limit
      : parent.childCount < BODY_HIERARCHY_CHILD_LIMIT;
    const withinDepthLimit = nextHierarchyDepth <= BODY_HIERARCHY_DEPTH_LIMIT;
    const emitNode = !!parent && parent.emitted && respectedTag && withinBreadthLimit && withinDepthLimit;

    if (emitNode) {
      out.push({
        nodeId: nextNodeId,
        depth: nextHierarchyDepth,
        tag: tagName,
        text: '',
        styleText: extractAttrValue(fullTag, 'style'),
        title: decodeBasicEntities(extractAttrValue(fullTag, 'title')),
        inputType: extractAttrValue(fullTag, 'type'),
        value: decodeBasicEntities(extractAttrValue(fullTag, 'value')),
      });
      nextNodeId += 1;
      parent.childCount += 1;
    }

    if (!isSelfClosing) {
      stack.push({
        tag: tagName,
        emitted: emitNode,
        hierarchyDepth: nextHierarchyDepth,
        childCount: 0,
      });
    }
  }

  return out;
}

function summarizeBodyHierarchy(bodyHierarchy) {
  if (!Array.isArray(bodyHierarchy) || bodyHierarchy.length === 0) {
    return 'body';
  }
  const parts = ['body'];
  for (const entry of bodyHierarchy) {
    if (entry.tag === '#text') {
      continue;
    }
    const depth = Math.max(0, Number(entry.depth) || 0);
    parts.push(`${'.'.repeat(depth)}${entry.tag}`);
  }
  return parts.join(' > ');
}

function estimateTextWidthPx(text, fontPx) {
  const value = collapseWhitespace(text);
  if (!value) {
    return Math.max(1, Math.round(fontPx * 0.56));
  }
  const glyphPx = Math.max(6, Math.round(Number(fontPx || 14) * 0.56));
  return Math.max(glyphPx, value.length * glyphPx);
}

function gadgetUsesInnerText(tag) {
  const name = safeString(tag).toLowerCase();
  return name === 'p' || name === 'span' || name === 'a' || name === 'button' || /^h[1-6]$/.test(name);
}

function gadgetIsLinkLike(tag) {
  return safeString(tag).toLowerCase() === 'a';
}

function collectGadgetInnerText(bodyHierarchy, startIndex, parentDepth) {
  const parts = [];
  const items = Array.isArray(bodyHierarchy) ? bodyHierarchy : [];
  for (let index = startIndex + 1; index < items.length; index += 1) {
    const entry = items[index] || {};
    const depth = Math.max(0, Number(entry.depth) || 0);
    if (depth <= parentDepth) {
      break;
    }
    if (collapseWhitespace(entry.tag) !== '#text') {
      continue;
    }
    const text = collapseWhitespace(entry.text);
    if (text) {
      parts.push(text);
    }
  }
  return collapseWhitespace(parts.join(' '));
}

function gadgetFontSizePx(tag) {
  const name = safeString(tag).toLowerCase();
  if (!/^h[1-6]$/.test(name)) {
    return DEFAULT_GADGET_FONT_PX;
  }
  const level = Number(name[1] || 1);
  return Math.max(DEFAULT_GADGET_FONT_PX, 24 - ((level - 1) * 2));
}

function gadgetLineHeightPx(tag) {
  const fontPx = gadgetFontSizePx(tag);
  return Math.max(DEFAULT_GADGET_LINE_HEIGHT_PX, fontPx + 4);
}

function gadgetDisplayText(tag, innerText) {
  const name = safeString(tag).toLowerCase();
  const text = collapseWhitespace(innerText);
  if (gadgetUsesInnerText(name) && text) {
    return text;
  }
  if (!name || name === '#text') {
    return text;
  }
  return `<${name}>`;
}

function gadgetIsButtonLike(entry) {
  const tag = collapseWhitespace(entry && entry.tag).toLowerCase();
  if (tag === 'button') {
    return true;
  }
  if (tag !== 'input') {
    return false;
  }
  const inputType = collapseWhitespace(entry && entry.inputType).toLowerCase();
  return inputType === 'button' || inputType === 'submit' || inputType === 'reset';
}

function fallbackInputButtonLabel(inputType) {
  const kind = collapseWhitespace(inputType).toLowerCase();
  if (kind === 'submit') return 'Submit';
  if (kind === 'reset') return 'Reset';
  return 'Button';
}

function gadgetTextForEntry(entry, innerText) {
  if (gadgetIsLinkLike(entry && entry.tag)) {
    const text = collapseWhitespace(innerText);
    if (text) {
      return text;
    }
    const title = collapseWhitespace(entry && entry.title);
    if (title) {
      return title;
    }
  }
  if (gadgetIsButtonLike(entry)) {
    const value = collapseWhitespace(entry && entry.value);
    return value || fallbackInputButtonLabel(entry && entry.inputType);
  }
  return gadgetDisplayText(entry && entry.tag, innerText);
}

function gadgetResolvedStyle(entry, index) {
  const tag = collapseWhitespace(entry && entry.tag).toLowerCase();
  return resolveInlineStyle(tag, `gadget.${index}`, entry && entry.styleText || '', null);
}

function gadgetTextColorRgb(tag, style) {
  if (gadgetIsLinkLike(tag)) {
    return LINK_GADGET_TEXT_COLOR_RGB;
  }
  return cssColorToRgbInt(style && style.color);
}

function buildGadgetSnapshot(bodyHierarchy, backgroundColorRgb = 0, limit = GADGET_SNAPSHOT_LIMIT) {
  const gadgets = [];
  const items = Array.isArray(bodyHierarchy) ? bodyHierarchy : [];
  let yCursor = 0;

  for (let index = 0; index < items.length && gadgets.length < limit; index += 1) {
    const entry = items[index] || {};
    const tag = collapseWhitespace(entry.tag).toLowerCase();
    if (!tag || tag === '#text') {
      continue;
    }

    const depth = Math.max(0, Number(entry.depth) || 0);
    const innerText = collectGadgetInnerText(items, index, depth);
    const text = gadgetTextForEntry(entry, innerText);
    if (!text) {
      continue;
    }

    const style = gadgetResolvedStyle(entry, index);
    const buttonLike = gadgetIsButtonLike(entry);
    const linkLike = gadgetIsLinkLike(tag);
    const fontSizePx = linkLike
      ? DEFAULT_GADGET_FONT_PX
      : Math.max(1, Math.round(Number(style && style.fontSizePx || gadgetFontSizePx(tag))));
    const lineHeightPx = linkLike
      ? DEFAULT_GADGET_LINE_HEIGHT_PX
      : Math.max(1, Math.round(Number(style && style.lineHeightPx || gadgetLineHeightPx(tag))));
    const paddingLeftPx = Math.max(0, Math.round(Number(style && style.paddingLeftPx || 0)));
    const paddingRightPx = Math.max(0, Math.round(Number(style && style.paddingRightPx || 0)));
    const paddingTopPx = Math.max(0, Math.round(Number(style && style.paddingTopPx || 0)));
    const paddingBottomPx = Math.max(0, Math.round(Number(style && style.paddingBottomPx || 0)));
    const xPx = depth * 12;
    const textWidthPx = estimateTextWidthPx(text, fontSizePx);
    const widthPx = buttonLike
      ? textWidthPx + paddingLeftPx + paddingRightPx
      : (linkLike
        ? textWidthPx + (LINK_GADGET_PAD_X * 2)
        : textWidthPx);
    const heightPx = buttonLike
      ? Math.max(lineHeightPx, lineHeightPx + paddingTopPx + paddingBottomPx)
      : (linkLike
        ? Math.max(lineHeightPx + (LINK_GADGET_PAD_Y * 2), lineHeightPx)
        : lineHeightPx);
    gadgets.push({
      nodeId: Math.max(1, Number(entry.nodeId) || (index + 1)),
      tag,
      text,
      xPx,
      yPx: yCursor,
      widthPx,
      heightPx,
      fontSizePx,
      lineHeightPx,
      textColorRgb: gadgetTextColorRgb(tag, style),
      buttonLike,
      changed: false,
    });
    yCursor += heightPx;
  }

  return {
    version: 1,
    backgroundColorRgb: Math.max(0, Number(backgroundColorRgb || 0) >>> 0) & 0xFFFFFF,
    gadgets,
  };
}

function ui3TagFillRgb(tag, buttonLike) {
  const name = collapseWhitespace(tag).toLowerCase();
  if (buttonLike) return 0xe7eefc;
  if (name === 'a') return 0xeef6ff;
  if (name === 'input' || name === 'textarea' || name === 'select') return 0xffffff;
  if (name === 'img' || name === 'canvas' || name === 'svg') return 0xfef3c7;
  if (name === 'table' || name === 'tr' || name === 'td' || name === 'th') return 0xf3f4f6;
  if (/^h[1-6]$/.test(name)) return 0xf7e8ff;
  return 0xf8fafc;
}

function ui3TagStrokeRgb(tag, buttonLike) {
  const name = collapseWhitespace(tag).toLowerCase();
  if (buttonLike) return 0x7c9be8;
  if (name === 'a') return 0x60a5fa;
  if (name === 'input' || name === 'textarea' || name === 'select') return 0x94a3b8;
  if (/^h[1-6]$/.test(name)) return 0xa855f7;
  return 0xcbd5e1;
}

function buildUi3Scene(bodyHierarchy, backgroundColorRgb = 0, limit = UI3_SCENE_NODE_LIMIT) {
  const items = Array.isArray(bodyHierarchy) ? bodyHierarchy : [];
  const ops = [];
  const rootId = 1;
  const backgroundId = 2;
  const contentRootId = 3;
  const bgRgb = Math.max(0, Number(backgroundColorRgb || 0) >>> 0) & 0xFFFFFF;
  let yCursor = 24;
  let emitted = 0;

  const node = (id, kind) => ops.push({ code: 1, node: id, a: kind });
  const addChild = (parent, child) => ops.push({ code: 2, node: parent, a: child });
  const position = (id, x, y) => ops.push({ code: 3, node: id, a: x, b: y });
  const clear = (id) => ops.push({ code: 4, node: id });
  const rect = (id, x, y, w, h) => ops.push({ code: 5, node: id, a: x, b: y, c: w, d: h });
  const fill = (id, color, alpha = 1) => ops.push({ code: 6, node: id, a: color, b: alpha });
  const stroke = (id, color, alpha = 1, width = 1) => ops.push({ code: 7, node: id, a: color, b: alpha, c: width });
  const text = (id, value) => ops.push({ code: 8, node: id, text: safeString(value) });
  const textFill = (id, color, alpha = 1) => ops.push({ code: 9, node: id, a: color, b: alpha });

  node(rootId, 0);
  node(backgroundId, 1);
  node(contentRootId, 0);
  addChild(rootId, backgroundId);
  addChild(rootId, contentRootId);
  position(rootId, 0, 0);
  position(backgroundId, 0, 0);
  position(contentRootId, 0, 0);
  clear(backgroundId);
  rect(backgroundId, 0, 0, UI3_SCENE_VIEWPORT_WIDTH, UI3_SCENE_VIEWPORT_HEIGHT);
  fill(backgroundId, bgRgb || 0xf8fafc, 1);

  for (let index = 0; index < items.length && emitted < limit; index += 1) {
    const entry = items[index] || {};
    const tag = collapseWhitespace(entry.tag).toLowerCase();
    if (!tag || tag === '#text') continue;

    const depth = Math.max(0, Number(entry.depth) || 0);
    const innerText = collectGadgetInnerText(items, index, depth);
    const label = gadgetTextForEntry(entry, innerText) || `<${tag}>`;
    if (!label) continue;

    const style = gadgetResolvedStyle(entry, index);
    const buttonLike = gadgetIsButtonLike(entry);
    const fontSizePx = Math.max(1, Math.round(Number(style && style.fontSizePx || gadgetFontSizePx(tag))));
    const lineHeightPx = Math.max(1, Math.round(Number(style && style.lineHeightPx || gadgetLineHeightPx(tag))));
    const paddingX = buttonLike || tag === 'a' ? 14 : 10;
    const paddingY = buttonLike || tag === 'a' ? 7 : 5;
    const xPx = 24 + depth * 22;
    const textWidthPx = estimateTextWidthPx(label, fontSizePx);
    const widthPx = Math.min(
      Math.max(48, textWidthPx + paddingX * 2),
      Math.max(48, UI3_SCENE_VIEWPORT_WIDTH - xPx - 32),
    );
    const heightPx = Math.max(lineHeightPx + paddingY * 2, buttonLike ? 34 : 26);
    const groupId = 1000 + emitted * 3;
    const graphicsId = groupId + 1;
    const textId = groupId + 2;
    const fillRgb = cssColorToRgbInt(style && style.backgroundColor) || ui3TagFillRgb(tag, buttonLike);
    const strokeRgb = ui3TagStrokeRgb(tag, buttonLike);
    const textRgb = gadgetTextColorRgb(tag, style) || 0x111827;

    node(groupId, 0);
    node(graphicsId, 1);
    node(textId, 2);
    addChild(contentRootId, groupId);
    addChild(groupId, graphicsId);
    addChild(groupId, textId);
    position(groupId, xPx, yCursor);
    position(graphicsId, 0, 0);
    position(textId, paddingX, paddingY);
    clear(graphicsId);
    rect(graphicsId, 0, 0, widthPx, heightPx);
    fill(graphicsId, fillRgb, buttonLike || tag === 'a' ? 1 : 0.92);
    stroke(graphicsId, strokeRgb, 1, buttonLike ? 2 : 1);
    text(textId, label);
    textFill(textId, textRgb, 1);

    yCursor += heightPx + 8;
    emitted += 1;
  }

  return {
    version: 1,
    rootId,
    viewportWidth: UI3_SCENE_VIEWPORT_WIDTH,
    viewportHeight: UI3_SCENE_VIEWPORT_HEIGHT,
    contentHeight: Math.max(UI3_SCENE_VIEWPORT_HEIGHT, yCursor + 24),
    opCount: ops.length,
    ops,
  };
}

function collectStyleArtifacts(source) {
  const html = safeString(source);
  const styles = [];
  const styleTagRe = /<style\b[^>]*>([\s\S]*?)<\/style>/gi;
  const linkTagRe = /<link\b[^>]*>/gi;
  let match;

  while ((match = styleTagRe.exec(html))) {
    styles.push({
      order: styles.length,
      cssText: safeString(match[1]),
      href: '',
      tagHtml: safeString(match[0]),
    });
  }

  while ((match = linkTagRe.exec(html))) {
    const tagHtml = safeString(match[0]);
    const rel = safeString(extractAttrValue(tagHtml, 'rel')).toLowerCase();
    if (!rel.split(/\s+/).includes('stylesheet')) {
      continue;
    }
    styles.push({
      order: styles.length,
      cssText: '',
      href: extractAttrValue(tagHtml, 'href'),
      tagHtml,
    });
  }

  return styles;
}

function collectScriptArtifacts(source) {
  const html = safeString(source);
  const scripts = [];
  const scriptTagRe = /<script\b[^>]*>([\s\S]*?)<\/script>/gi;
  let match;

  while ((match = scriptTagRe.exec(html))) {
    const tagHtml = safeString(match[0]);
    scripts.push({
      order: scripts.length,
      src: extractAttrValue(tagHtml, 'src'),
      scriptText: safeString(match[1]),
      tagHtml,
    });
  }

  return scripts;
}

function buildMinimalShell(title) {
  const safeTitle = escapeHtmlText(title);
  const titleChunk = TRUESURFER_SUBSET_PROFILE.includeTitle
    ? `<title>${safeTitle}</title>`
    : '';
  const headChunk = TRUESURFER_SUBSET_PROFILE.includeHead
    ? `<head>${titleChunk}</head>`
    : DEFAULT_HEAD_SHELL;
  const bodyChunk = TRUESURFER_SUBSET_PROFILE.includeBody
    ? DEFAULT_BODY_SHELL
    : '';
  return `<!doctype html><html>${headChunk}${bodyChunk}</html>`;
}

function extractDocumentArtifacts(source) {
  const startedAt = Date.now();
  const html = safeString(source);
  const headBlock = TRUESURFER_SUBSET_PROFILE.includeHead ? extractTagBlock(html, 'head') : null;
  const bodyBlock = TRUESURFER_SUBSET_PROFILE.includeBody ? extractTagBlock(html, 'body') : null;
  const title = TRUESURFER_SUBSET_PROFILE.includeTitle
    ? extractTitleFromHead(headBlock ? headBlock.inner : html)
    : '';
  const faviconHref = extractFaviconHref(headBlock ? headBlock.inner : html);
  const bodyHtml = TRUESURFER_SUBSET_PROFILE.includeBody
    ? (bodyBlock ? bodyBlock.inner : html)
    : '';
  const bodyBackgroundColorRgb = extractBodyBackgroundColorRgb(html, bodyBlock ? bodyBlock.openTag : '');
  const bodyHierarchy = TRUESURFER_SUBSET_PROFILE.includeBodyHierarchy
    ? collectBodyHierarchy(bodyHtml, BODY_HIERARCHY_ROOT_LIMIT)
    : [];
  const bodyHierarchySummary = summarizeBodyHierarchy(bodyHierarchy);
  const styles = TRUESURFER_SUBSET_PROFILE.includeStyles ? collectStyleArtifacts(html) : [];
  const scripts = TRUESURFER_SUBSET_PROFILE.includeScripts ? collectScriptArtifacts(html) : [];
  const styleIndex = emptyStyleIndex();
  const domParseMs = Date.now() - startedAt;
  const styleIndexMs = 0;
  const shellHtml = buildMinimalShell(title);
  const parseMs = Date.now() - startedAt;
  const gadgetSnapshot = buildGadgetSnapshot(bodyHierarchy, bodyBackgroundColorRgb);
  const ui3Scene = buildUi3Scene(bodyHierarchy, bodyBackgroundColorRgb);

  let styleBytes = 0;
  for (const style of styles) {
    styleBytes += safeString(style.cssText).length;
    styleBytes += safeString(style.tagHtml).length;
    styleBytes += safeString(style.href).length;
  }

  let scriptBytes = 0;
  for (const script of scripts) {
    scriptBytes += safeString(script.scriptText).length;
    scriptBytes += safeString(script.tagHtml).length;
    scriptBytes += safeString(script.src).length;
  }

  return {
    title,
    faviconHref,
    parseMs,
    domParseMs,
    styleIndexMs,
    shellHtml,
    shellBytes: shellHtml.length,
    bodyHtml,
    bodyBytes: bodyHtml.length,
    bodyHierarchy,
    bodyHierarchySummary,
    gadgetSnapshot,
    ui3Scene,
    styleCount: styles.length,
    styleBytes,
    styleSlotCount: styleIndex.styleSlotCount,
    styledNodeCount: styleIndex.nodeRefCount,
    styleRuleCount: styleIndex.ruleCount,
    scriptCount: scripts.length,
    scriptBytes,
    styles,
    scripts,
    styleIndex,
  };
}

export {
  TRUESURFER_SUBSET_PROFILE,
  extractDocumentArtifacts,
};
