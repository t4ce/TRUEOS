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
const DEFAULT_GADGET_FONT_PX = 14;
const DEFAULT_GADGET_LINE_HEIGHT_PX = 20;
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

function stripCommentAndRawTextNoise(html) {
  return safeString(html)
    .replace(/<!--[\s\S]*?-->/g, '')
    .replace(/<script\b[^>]*>[\s\S]*?<\/script>/gi, '')
    .replace(/<style\b[^>]*>[\s\S]*?<\/style>/gi, '');
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
  return name === 'p' || name === 'span' || name === 'button' || /^h[1-6]$/.test(name);
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

function buildGadgetSnapshot(bodyHierarchy, limit = GADGET_SNAPSHOT_LIMIT) {
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
    const fontSizePx = Math.max(1, Math.round(Number(style && style.fontSizePx || gadgetFontSizePx(tag))));
    const lineHeightPx = Math.max(1, Math.round(Number(style && style.lineHeightPx || gadgetLineHeightPx(tag))));
    const paddingLeftPx = Math.max(0, Math.round(Number(style && style.paddingLeftPx || 0)));
    const paddingRightPx = Math.max(0, Math.round(Number(style && style.paddingRightPx || 0)));
    const paddingTopPx = Math.max(0, Math.round(Number(style && style.paddingTopPx || 0)));
    const paddingBottomPx = Math.max(0, Math.round(Number(style && style.paddingBottomPx || 0)));
    const xPx = depth * 12;
    const textWidthPx = estimateTextWidthPx(text, fontSizePx);
    const widthPx = buttonLike
      ? textWidthPx + paddingLeftPx + paddingRightPx
      : textWidthPx;
    const heightPx = buttonLike
      ? Math.max(lineHeightPx, lineHeightPx + paddingTopPx + paddingBottomPx)
      : lineHeightPx;
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
      textColorRgb: cssColorToRgbInt(style && style.color),
      buttonLike,
      changed: false,
    });
    yCursor += heightPx;
  }

  return {
    version: 1,
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
  const bodyHtml = TRUESURFER_SUBSET_PROFILE.includeBody
    ? (bodyBlock ? bodyBlock.inner : html)
    : '';
  const bodyHierarchy = TRUESURFER_SUBSET_PROFILE.includeBodyHierarchy
    ? collectBodyHierarchy(bodyHtml, BODY_HIERARCHY_LIMIT)
    : [];
  const bodyHierarchySummary = summarizeBodyHierarchy(bodyHierarchy);
  const styles = TRUESURFER_SUBSET_PROFILE.includeStyles ? collectStyleArtifacts(html) : [];
  const scripts = TRUESURFER_SUBSET_PROFILE.includeScripts ? collectScriptArtifacts(html) : [];
  const styleIndex = emptyStyleIndex();
  const domParseMs = Date.now() - startedAt;
  const styleIndexMs = 0;
  const shellHtml = buildMinimalShell(title);
  const parseMs = Date.now() - startedAt;
  const gadgetSnapshot = buildGadgetSnapshot(bodyHierarchy);

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
