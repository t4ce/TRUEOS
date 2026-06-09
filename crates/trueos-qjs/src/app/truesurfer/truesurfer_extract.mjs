import { cssColorToRgbInt, resolveInlineStyle } from './css.mjs';
import { BLOCK_TAGS } from './htmlDefaults.mjs';

const TRUESURFER_SUBSET_PROFILE = Object.freeze({
  includeHead: true,
  includeTitle: true,
  includeBody: true,
  includeStyles: true,
  includeScripts: true,
  includeBodyHierarchy: true,
  maxBodyHierarchyRoots: 100,
  maxBodyHierarchyChildrenPerNode: 100,
  maxBodyHierarchyDepth: 100,
  bodyTags: Object.freeze([
    'div', 'p', 'span', 'a', 'ul', 'ol', 'li', 'table', 'thead', 'tbody',
    'tr', 'td', 'th', 'section', 'article', 'header', 'footer', 'main', 'nav',
    'img', 'form', 'input', 'button', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6',
  ]),
});

const COMMON_BODY_TAGS = new Set([...TRUESURFER_SUBSET_PROFILE.bodyTags, ...BLOCK_TAGS]);
const BODY_HIERARCHY_ROOT_LIMIT = TRUESURFER_SUBSET_PROFILE.maxBodyHierarchyRoots;
const BODY_HIERARCHY_CHILD_LIMIT = TRUESURFER_SUBSET_PROFILE.maxBodyHierarchyChildrenPerNode;
const BODY_HIERARCHY_DEPTH_LIMIT = TRUESURFER_SUBSET_PROFILE.maxBodyHierarchyDepth;
const GADGET_SNAPSHOT_LIMIT = 48;
const DEFAULT_GADGET_FONT_PX = 14;
const DEFAULT_GADGET_LINE_HEIGHT_PX = 20;
const LINK_GADGET_PAD_X = 10;
const LINK_GADGET_PAD_Y = 6;
const LINK_GADGET_TEXT_COLOR_RGB = 0x1d4ed8;
const BODY_STYLE_TAG_RE = /<style\b[^>]*>([\s\S]*?)<\/style>/gi;
const DEFAULT_HEAD_SHELL = '<head></head>';
const DEFAULT_BODY_SHELL = '<body></body>';
const TRUESURFER_WIDGET_SVG_ENABLED = false;
const COMMON_BODY_TAGS_FALLBACK = new Set([
  'div', 'p', 'span', 'a', 'ul', 'ol', 'li', 'table', 'thead', 'tbody',
  'tr', 'td', 'th', 'section', 'article', 'header', 'footer', 'main', 'nav',
  'img', 'form', 'input', 'button', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6',
]);
const VOID_HTML_TAGS = new Set([
  'area', 'base', 'br', 'col', 'embed', 'hr', 'img', 'input', 'link', 'meta',
  'param', 'source', 'track', 'wbr',
]);
const RAW_TEXT_TAGS = new Set(['script', 'style', 'template']);

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

const normalizeWhitespace = collapseWhitespace;

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

