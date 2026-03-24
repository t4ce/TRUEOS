/*
Truesurfer pipeline bridge:

html shack -> N-Browsers (Truesurfers)

In Truesurfer:
- Parse5 + CSS parse, JavaScript in parallel isolated
- Lightning CSS enrichment of the document and DOM subset
- Pipeline channel handoff to Yoga for layout enrichment

In UI2:
- N-window compositor-pattern UI that can already render the minimal demo
- Conceptually the full bridge is in place: acquired HTML can flow through parse,
  enrichment, layout, and minimal hosted composition for visual feedback
*/

const root = globalThis;
const browserId = Number(root.__trueosTruesurferBrowserId || 0);
const TRUESURFER_SUBSET_PROFILE = Object.freeze({
  includeHead: true,
  includeTitle: true,
  includeBody: true,
  includeStyles: true,
  includeScripts: true,
  includeBodyHierarchy: true,
  includeHostedTextRows: true,
  maxBodyHierarchyNodes: 10,
  maxBodyHierarchyDepth: 6,
  bodyTags: Object.freeze([
    'div', 'p', 'span', 'a', 'ul', 'ol', 'li', 'table', 'thead', 'tbody',
    'tr', 'td', 'th', 'section', 'article', 'header', 'footer', 'main', 'nav',
    'img', 'form', 'input', 'button', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6',
  ]),
});
const COMMON_BODY_TAGS = new Set(TRUESURFER_SUBSET_PROFILE.bodyTags);
const BODY_HIERARCHY_LIMIT = TRUESURFER_SUBSET_PROFILE.maxBodyHierarchyNodes;
const BODY_HIERARCHY_DEPTH_LIMIT = TRUESURFER_SUBSET_PROFILE.maxBodyHierarchyDepth;
const LAYOUT_INTENT_NODE_LIMIT = 32;
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

