import { passHtmlThroughDiffBox } from '/qjs/truesurfer/diff_box.mjs';

const root = globalThis;
const browserId = Number(root.__trueosTruesurferBrowserId || 0);
const TRUESURFER_SUBSET_PROFILE = Object.freeze({
  includeHead: true,
  includeTitle: true,
  includeBody: true,
  includeStyles: true,
  includeScripts: true,
  includeBodyHierarchy: true,
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
const DEFAULT_HEAD_SHELL = '<head></head>';
const DEFAULT_BODY_SHELL = '<body></body>';
const DEFAULT_HTML_SHELL = '<html></html>';
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
  const tagRe = /<\/?([a-zA-Z0-9:-]+)\b[^>]*>/g;
  const stack = ['body'];
  const out = [];
  let match;

  while ((match = tagRe.exec(source)) && out.length < limit) {
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
    if (COMMON_BODY_TAGS.has(tagName) && currentDepth <= BODY_HIERARCHY_DEPTH_LIMIT) {
      out.push({ depth: stack.length - 1, tag: tagName });
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
    const depth = Math.max(0, Number(entry.depth) || 0);
    parts.push(`${'.'.repeat(depth)}${entry.tag}`);
  }
  return parts.join(' > ');
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

function logSyncPipeline(forwarded, parsed) {
  // This path is intentionally tiny by design: signal in, then a minimal subset scan.
  log(
    `[truesurfer pipeline] browser=${browserId} mode=minimal_subset entry=signal stages=diff_box>subset_scan>head+title>body_outline changed=${forwarded.changed ? 1 : 0} shell_bytes=${parsed.shellBytes} body_bytes=${parsed.bodyBytes} body_nodes=${parsed.bodyHierarchy.length} max_nodes=${TRUESURFER_SUBSET_PROFILE.maxBodyHierarchyNodes} max_depth=${TRUESURFER_SUBSET_PROFILE.maxBodyHierarchyDepth} url=${forwarded.url}`,
  );
}

function setHtml(nextHtml, meta) {
  const forwarded = passHtmlThroughDiffBox(nextHtml, meta || {});
  const html = forwarded.html;
  const url = forwarded.url;
  const lines = countLines(html);

  try {
    const parsed = extractDocumentArtifacts(html);
    logSyncPipeline(forwarded, parsed);
    root.__trueosTruesurferLastStyleIndex = parsed.styleIndex;
    root.__trueosTruesurferLastArtifacts = {
      url,
      title: parsed.title,
      shellBytes: parsed.shellBytes,
      bodyBytes: parsed.bodyBytes,
      bodyHierarchy: parsed.bodyHierarchy,
      bodyHierarchySummary: parsed.bodyHierarchySummary,
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
