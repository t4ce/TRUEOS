import * as lightningcss from 'trueos:lightningcss';

function collapseWhitespace(s) {
  return String(s || '').replace(/\s+/g, ' ').trim();
}

function isElement(node) {
  return !!node && typeof node === 'object' && typeof node.tagName === 'string';
}

function isTextNode(node) {
  return !!node && typeof node === 'object' && node.nodeName === '#text' && typeof node.value === 'string';
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

function formatCssRows(cssText, baseDepth) {
  const raw = String(cssText || '').trim();
  if (!raw) return [];
  const rows = [];
  let cur = '';
  let d = Math.max(0, Number(baseDepth || 0) | 0);

  const pushLine = (text, depth) => {
    const t = String(text || '').trim();
    if (!t) return;
    rows.push({ depth: Math.max(0, Number(depth || 0) | 0), text: t });
  };

  for (let i = 0; i < raw.length; i++) {
    const ch = raw[i];
    if (ch === '{') {
      if (cur.trim()) pushLine(`${cur.trim()} {`, d);
      else pushLine('{', d);
      cur = '';
      d += 1;
      continue;
    }
    if (ch === '}') {
      if (cur.trim()) pushLine(cur.trim(), d);
      cur = '';
      d = Math.max(0, d - 1);
      pushLine('}', d);
      continue;
    }
    if (ch === ';') {
      cur += ';';
      if (cur.trim()) pushLine(cur.trim(), d);
      cur = '';
      continue;
    }
    cur += ch;
  }
  if (cur.trim()) pushLine(cur.trim(), d);
  return rows;
}

export function extractCssObjects(doc) {
  const cssObjects = [];
  const kids = Array.isArray(doc && doc.childNodes) ? doc.childNodes : [];
  for (let i = 0; i < kids.length; i++) {
    collectCssObjects(kids[i], `root.${i}`, cssObjects);
  }
  return cssObjects;
}

export function extractCssRows(doc) {
  const cssObjects = extractCssObjects(doc);
  const rows = [
    { depth: 0, text: '' },
    { depth: 0, text: '/* CSS */' },
  ];

  if (cssObjects.length <= 0) {
    rows.push({ depth: 0, text: '(no styles found)' });
    return { cssObjects, rows };
  }

  for (let i = 0; i < cssObjects.length; i++) {
    const it = cssObjects[i];
    const path = String(it && it.path || '');
    const tag = String(it && it.tag || '');
    const style = it && it.style || null;
    const kind = String(style && style.kind || 'unknown');
    rows.push({ depth: 0, text: `[${i}] ${path} <${tag}> ${kind}` });

    if (kind === 'external') {
      const href = String(style && style.source || '');
      rows.push({ depth: 1, text: `href: ${href || '(missing href)'}` });
      continue;
    }

    const css = String(style && style.css || '');
    const cssRows = formatCssRows(css, 1);
    if (cssRows.length <= 0) {
      rows.push({ depth: 1, text: '(empty css)' });
      continue;
    }
    for (let j = 0; j < cssRows.length; j++) {
      rows.push(cssRows[j]);
    }
  }

  return { cssObjects, rows };
}