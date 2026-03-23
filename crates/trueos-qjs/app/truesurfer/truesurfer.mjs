import parse5 from 'parse5';
import { passHtmlThroughDiffBox } from '/qjs/truesurfer/diff_box.mjs';

const root = globalThis;
const browserId = Number(root.__trueosTruesurferBrowserId || 0);

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

function childElements(node) {
  if (!node || !Array.isArray(node.childNodes)) {
    return [];
  }
  return node.childNodes.filter((child) => child && child.nodeName && !child.nodeName.startsWith('#'));
}

function firstChildElementByTagName(node, tagName) {
  const target = safeString(tagName).toLowerCase();
  const children = childElements(node);
  for (const child of children) {
    if (safeString(child.tagName || child.nodeName).toLowerCase() === target) {
      return child;
    }
  }
  return null;
}

function extractDocumentTitle(doc) {
  const htmlNode = firstChildElementByTagName(doc, 'html');
  const headNode = firstChildElementByTagName(htmlNode || doc, 'head');
  const titleNode = firstChildElementByTagName(headNode, 'title');
  if (!titleNode || !Array.isArray(titleNode.childNodes)) {
    return '';
  }

  let title = '';
  for (const child of titleNode.childNodes) {
    if (child && child.nodeName === '#text') {
      title += safeString(child.value);
    }
  }
  return title.trim();
}

function relListIncludes(node, value) {
  const attrs = Array.isArray(node && node.attrs) ? node.attrs : [];
  const relAttr = attrs.find((attr) => safeString(attr && attr.name).toLowerCase() === 'rel');
  if (!relAttr) {
    return false;
  }
  const relValue = safeString(relAttr.value).toLowerCase();
  return relValue.split(/\s+/).includes(safeString(value).toLowerCase());
}

function findAttr(node, name) {
  const target = safeString(name).toLowerCase();
  const attrs = Array.isArray(node && node.attrs) ? node.attrs : [];
  const attr = attrs.find((candidate) => safeString(candidate && candidate.name).toLowerCase() === target);
  return attr ? safeString(attr.value) : '';
}

function serializeNode(node) {
  return node ? parse5.serialize(node) : '';
}

function serializeChildNodes(node) {
  if (!node || !Array.isArray(node.childNodes) || node.childNodes.length === 0) {
    return '';
  }
  let html = '';
  for (const child of node.childNodes) {
    html += serializeNode(child);
  }
  return html;
}

function detachNodeFromParent(node) {
  const parent = node && node.parentNode;
  if (!parent || !Array.isArray(parent.childNodes)) {
    return;
  }
  const nextChildren = [];
  for (const child of parent.childNodes) {
    if (child !== node) {
      nextChildren.push(child);
    }
  }
  parent.childNodes = nextChildren;
  node.parentNode = null;
}

function walkAndExtractResiduals(node, artifacts) {
  if (!node || !Array.isArray(node.childNodes) || node.childNodes.length === 0) {
    return;
  }

  const snapshot = node.childNodes.slice();
  for (const child of snapshot) {
    const tagName = safeString(child && (child.tagName || child.nodeName)).toLowerCase();

    if (tagName === 'style') {
      artifacts.styles.push({
        order: artifacts.styles.length,
        cssText: serializeChildNodes(child),
        tagHtml: serializeNode(child),
      });
      detachNodeFromParent(child);
      continue;
    }

    if (tagName === 'script') {
      artifacts.scripts.push({
        order: artifacts.scripts.length,
        src: findAttr(child, 'src'),
        scriptText: serializeChildNodes(child),
        tagHtml: serializeNode(child),
      });
      detachNodeFromParent(child);
      continue;
    }

    if (tagName === 'link' && relListIncludes(child, 'stylesheet')) {
      artifacts.styles.push({
        order: artifacts.styles.length,
        href: findAttr(child, 'href'),
        cssText: '',
        tagHtml: serializeNode(child),
      });
      detachNodeFromParent(child);
      continue;
    }

    walkAndExtractResiduals(child, artifacts);
  }
}