function findClosingTagOutsideQuotes(source, tagName, startIndex) {
  const html = safeString(source);
  const lower = html.toLowerCase();
  const needle = `</${safeString(tagName).toLowerCase()}>`;
  let inQuote = '';
  for (let index = Math.max(0, Number(startIndex) || 0); index < html.length; index += 1) {
    const ch = html[index];
    if (inQuote) {
      if (ch === inQuote) inQuote = '';
      continue;
    }
    if (ch === '"' || ch === "'") {
      inQuote = ch;
      continue;
    }
    if (lower.startsWith(needle, index)) return index;
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
  const closeStart = findClosingTagOutsideQuotes(html, tagName, openEnd + 1);
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

function attrArrayToMap(attrs) {
  const out = {};
  const items = Array.isArray(attrs) ? attrs : [];
  for (const attr of items) {
    if (!attr || typeof attr.name !== 'string') continue;
    out[attr.name] = safeString(attr.value);
  }
  return Object.keys(out).length > 0 ? out : undefined;
}

function parseTagName(tagHtml) {
  const match = /^<\s*\/?\s*([a-zA-Z0-9:-]+)/.exec(safeString(tagHtml));
  return match ? safeString(match[1]).toLowerCase() : '';
}

function parseTagAttributes(tagHtml) {
  const source = safeString(tagHtml);
  const tagName = parseTagName(source);
  if (!tagName) return [];
  let body = source.slice(source.indexOf(tagName) + tagName.length);
  if (body.endsWith('>')) body = body.slice(0, -1);
  if (body.endsWith('/')) body = body.slice(0, -1);
  const attrs = [];
  const attrRe = /([^\s"'<>/=]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'=<>`]+)))?/g;
  let match;
  while ((match = attrRe.exec(body))) {
    const name = safeString(match[1]);
    if (!name) continue;
    attrs.push({
      name,
      value: decodeBasicEntities(match[2] ?? match[3] ?? match[4] ?? ''),
    });
  }
  return attrs;
}

function appendDomText(parent, rawText) {
  const text = decodeBasicEntities(safeString(rawText));
  if (!text) return;
  parent.childNodes.push({ nodeName: '#text', value: text });
}

function buildBodyDomTree(bodyHtml) {
  const source = stripCommentAndRawTextNoise(bodyHtml);
  const root = { nodeName: 'body', tagName: 'body', attrs: [], childNodes: [] };
  const stack = [root];
  let index = 0;

  while (index < source.length) {
    const open = source.indexOf('<', index);
    if (open < 0) {
      appendDomText(stack[stack.length - 1], source.slice(index));
      break;
    }
    if (open > index) {
      appendDomText(stack[stack.length - 1], source.slice(index, open));
    }

    if (source.slice(open, open + 4) === '<!--') {
      const end = source.indexOf('-->', open + 4);
      index = end >= 0 ? end + 3 : source.length;
      continue;
    }

    const close = findTagClose(source, open);
    if (close < 0) {
      appendDomText(stack[stack.length - 1], source.slice(open));
      break;
    }

    const tagHtml = source.slice(open, close + 1);
    const tagName = parseTagName(tagHtml);
    if (!tagName) {
      index = close + 1;
      continue;
    }

    const isClose = /^<\s*\//.test(tagHtml);
    if (isClose) {
      for (let scan = stack.length - 1; scan >= 1; scan -= 1) {
        if (stack[scan].tagName === tagName) {
          stack.length = scan;
          break;
        }
      }
      index = close + 1;
      continue;
    }

    if (RAW_TEXT_TAGS.has(tagName)) {
      const closeNeedle = `</${tagName}>`;
      const rawClose = source.toLowerCase().indexOf(closeNeedle, close + 1);
      index = rawClose >= 0 ? rawClose + closeNeedle.length : close + 1;
      continue;
    }

    const node = {
      nodeName: tagName,
      tagName,
      attrs: parseTagAttributes(tagHtml),
      childNodes: [],
    };
    stack[stack.length - 1].childNodes.push(node);

    const isSelfClosing = /\/\s*>$/.test(tagHtml) || VOID_HTML_TAGS.has(tagName);
    if (!isSelfClosing) {
      stack.push(node);
    }
    index = close + 1;
  }

  return root;
}

function isDomElement(node) {
  return node && typeof node === 'object' && typeof node.nodeName === 'string' && Array.isArray(node.childNodes);
}

function isDomText(node) {
  return node && typeof node === 'object' && node.nodeName === '#text' && typeof node.value === 'string';
}

function directTextFromDomNode(node) {
  if (!isDomElement(node)) return '';
  let text = '';
  for (const child of node.childNodes) {
    if (isDomText(child)) text += child.value;
  }
  return normalizeWhitespace(text);
}

function extractDomText(node) {
  if (isDomText(node)) return node.value;
  if (!isDomElement(node)) return '';
  return node.childNodes.map(extractDomText).join(' ');
}

function escapeHtmlAttr(value) {
  return safeString(value)
    .replace(/&/g, '&amp;')
    .replace(/"/g, '&quot;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;');
}

function serializeDomNode(node) {
  if (isDomText(node)) return escapeHtmlText(node.value);
  if (!isDomElement(node)) return '';
  const tagName = safeString(node.tagName || node.nodeName).toLowerCase();
  if (!tagName || tagName === 'body' || tagName === 'html') {
    return node.childNodes.map(serializeDomNode).join('');
  }
  const attrs = Array.isArray(node.attrs) ? node.attrs : [];
  const attrText = attrs
    .map((attr) => attr && attr.name ? ` ${attr.name}="${escapeHtmlAttr(attr.value)}"` : '')
    .join('');
  if (VOID_HTML_TAGS.has(tagName)) return `<${tagName}${attrText}>`;
  return `<${tagName}${attrText}>${node.childNodes.map(serializeDomNode).join('')}</${tagName}>`;
}

function collectIframeSrcdocTextRows(srcdoc) {
  const rows = [];
  const doc = buildBodyDomTree(srcdoc);
  const pushUnique = (text) => {
    const cleaned = normalizeWhitespace(text);
    if (!cleaned || rows.includes(cleaned)) return;
    rows.push(cleaned);
  };
  const walk = (node) => {
    if (rows.length >= 16 || !isDomElement(node)) return;
    const tag = safeString(node.tagName || node.nodeName).toLowerCase();
    if (/^h[1-6]$/.test(tag) || tag === 'label' || tag === 'p') {
      pushUnique(directTextFromDomNode(node));
    } else if (tag === 'button') {
      pushUnique(extractDomText(node));
    }
    for (const child of node.childNodes) walk(child);
  };
  walk(doc);
  return rows;
}

function toWidgetRenderTree(node, path = '0') {
  if (!isDomElement(node)) return [];

  const tagName = safeString(node.tagName || node.nodeName).toLowerCase();
  const attrs = attrArrayToMap(node.attrs);

  if (tagName === 'textarea') {
    const value = extractDomText(node);
    return [{ kind: 'block', key: `${path}:${tagName}`, tagName, attrs: { ...(attrs || {}), value }, children: [] }];
  }

  if (tagName === 'input') {
    const inputType = safeString(attrs && attrs.type || 'text').toLowerCase();
    if (inputType === 'time') return [{ kind: 'block', key: `${path}:input`, tagName: 'timeinput', attrs, children: [] }];
    if (inputType === 'month') return [{ kind: 'block', key: `${path}:input`, tagName: 'monthinput', attrs, children: [] }];
    if (inputType === 'week') return [{ kind: 'block', key: `${path}:input`, tagName: 'weekinput', attrs, children: [] }];
    if (inputType === 'date') return [{ kind: 'block', key: `${path}:input`, tagName: 'dateinput', attrs, children: [] }];
    if (inputType === 'datetime-local') return [{ kind: 'block', key: `${path}:input`, tagName: 'datetimelocalinput', attrs, children: [] }];
  }

  if (tagName === 'progress' || tagName === 'meter') {
    const fallbackText = normalizeWhitespace(extractDomText(node));
    const children = [];
    if (fallbackText) children.push({ kind: 'text', text: fallbackText });
    children.push({ kind: 'block', key: `${path}:${tagName}`, tagName, attrs, children: [] });
    return [{ kind: 'block', key: `${path}:${tagName}-row`, tagName: 'barrow', attrs: { 'data-kind': tagName }, children }];
  }

  if (tagName === 'slider') {
    const sliderKey = `${path}:${tagName}`;
    return [{
      kind: 'block',
      key: `${path}:${tagName}-row`,
      tagName: 'barrow',
      attrs: { 'data-kind': tagName },
      children: [
        { kind: 'block', key: `${path}:${tagName}-label`, tagName: 'sliderlabel', attrs: { 'data-slider-key': sliderKey, 'data-slider-init': safeString(attrs && attrs.value) }, children: [] },
        { kind: 'block', key: sliderKey, tagName, attrs, children: [] },
      ],
    }];
  }

  if (tagName === 'img' || tagName === 'canvas' || tagName === 'number') {
    return [{ kind: 'block', key: `${path}:${tagName}`, tagName, attrs, children: [] }];
  }

  if (tagName === 'svg') {
    if (!TRUESURFER_WIDGET_SVG_ENABLED) {
      return [];
    }
    return [{ kind: 'block', key: `${path}:${tagName}`, tagName, attrs: { ...(attrs || {}), 'data-svg': serializeDomNode(node) }, children: [] }];
  }

  if (tagName === 'iframe') {
    const iframeKey = `${path}:${tagName}`;
    const srcdoc = safeString(attrs && attrs.srcdoc);
    const rows = collectIframeSrcdocTextRows(srcdoc);
    const iframeAttrs = rows.length > 0
      ? { ...(attrs || {}), 'data-trueos-srcdoc-text': rows.join('\n') }
      : attrs;
    const children = srcdoc.trim()
      ? toWidgetRenderTree(buildBodyDomTree(srcdoc), `${path}:iframe-doc`)
      : [];
    return [{ kind: 'block', key: iframeKey, tagName, attrs: iframeAttrs, children }];
  }

  if (tagName === 'select') {
    const options = [];
    let selectedIndex = 0;
    for (const child of node.childNodes) {
      if (!isDomElement(child)) continue;
      const childTag = safeString(child.tagName || child.nodeName).toLowerCase();
      if (childTag !== 'option') continue;
      const label = normalizeWhitespace(extractDomText(child));
      if (label) options.push(label);
      const optionAttrs = attrArrayToMap(child.attrs) || {};
      if (Object.prototype.hasOwnProperty.call(optionAttrs, 'selected')) selectedIndex = Math.max(0, options.length - 1);
    }
    return [{
      kind: 'block',
      key: `${path}:${tagName}`,
      tagName,
      attrs: { ...(attrs || {}), 'data-options': options.join('\n'), 'data-selected-index': safeString(selectedIndex) },
      children: [],
    }];
  }

  if (tagName === 'color') {
    const mkSpin = (channel, value) => ({
      kind: 'block',
      key: `${path}:color-${channel}`,
      tagName: 'number',
      attrs: { channel, min: '0', max: '255', step: '1', value },
      children: [],
    });
    return [
      { kind: 'block', key: `${path}:color`, tagName: 'color', attrs, children: [] },
      {
        kind: 'block',
        key: `${path}:color-controls`,
        tagName: 'p',
        attrs: {},
        children: [mkSpin('r', '255'), mkSpin('g', '0'), mkSpin('b', '0'), mkSpin('a', '255')],
      },
    ];
  }

  if (tagName === 'search') {
    const inputKey = `${path}:search-input`;
    return [{
      kind: 'block',
      key: `${path}:search-row`,
      tagName: 'searchrow',
      attrs: {},
      children: [
        { kind: 'block', key: `${path}:search-btn`, tagName: 'searchbutton', attrs: { 'data-focus-key': inputKey }, children: [] },
        { kind: 'block', key: inputKey, tagName: 'input', attrs: { ...(attrs || {}), type: 'text' }, children: [] },
      ],
    }];
  }

  if (tagName === 'details' || tagName === 'stub') {
    const detailsKey = `${path}:${tagName}`;
    const summaryEl = node.childNodes.find((child) => isDomElement(child) && safeString(child.tagName || child.nodeName).toLowerCase() === 'summary');
    const summaryAttrs = attrArrayToMap(summaryEl && summaryEl.attrs) || {};
    const summaryTextFallback = summaryEl
      ? normalizeWhitespace(extractDomText(summaryEl))
      : normalizeWhitespace(safeString(attrs && (attrs.summary || attrs.title))) || 'Details';

    const buildSummaryChildren = () => {
      if (!summaryEl) return summaryTextFallback ? [{ kind: 'text', text: summaryTextFallback }] : [];
      const keep = [];
      const trailing = [];
      let inlineText = '';
      let elementIndex = 0;
      for (const child of summaryEl.childNodes) {
        if (isDomText(child)) {
          inlineText += child.value;
          continue;
        }
        if (!isDomElement(child)) continue;
        const childTag = safeString(child.tagName || child.nodeName).toLowerCase();
        const childPath = `${path}:summary.${elementIndex}`;
        elementIndex += 1;
        if (childTag === 'input' || childTag === 'button' || childTag === 'select' || childTag === 'textarea') {
          const nodes = toWidgetRenderTree(child, childPath);
          const childAttrs = attrArrayToMap(child.attrs) || {};
          const inputType = safeString(childAttrs.type || 'text').toLowerCase();
          if (childTag === 'input' && (inputType === 'checkbox' || inputType === 'radio')) trailing.push(...nodes);
          else keep.push(...nodes);
        } else {
          inlineText += extractDomText(child) + ' ';
        }
      }
      const text = normalizeWhitespace(inlineText);
      const textNodes = text ? [{ kind: 'text', text }] : [];
      const out = [...textNodes, ...keep, ...trailing];
      return out.length > 0 ? out : summaryTextFallback ? [{ kind: 'text', text: summaryTextFallback }] : [];
    };

    const children = [{
      kind: 'block',
      key: `${path}:summary`,
      tagName: 'summary',
      attrs: {
        ...summaryAttrs,
        'data-details-key': detailsKey,
        ...(attrs && Object.prototype.hasOwnProperty.call(attrs, 'open') ? { 'data-details-open': '1' } : {}),
      },
      children: buildSummaryChildren(),
    }];

    let elementIndex = 0;
    for (const child of node.childNodes) {
      if (!isDomElement(child)) continue;
      const childTag = safeString(child.tagName || child.nodeName).toLowerCase();
      const childPath = `${path}.${elementIndex}`;
      elementIndex += 1;
      if (childTag === 'summary') continue;
      if (BLOCK_TAGS.has(childTag)) children.push(...toWidgetRenderTree(child, childPath));
    }
    return [{ kind: 'block', key: detailsKey, tagName: 'details', attrs, children }];
  }

  const children = [];
  let inlineText = '';
  let elementIndex = 0;
  for (const child of node.childNodes) {
    if (isDomText(child)) {
      inlineText += child.value;
      continue;
    }
    if (!isDomElement(child)) continue;
    const childTag = safeString(child.tagName || child.nodeName).toLowerCase();
    const childPath = `${path}.${elementIndex}`;
    elementIndex += 1;
    if (BLOCK_TAGS.has(childTag)) {
      const text = normalizeWhitespace(inlineText);
      if (text) children.push({ kind: 'text', text });
      inlineText = '';
      children.push(...toWidgetRenderTree(child, childPath));
    } else {
      inlineText += extractDomText(child) + ' ';
    }
  }

  const tail = normalizeWhitespace(inlineText);
  if (tail) children.push({ kind: 'text', text: tail });
  if (tagName === 'html' || tagName === 'body') return children;
  return [{ kind: 'block', key: `${path}:${tagName}`, tagName, attrs, children }];
}

function buildWidgetRenderTree(bodyHtml) {
  const inner = toWidgetRenderTree(buildBodyDomTree(bodyHtml), '0');
  return [{
    kind: 'block',
    key: 'root:internal-iframe',
    tagName: 'iframe',
    attrs: { 'data-root': '1' },
    children: inner,
  }];
}

function collectWidgetTextRows(nodes) {
  const rows = [];
  const push = (text) => {
    const cleaned = normalizeWhitespace(safeString(text));
    if (!cleaned) return;
    if (cleaned.indexOf('<truesurfer-') === 0 || cleaned.indexOf('__trueo') === 0) return;
    rows.push(cleaned);
  };
  const walk = (node) => {
    if (!node || typeof node !== 'object' || rows.length >= 512) return;
    if (node.kind === 'text') {
      push(node.text);
      return;
    }
    const children = Array.isArray(node.children) ? node.children : [];
    for (const child of children) walk(child);
  };
  const roots = Array.isArray(nodes) ? nodes : [];
  for (const node of roots) walk(node);
  return rows;
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
  const widgetRenderTree = buildWidgetRenderTree(bodyHtml);
  const widgetTextRows = collectWidgetTextRows(widgetRenderTree);
  const bodyHierarchySummary = summarizeBodyHierarchy(bodyHierarchy);
  const styles = TRUESURFER_SUBSET_PROFILE.includeStyles ? collectStyleArtifacts(html) : [];
  const scripts = TRUESURFER_SUBSET_PROFILE.includeScripts ? collectScriptArtifacts(html) : [];
  const styleIndex = emptyStyleIndex();
  const domParseMs = Date.now() - startedAt;
  const styleIndexMs = 0;
  const shellHtml = buildMinimalShell(title);
  const parseMs = Date.now() - startedAt;
  const gadgetSnapshot = buildGadgetSnapshot(bodyHierarchy, bodyBackgroundColorRgb);
  const ui3Scene = null;

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
    widgetRenderTree,
    widgetTextRows,
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