function log(line) {
  if (typeof console !== 'undefined' && console && typeof console.log === 'function') {
    console.log(line);
  }
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

function countLines(source) {
  if (!source) {
    return 1;
  }
  let lines = 1;
  for (let index = 0; index < source.length; index += 1) {
    if (source.charCodeAt(index) === 10) {
      lines += 1;
    }
  }
  return lines;
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

function collectBodyHierarchy(bodyHtml, limit = BODY_HIERARCHY_LIMIT) {
  const source = stripCommentAndRawTextNoise(bodyHtml);
  const tokenRe = /<\/?([a-zA-Z0-9:-]+)\b[^>]*>|([^<]+)/g;
  const stack = ['body'];
  const out = [];
  let match;

  while ((match = tokenRe.exec(source)) && out.length < limit) {
    const textChunk = match[2];
    if (textChunk != null) {
      const text = collapseWhitespace(decodeBasicEntities(textChunk));
      if (text && (stack.length - 1) <= BODY_HIERARCHY_DEPTH_LIMIT) {
        out.push({ depth: stack.length - 1, tag: '#text', text });
      }
      continue;
    }

    const fullTag = match[0];
    const tagName = safeString(match[1]).toLowerCase();
    const isClose = fullTag[1] === '/';
    const isSelfClosing = /\/\s*>$/.test(fullTag) || VOID_HTML_TAGS.has(tagName);

    if (isClose) {
      for (let index = stack.length - 1; index >= 1; index -= 1) {
        if (stack[index] === tagName) {
          stack.length = index;
          break;
        }
      }
      continue;
    }

    const currentDepth = stack.length - 1;
    if ((COMMON_BODY_TAGS.has(tagName) || COMMON_BODY_TAGS_FALLBACK.has(tagName)) && currentDepth <= BODY_HIERARCHY_DEPTH_LIMIT) {
      out.push({ depth: stack.length - 1, tag: tagName, text: '' });
    }

    if (!isSelfClosing) {
      stack.push(tagName);
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

function buildHostedTextRows(title, bodyHierarchy, limit = 24) {
  const rows = [];
  const titleText = collapseWhitespace(title);
  if (titleText) {
    rows.push({ text: titleText, indentPx: 0 });
  }
  const items = Array.isArray(bodyHierarchy) ? bodyHierarchy : [];
  for (let index = 0; index < items.length && rows.length < limit; index += 1) {
    const entry = items[index] || {};
    const tag = collapseWhitespace(entry.tag);
    const text = collapseWhitespace(entry.text);
    if (tag === '#text' && text) {
      const depth = Math.max(0, Number(entry.depth) || 0);
      rows.push({
        text,
        indentPx: depth * 12,
      });
      continue;
    }
    if (!tag) {
      continue;
    }
    const depth = Math.max(0, Number(entry.depth) || 0);
    rows.push({
      text: tag,
      indentPx: depth * 12,
    });
  }
  return rows;
}

function estimateTextWidthPx(text, fontPx) {
  const value = collapseWhitespace(text);
  if (!value) {
    return Math.max(1, Math.round(fontPx * 0.56));
  }
  const glyphPx = Math.max(6, Math.round(Number(fontPx || 14) * 0.56));
  return Math.max(glyphPx, value.length * glyphPx);
}

function classifyLayoutTag(tag) {
  switch (safeString(tag).toLowerCase()) {
    case '#text':
      return 'text';
    case 'img':
      return 'image';
    case 'button':
      return 'button';
    case 'input':
      return 'input';
    case 'hr':
      return 'rule';
    case 'a':
      return 'inline';
    case 'span':
      return 'inline';
    default:
      return 'block';
  }
}

function estimateLayoutNodeMetrics(tag, depth, text = '') {
  const name = safeString(tag).toLowerCase();
  const indent = Math.max(0, Number(depth) || 0) * 12;
  let intrinsicWidthPx = 0;
  let intrinsicHeightPx = 20;
  let minWidthPx = 0;
  let minHeightPx = 20;
  let paddingLeftPx = 0;
  let paddingTopPx = 0;
  let paddingRightPx = 0;
  let paddingBottomPx = 0;
  let marginBottomPx = 4;

  if (name === '#text') {
    intrinsicWidthPx = estimateTextWidthPx(text, 14);
    intrinsicHeightPx = 20;
    minWidthPx = intrinsicWidthPx;
    minHeightPx = intrinsicHeightPx;
    marginBottomPx = 2;
  } else if (/^h[1-6]$/.test(name)) {
    const level = Number(name[1] || 1);
    intrinsicHeightPx = Math.max(20, 34 - (level * 3));
    minHeightPx = intrinsicHeightPx;
    marginBottomPx = 8;
  } else if (name === 'button') {
    intrinsicWidthPx = 120;
    intrinsicHeightPx = 32;
    minWidthPx = intrinsicWidthPx;
    minHeightPx = intrinsicHeightPx;
    paddingLeftPx = 12;
    paddingRightPx = 12;
    paddingTopPx = 6;
    paddingBottomPx = 6;
    marginBottomPx = 6;
  } else if (name === 'input') {
    intrinsicWidthPx = 180;
    intrinsicHeightPx = 30;
    minWidthPx = intrinsicWidthPx;
    minHeightPx = intrinsicHeightPx;
    paddingLeftPx = 10;
    paddingRightPx = 10;
    paddingTopPx = 6;
    paddingBottomPx = 6;
    marginBottomPx = 6;
  } else if (name === 'img') {
    intrinsicWidthPx = 160;
    intrinsicHeightPx = 120;
    minWidthPx = intrinsicWidthPx;
    minHeightPx = intrinsicHeightPx;
    marginBottomPx = 8;
  } else if (name === 'hr') {
    intrinsicHeightPx = 8;
    minHeightPx = intrinsicHeightPx;
    marginBottomPx = 8;
  } else if (name === 'a' || name === 'span') {
    intrinsicWidthPx = estimateTextWidthPx(name, 14);
    intrinsicHeightPx = 20;
    minWidthPx = intrinsicWidthPx;
    minHeightPx = intrinsicHeightPx;
  }

  return {
    indentPx: indent,
    intrinsicWidthPx,
    intrinsicHeightPx,
    minWidthPx,
    minHeightPx,
    paddingLeftPx,
    paddingTopPx,
    paddingRightPx,
    paddingBottomPx,
    marginBottomPx,
  };
}

function buildLayoutIntent(title, bodyHierarchy, limit = LAYOUT_INTENT_NODE_LIMIT) {
  const nodes = [{
    nodeId: 1,
    parentId: 0,
    depth: 0,
    kind: 'root',
    tag: 'body',
    intrinsicWidthPx: 0,
    intrinsicHeightPx: 0,
    minWidthPx: 0,
    minHeightPx: 0,
    marginLeftPx: 0,
    marginTopPx: 0,
    marginRightPx: 0,
    marginBottomPx: 0,
    paddingLeftPx: 0,
    paddingTopPx: 0,
    paddingRightPx: 0,
    paddingBottomPx: 0,
    flexGrow: 0,
    flexShrink: 0,
  }];
  const stack = [1];
  let nextNodeId = 2;
  const titleText = collapseWhitespace(title);

  if (titleText && nodes.length < limit) {
    nodes.push({
      nodeId: nextNodeId,
      parentId: 1,
      depth: 1,
      kind: 'text',
      tag: 'title',
      text: titleText,
      intrinsicWidthPx: estimateTextWidthPx(titleText, 18),
      intrinsicHeightPx: 26,
      minWidthPx: 0,
      minHeightPx: 26,
      marginLeftPx: 0,
      marginTopPx: 0,
      marginRightPx: 0,
      marginBottomPx: 8,
      paddingLeftPx: 0,
      paddingTopPx: 0,
      paddingRightPx: 0,
      paddingBottomPx: 0,
      flexGrow: 0,
      flexShrink: 0,
    });
    nextNodeId += 1;
  }

  const items = Array.isArray(bodyHierarchy) ? bodyHierarchy : [];
  for (let index = 0; index < items.length && nodes.length < limit; index += 1) {
    const entry = items[index] || {};
    const tag = collapseWhitespace(entry.tag).toLowerCase();
    const text = collapseWhitespace(entry.text);
    if (!tag) {
      continue;
    }
    const depth = Math.max(0, Number(entry.depth) || 0);
    while (stack.length > depth + 1) {
      stack.pop();
    }
    const parentId = stack[stack.length - 1] || 1;
    const metrics = estimateLayoutNodeMetrics(tag, depth, text);
    const nodeId = nextNodeId;
    nextNodeId += 1;
    nodes.push({
      nodeId,
      parentId,
      depth: depth + 1,
      kind: classifyLayoutTag(tag),
      tag,
      text,
      intrinsicWidthPx: metrics.intrinsicWidthPx,
      intrinsicHeightPx: metrics.intrinsicHeightPx,
      minWidthPx: metrics.minWidthPx,
      minHeightPx: metrics.minHeightPx,
      marginLeftPx: metrics.indentPx,
      marginTopPx: 0,
      marginRightPx: 0,
      marginBottomPx: metrics.marginBottomPx,
      paddingLeftPx: metrics.paddingLeftPx,
      paddingTopPx: metrics.paddingTopPx,
      paddingRightPx: metrics.paddingRightPx,
      paddingBottomPx: metrics.paddingBottomPx,
      flexGrow: 0,
      flexShrink: 0,
    });
    if (tag !== '#text') {
      stack.push(nodeId);
    }
  }

  return {
    version: 1,
    nodes,
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
  const textRows = TRUESURFER_SUBSET_PROFILE.includeHostedTextRows
    ? buildHostedTextRows(title, bodyHierarchy)
    : [];
  const layoutIntent = buildLayoutIntent(title, bodyHierarchy);

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
    textRows,
    layoutIntent,
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

function logSyncPipeline(url, parsed) {
  log(
    `[truesurfer pipeline] browser=${browserId} mode=minimal_subset entry=signal stages=subset_scan>head+title>body_outline shell_bytes=${parsed.shellBytes} body_bytes=${parsed.bodyBytes} body_nodes=${parsed.bodyHierarchy.length} max_nodes=${TRUESURFER_SUBSET_PROFILE.maxBodyHierarchyNodes} max_depth=${TRUESURFER_SUBSET_PROFILE.maxBodyHierarchyDepth} url=${url}`,
  );
}

function setHtml(nextHtml, meta) {
  const html = safeString(nextHtml);
  const url = safeString(meta && meta.url);
  const lines = countLines(html);

  try {
    const parsed = extractDocumentArtifacts(html);
    logSyncPipeline(url, parsed);
    root.__trueosTruesurferLastStyleIndex = parsed.styleIndex;
    root.__trueosTruesurferLastArtifacts = {
      url,
      title: parsed.title,
      shellBytes: parsed.shellBytes,
      bodyBytes: parsed.bodyBytes,
      bodyHierarchy: parsed.bodyHierarchy,
      bodyHierarchySummary: parsed.bodyHierarchySummary,
      textRows: parsed.textRows,
      layoutIntent: parsed.layoutIntent,
      styleCount: parsed.styleCount,
      styleBytes: parsed.styleBytes,
      styleSlotCount: parsed.styleSlotCount,
      styledNodeCount: parsed.styledNodeCount,
      styleRuleCount: parsed.styleRuleCount,
      scriptCount: parsed.scriptCount,
      scriptBytes: parsed.scriptBytes,
    };
    log(
      `[truesurfer extract] browser=${browserId} title=${parsed.title} shell_bytes=${parsed.shellBytes} body_bytes=${parsed.bodyBytes} body_nodes=${parsed.bodyHierarchy.length} body_outline=${parsed.bodyHierarchySummary} style_count=${parsed.styleCount} style_slots=${parsed.styleSlotCount} styled_nodes=${parsed.styledNodeCount} style_rules=${parsed.styleRuleCount} script_count=${parsed.scriptCount} dom_ms=${parsed.domParseMs} css_ms=${parsed.styleIndexMs} ms=${parsed.parseMs} url=${url}`,
    );
    return {
      ok: 1,
      bytes: html.length,
      lines,
      parseMs: parsed.parseMs,
      domParseMs: parsed.domParseMs,
      styleIndexMs: parsed.styleIndexMs,
      title: parsed.title,
      shellBytes: parsed.shellBytes,
      bodyBytes: parsed.bodyBytes,
      textRows: parsed.textRows,
      layoutIntent: parsed.layoutIntent,
      styleCount: parsed.styleCount,
      styleBytes: parsed.styleBytes,
      styleSlotCount: parsed.styleSlotCount,
      styledNodeCount: parsed.styledNodeCount,
      styleRuleCount: parsed.styleRuleCount,
      scriptCount: parsed.scriptCount,
      scriptBytes: parsed.scriptBytes,
    };
  } catch (error) {
    const message =
      error && error.stack ? String(error.stack) : error ? String(error) : 'unknown minimal extract error';
    log(`[truesurfer extract] browser=${browserId} fail error=${message}`);
    return {
      ok: 0,
      bytes: html.length,
      lines,
      error: message,
    };
  }
}

root.__trueosTruesurfer = {
  setHtml,
};
root.__trueosTruesurferSubsetProfile = TRUESURFER_SUBSET_PROFILE;
root.__trueosTruesurferReady = 1;

log(`[truesurfer bootstrap] browser=${browserId} ready`);