function pushHierarchyRows(node, depth, rows) {
  if (!node || !Array.isArray(node.childNodes) || node.childNodes.length === 0) {
    return;
  }

  for (const child of node.childNodes) {
    const tagName = safeString(child && (child.tagName || child.nodeName)).toLowerCase();
    if (!tagName || tagName.startsWith('#')) {
      continue;
    }
    rows.push(`${depth}|<${tagName}>`);
    pushHierarchyRows(child, depth + 1, rows);
    rows.push(`${depth}|</${tagName}>`);
  }
}

function buildHierarchyRowsText(doc) {
  const rows = [];
  pushHierarchyRows(doc, 0, rows);
  return rows.join('\n');
}

function extractDocumentArtifacts(source) {
  const startedAt = Date.now();
  const doc = parse5.parse(source);
  const title = extractDocumentTitle(doc);
  const htmlNode = firstChildElementByTagName(doc, 'html');
  const bodyNode = firstChildElementByTagName(htmlNode || doc, 'body');
  const artifacts = {
    styles: [],
    scripts: [],
    bodyHtml: '',
    shellHtml: '',
    hierarchyRowsText: '',
  };

  walkAndExtractResiduals(doc, artifacts);
  artifacts.hierarchyRowsText = buildHierarchyRowsText(doc);

  if (bodyNode) {
    artifacts.bodyHtml = serializeChildNodes(bodyNode);
    bodyNode.childNodes = [];
  }

  artifacts.shellHtml = parse5.serialize(doc);
  const parseMs = Date.now() - startedAt;

  let styleBytes = 0;
  for (const style of artifacts.styles) {
    styleBytes += safeString(style.cssText).length;
    styleBytes += safeString(style.tagHtml).length;
    styleBytes += safeString(style.href).length;
  }

  let scriptBytes = 0;
  for (const script of artifacts.scripts) {
    scriptBytes += safeString(script.scriptText).length;
    scriptBytes += safeString(script.tagHtml).length;
    scriptBytes += safeString(script.src).length;
  }

  return {
    title,
    parseMs,
    shellHtml: artifacts.shellHtml,
    shellBytes: artifacts.shellHtml.length,
    bodyHtml: artifacts.bodyHtml,
    bodyBytes: artifacts.bodyHtml.length,
    hierarchyRowsText: artifacts.hierarchyRowsText,
    styleCount: artifacts.styles.length,
    styleBytes,
    scriptCount: artifacts.scripts.length,
    scriptBytes,
    styles: artifacts.styles,
    scripts: artifacts.scripts,
  };
}

function setHtml(nextHtml, meta) {
  const forwarded = passHtmlThroughDiffBox(nextHtml, meta || {});
  const html = forwarded.html;
  const url = forwarded.url;
  const lines = countLines(html);

  try {
    const parsed = extractDocumentArtifacts(html);
    root.__trueosTruesurferLastArtifacts = {
      url,
      title: parsed.title,
      shellHtml: parsed.shellHtml,
      bodyHtml: parsed.bodyHtml,
      hierarchyRowsText: parsed.hierarchyRowsText,
      styles: parsed.styles,
      scripts: parsed.scripts,
    };
    log(
      `[truesurfer extract] browser=${browserId} title=${parsed.title} shell_bytes=${parsed.shellBytes} body_bytes=${parsed.bodyBytes} style_count=${parsed.styleCount} script_count=${parsed.scriptCount} ms=${parsed.parseMs} url=${url}`,
    );
    return {
      ok: 1,
      bytes: html.length,
      lines,
      parseMs: parsed.parseMs,
      title: parsed.title,
      hierarchyRows: parsed.hierarchyRowsText,
      shellBytes: parsed.shellBytes,
      bodyBytes: parsed.bodyBytes,
      styleCount: parsed.styleCount,
      styleBytes: parsed.styleBytes,
      scriptCount: parsed.scriptCount,
      scriptBytes: parsed.scriptBytes,
    };
  } catch (error) {
    const message =
      error && error.stack ? String(error.stack) : error ? String(error) : 'unknown parse5 error';
    log(`[truesurfer parse5] browser=${browserId} fail error=${message}`);
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
root.__trueosTruesurferReady = 1;
