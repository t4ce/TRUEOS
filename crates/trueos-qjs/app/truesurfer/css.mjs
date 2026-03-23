import * as lightningcss from 'trueos:lightningcss';
import { createComputedStyle } from './cssDefaults.mjs';

const COMPACT_STYLE_FIELDS = [
  'display',
  'color',
  'backgroundColor',
  'fontSizePx',
  'fontWeight',
  'fontStyle',
  'textAlign',
  'whiteSpace',
  'marginLeftPx',
  'marginTopPx',
  'marginRightPx',
  'marginBottomPx',
  'paddingLeftPx',
  'paddingTopPx',
  'paddingRightPx',
  'paddingBottomPx',
];

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

function parseClassList(node) {
  const raw = collapseWhitespace(getAttr(node, 'class'));
  if (!raw) return [];
  return raw.split(' ').filter(Boolean);
}

function buildElementDescriptor(node, path) {
  return {
    path: String(path || ''),
    tag: String(node && node.tagName || '').toLowerCase(),
    id: String(getAttr(node, 'id') || ''),
    classes: parseClassList(node),
  };
}

function normalizeDeclarationList(declarations) {
  if (!Array.isArray(declarations)) return [];
  const out = [];
  for (let i = 0; i < declarations.length; i++) {
    const entry = declarations[i];
    const name = String(entry && entry.name || '').trim().toLowerCase();
    const value = String(entry && entry.value || '').trim();
    if (!name || !value) continue;
    let important = false;
    let finalValue = value;
    if (/!important\s*$/i.test(finalValue)) {
      important = true;
      finalValue = finalValue.replace(/!important\s*$/i, '').trim();
    }
    if (!finalValue) continue;
    out.push({ name, value: finalValue, important });
  }
  return out;
}

function parseDeclarationText(cssText) {
  const parts = String(cssText || '').split(';');
  const raw = [];
  for (let i = 0; i < parts.length; i++) {
    const entry = String(parts[i] || '').trim();
    if (!entry) continue;
    const split = entry.indexOf(':');
    if (split <= 0) continue;
    raw.push({
      name: entry.slice(0, split),
      value: entry.slice(split + 1),
    });
  }
  return normalizeDeclarationList(raw);
}

function splitSelectorList(selectorText) {
  return String(selectorText || '')
    .split(',')
    .map((part) => collapseWhitespace(part))
    .filter(Boolean);
}

function parseSelectorToken(token) {
  let raw = collapseWhitespace(token);
  if (!raw) return null;
  if (/[\[\]>+~]/.test(raw)) return null;
  raw = raw.replace(/:{1,2}[a-z0-9_-]+(?:\([^)]*\))?/gi, '');
  if (!raw) return null;

  let tag = '';
  let id = '';
  const classes = [];
  let cursor = 0;
  while (cursor < raw.length) {
    const ch = raw[cursor];
    if (ch === '#') {
      cursor += 1;
      let end = cursor;
      while (end < raw.length && /[a-zA-Z0-9_-]/.test(raw[end])) end += 1;
      id = raw.slice(cursor, end);
      cursor = end;
      continue;
    }
    if (ch === '.') {
      cursor += 1;
      let end = cursor;
      while (end < raw.length && /[a-zA-Z0-9_-]/.test(raw[end])) end += 1;
      const className = raw.slice(cursor, end);
      if (className) classes.push(className);
      cursor = end;
      continue;
    }
    let end = cursor;
    while (end < raw.length && /[a-zA-Z0-9_*-]/.test(raw[end])) end += 1;
    const tokenText = raw.slice(cursor, end);
    if (tokenText && tokenText !== '*') tag = tokenText.toLowerCase();
    cursor = end > cursor ? end : cursor + 1;
  }

  return { tag, id, classes };
}

function computeSpecificity(tokens) {
  let a = 0;
  let b = 0;
  let c = 0;
  for (let i = 0; i < tokens.length; i++) {
    const token = tokens[i];
    if (!token) continue;
    if (token.id) a += 1;
    b += Array.isArray(token.classes) ? token.classes.length : 0;
    if (token.tag) c += 1;
  }
  return [a, b, c];
}

function parseSelector(selectorText) {
  const rawTokens = collapseWhitespace(selectorText).split(' ').filter(Boolean);
  if (rawTokens.length <= 0) return null;
  const tokens = [];
  for (let i = 0; i < rawTokens.length; i++) {
    const token = parseSelectorToken(rawTokens[i]);
    if (!token) return null;
    tokens.push(token);
  }
  return {
    text: collapseWhitespace(selectorText),
    tokens,
    specificity: computeSpecificity(tokens),
  };
}

function parseStylesheetRules(cssText, startOrder = 0) {
  const css = String(cssText || '');
  const rules = [];
  let order = Number(startOrder || 0) | 0;
  let selectorStart = 0;
  let i = 0;

  while (i < css.length) {
    if (css[i] !== '{') {
      i += 1;
      continue;
    }

    const selectorText = css.slice(selectorStart, i).trim();
    let depth = 1;
    let bodyStart = i + 1;
    let j = i + 1;
    while (j < css.length && depth > 0) {
      if (css[j] === '{') depth += 1;
      else if (css[j] === '}') depth -= 1;
      j += 1;
    }

    const body = css.slice(bodyStart, Math.max(bodyStart, j - 1)).trim();
    selectorStart = j;
    i = j;

    if (!selectorText || selectorText.startsWith('@') || !body) continue;

    const declarations = parseDeclarationText(body);
    if (declarations.length <= 0) continue;

    const selectors = splitSelectorList(selectorText);
    for (let k = 0; k < selectors.length; k++) {
      const parsedSelector = parseSelector(selectors[k]);
      if (!parsedSelector) continue;
      rules.push({
        selectorText: parsedSelector.text,
        tokens: parsedSelector.tokens,
        specificity: parsedSelector.specificity,
        declarations,
        order,
      });
      order += 1;
    }
  }

  return { rules, nextOrder: order };
}

function matchesToken(element, token) {
  if (!element || !token) return false;
  if (token.tag && token.tag !== String(element.tag || '').toLowerCase()) return false;
  if (token.id && token.id !== String(element.id || '')) return false;
  const classes = Array.isArray(element.classes) ? element.classes : [];
  for (let i = 0; i < token.classes.length; i++) {
    if (!classes.includes(token.classes[i])) return false;
  }
  return true;
}

function matchesSelector(rule, element, ancestors) {
  if (!rule || !Array.isArray(rule.tokens) || rule.tokens.length <= 0) return false;
  if (!matchesToken(element, rule.tokens[rule.tokens.length - 1])) return false;
  let ancestorIndex = Array.isArray(ancestors) ? ancestors.length - 1 : -1;
  for (let tokenIndex = rule.tokens.length - 2; tokenIndex >= 0; tokenIndex--) {
    let found = false;
    while (ancestorIndex >= 0) {
      if (matchesToken(ancestors[ancestorIndex], rule.tokens[tokenIndex])) {
        found = true;
        ancestorIndex -= 1;
        break;
      }
      ancestorIndex -= 1;
    }
    if (!found) return false;
  }
  return true;
}

function compareSpecificity(a, b) {
  const left = Array.isArray(a) ? a : [0, 0, 0];
  const right = Array.isArray(b) ? b : [0, 0, 0];
  for (let i = 0; i < 3; i++) {
    if (left[i] > right[i]) return 1;
    if (left[i] < right[i]) return -1;
  }
  return 0;
}

function expandBoxValues(value) {
  const parts = collapseWhitespace(value).split(' ').filter(Boolean);
  if (parts.length <= 0) return null;
  if (parts.length === 1) return [parts[0], parts[0], parts[0], parts[0]];
  if (parts.length === 2) return [parts[0], parts[1], parts[0], parts[1]];
  if (parts.length === 3) return [parts[0], parts[1], parts[2], parts[1]];
  return [parts[0], parts[1], parts[2], parts[3]];
}

function parsePx(value) {
  const raw = String(value || '').trim().toLowerCase();
  if (raw === '0') return 0;
  const match = raw.match(/^(-?\d+(?:\.\d+)?)px$/);
  if (!match) return null;
  return Number(match[1]);
}

function rgbByteToHex(v) {
  const n = Math.max(0, Math.min(255, Math.round(Number(v || 0))));
  return n.toString(16).padStart(2, '0');
}

function normalizeColor(value) {
  const raw = collapseWhitespace(String(value || '')).toLowerCase();
  if (!raw) return null;
  if (raw === 'transparent') return 'transparent';
  if (/^#[0-9a-f]{3}$/i.test(raw)) {
    return `#${raw[1]}${raw[1]}${raw[2]}${raw[2]}${raw[3]}${raw[3]}`;
  }
  if (/^#[0-9a-f]{6}$/i.test(raw)) return raw;
  let match = raw.match(/^rgb\((\d+)\s*,\s*(\d+)\s*,\s*(\d+)\)$/i);
  if (match) {
    return `#${rgbByteToHex(match[1])}${rgbByteToHex(match[2])}${rgbByteToHex(match[3])}`;
  }
  match = raw.match(/^rgba\((\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*,\s*([0-9.]+)\)$/i);
  if (match) {
    if (Number(match[4]) <= 0) return 'transparent';
    return `#${rgbByteToHex(match[1])}${rgbByteToHex(match[2])}${rgbByteToHex(match[3])}`;
  }
  return raw;
}

function applyNormalizedField(style, field, value) {
  if (value == null) return false;
  style[field] = value;
  return true;
}

function applyDeclaration(style, name, value) {
  const prop = String(name || '').toLowerCase();
  const raw = String(value || '').trim();
  if (!prop || !raw) return false;
  if (prop === 'display') return applyNormalizedField(style, 'display', raw.toLowerCase());
  if (prop === 'color') return applyNormalizedField(style, 'color', normalizeColor(raw));
  if (prop === 'background-color') return applyNormalizedField(style, 'backgroundColor', normalizeColor(raw));
  if (prop === 'background') return applyNormalizedField(style, 'backgroundColor', normalizeColor(raw));
  if (prop === 'font-size') return applyNormalizedField(style, 'fontSizePx', parsePx(raw));
  if (prop === 'font-weight') return applyNormalizedField(style, 'fontWeight', raw.toLowerCase());
  if (prop === 'font-style') return applyNormalizedField(style, 'fontStyle', raw.toLowerCase());
  if (prop === 'text-align') return applyNormalizedField(style, 'textAlign', raw.toLowerCase());
  if (prop === 'white-space') return applyNormalizedField(style, 'whiteSpace', raw.toLowerCase());
  if (prop === 'margin-left') return applyNormalizedField(style, 'marginLeftPx', parsePx(raw));
  if (prop === 'margin-top') return applyNormalizedField(style, 'marginTopPx', parsePx(raw));
  if (prop === 'margin-right') return applyNormalizedField(style, 'marginRightPx', parsePx(raw));
  if (prop === 'margin-bottom') return applyNormalizedField(style, 'marginBottomPx', parsePx(raw));
  if (prop === 'padding-left') return applyNormalizedField(style, 'paddingLeftPx', parsePx(raw));
  if (prop === 'padding-top') return applyNormalizedField(style, 'paddingTopPx', parsePx(raw));
  if (prop === 'padding-right') return applyNormalizedField(style, 'paddingRightPx', parsePx(raw));
  if (prop === 'padding-bottom') return applyNormalizedField(style, 'paddingBottomPx', parsePx(raw));
  if (prop === 'margin') {
    const values = expandBoxValues(raw);
    if (!values) return false;
    const top = parsePx(values[0]);
    const right = parsePx(values[1]);
    const bottom = parsePx(values[2]);
    const left = parsePx(values[3]);
    style.marginTopPx = top == null ? style.marginTopPx : top;
    style.marginRightPx = right == null ? style.marginRightPx : right;
    style.marginBottomPx = bottom == null ? style.marginBottomPx : bottom;
    style.marginLeftPx = left == null ? style.marginLeftPx : left;
    return true;
  }
  if (prop === 'padding') {
    const values = expandBoxValues(raw);
    if (!values) return false;
    const top = parsePx(values[0]);
    const right = parsePx(values[1]);
    const bottom = parsePx(values[2]);
    const left = parsePx(values[3]);
    style.paddingTopPx = top == null ? style.paddingTopPx : top;
    style.paddingRightPx = right == null ? style.paddingRightPx : right;
    style.paddingBottomPx = bottom == null ? style.paddingBottomPx : bottom;
    style.paddingLeftPx = left == null ? style.paddingLeftPx : left;
    return true;
  }
  return false;
}

function applyCascadeDeclaration(style, winners, declaration, meta) {
  const key = String(declaration && declaration.name || '').toLowerCase();
  if (!key) return;
  const prev = winners[key] || null;
  const next = {
    important: meta.important === true,
    specificity: Array.isArray(meta.specificity) ? meta.specificity : [0, 0, 0],
    order: Number(meta.order || 0),
    value: String(declaration.value || ''),
    selectorText: String(meta.selectorText || ''),
  };

  let take = false;
  if (!prev) {
    take = true;
  } else if (prev.important !== next.important) {
    take = next.important;
  } else {
    const cmp = compareSpecificity(next.specificity, prev.specificity);
    if (cmp > 0) take = true;
    else if (cmp === 0 && next.order >= prev.order) take = true;
  }

  if (take) winners[key] = next;
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

function buildCssContext(cssObjects) {
  const byPath = Object.create(null);
  const stylesheets = [];
  const rules = [];
  let order = 0;

  for (let i = 0; i < cssObjects.length; i++) {
    const entry = cssObjects[i];
    const path = String(entry && entry.path || '');
    const style = entry && entry.style || null;
    const kind = String(style && style.kind || 'unknown');
    if (kind === 'inline' && path) {
      if (!Array.isArray(byPath[path])) byPath[path] = [];
      byPath[path].push(entry);
      continue;
    }
    stylesheets.push(entry);
    if (kind === 'stylesheet') {
      const parsedRules = parseStylesheetRules(style && style.css || '', order);
      order = parsedRules.nextOrder;
      for (let j = 0; j < parsedRules.rules.length; j++) {
        rules.push(parsedRules.rules[j]);
      }
    }
  }

  return { byPath, stylesheets, rules };
}

function compactStyleEntry(style) {
  const entry = Object.create(null);
  for (let i = 0; i < COMPACT_STYLE_FIELDS.length; i++) {
    const key = COMPACT_STYLE_FIELDS[i];
    entry[key] = style && style[key] != null ? style[key] : null;
  }
  return entry;
}

function compactStyleKey(style) {
  let key = '';
  for (let i = 0; i < COMPACT_STYLE_FIELDS.length; i++) {
    const field = COMPACT_STYLE_FIELDS[i];
    const value = style && style[field] != null ? style[field] : '';
    if (i > 0) key += '\x1f';
    key += String(value);
  }
  return key;
}

function walkElementTree(node, path, ancestors, visit) {
  if (!node || typeof node !== 'object') return;
  if (!isElement(node)) {
    const kids = Array.isArray(node.childNodes) ? node.childNodes : [];
    for (let i = 0; i < kids.length; i++) {
      walkElementTree(kids[i], `${path}.${i}`, ancestors, visit);
    }
    return;
  }

  visit(node, path, ancestors);
  const nextAncestors = ancestors.concat([{ node, path }]);
  const kids = Array.isArray(node.childNodes) ? node.childNodes : [];
  for (let i = 0; i < kids.length; i++) {
    walkElementTree(kids[i], `${path}.${i}`, nextAncestors, visit);
  }
}

export function buildCssStyleRefIndex(doc) {
  const cssObjects = extractCssObjects(doc);
  const context = buildCssContext(cssObjects);
  const cssSection = {
    byPath: context.byPath,
    stylesheets: context.stylesheets,
    rules: context.rules,
  };
  const styleTable = [];
  const styleRefByKey = Object.create(null);
  const nodeStyleRefs = [];
  let inlineStyleCount = 0;
  let elementCount = 0;

  walkElementTree(doc, 'root', [], (node, path, ancestors) => {
    const parent = ancestors.length > 0 ? ancestors[ancestors.length - 1] : null;
    const parentStyle = parent && parent.node && parent.node.__trueosComputedStyle
      ? parent.node.__trueosComputedStyle
      : null;
    const computedStyle = resolveNodeStyle(node, path, cssSection, ancestors, parentStyle);
    if (!computedStyle) return;

    const styleKey = compactStyleKey(computedStyle);
    let styleRef = styleRefByKey[styleKey];
    if (styleRef == null) {
      styleRef = styleTable.length;
      styleRefByKey[styleKey] = styleRef;
      styleTable.push(compactStyleEntry(computedStyle));
    }

    if (computedStyle.source && computedStyle.source.inline) {
      inlineStyleCount += 1;
    }
    elementCount += 1;
    node.__trueosComputedStyle = computedStyle;
    node.__trueosStyleRef = styleRef;
    node.__trueosNodePath = path;
    nodeStyleRefs.push({
      path,
      styleRef,
    });
  });

  return {
    styleTable,
    nodeStyleRefs,
    styleSlotCount: styleTable.length,
    nodeRefCount: nodeStyleRefs.length,
    inlineStyleCount,
    stylesheetCount: context.stylesheets.length,
    ruleCount: context.rules.length,
    elementCount,
  };
}

export function resolveNodeStyle(node, path, cssSection, ancestors, parentStyle = null) {
  if (!isElement(node)) return null;

  const element = buildElementDescriptor(node, path);
  const ancestorDescriptors = Array.isArray(ancestors)
    ? ancestors.map((entry) => buildElementDescriptor(entry && entry.node, entry && entry.path)).filter((entry) => !!entry.tag)
    : [];
  const style = createComputedStyle(element.tag, path, parentStyle);
  const winners = Object.create(null);
  const matchedRules = [];
  const rules = Array.isArray(cssSection && cssSection.rules) ? cssSection.rules : [];
  for (let i = 0; i < rules.length; i++) {
    const rule = rules[i];
    if (!matchesSelector(rule, element, ancestorDescriptors)) continue;
    matchedRules.push(rule.selectorText);
    const declarations = normalizeDeclarationList(rule.declarations);
    for (let j = 0; j < declarations.length; j++) {
      applyCascadeDeclaration(style, winners, declarations[j], {
        important: declarations[j].important,
        specificity: rule.specificity,
        order: rule.order,
        selectorText: rule.selectorText,
      });
    }
  }

  const inlineEntries = cssSection && cssSection.byPath && Array.isArray(cssSection.byPath[path]) ? cssSection.byPath[path] : [];
  for (let i = 0; i < inlineEntries.length; i++) {
    const declarations = normalizeDeclarationList(inlineEntries[i] && inlineEntries[i].style && inlineEntries[i].style.declarations);
    for (let j = 0; j < declarations.length; j++) {
      applyCascadeDeclaration(style, winners, declarations[j], {
        important: declarations[j].important,
        specificity: [1, 0, 0],
        order: Number.MAX_SAFE_INTEGER - inlineEntries.length + i,
        selectorText: 'style="…"',
      });
    }
  }

  const winnerKeys = Object.keys(winners);
  for (let i = 0; i < winnerKeys.length; i++) {
    const key = winnerKeys[i];
    applyDeclaration(style, key, winners[key].value);
  }

  style.source = {
    matchedRules,
    inline: inlineEntries.length > 0,
  };
  return style;
}

export function extractCssObjects(doc) {
  const cssObjects = [];
  const kids = Array.isArray(doc && doc.childNodes) ? doc.childNodes : [];
  for (let i = 0; i < kids.length; i++) {
    collectCssObjects(kids[i], `root.${i}`, cssObjects);
  }
  return cssObjects;
}

export function extractCssSection(doc) {
  const cssObjects = extractCssObjects(doc);
  const context = buildCssContext(cssObjects);
  const rows = [
    { depth: 0, text: '' },
    { depth: 0, text: '/* CSS */' },
  ];

  if (cssObjects.length <= 0) {
    rows.push({ depth: 0, text: '(no styles found)' });
    return {
      cssObjects,
      rows,
      byPath: context.byPath,
      stylesheets: context.stylesheets,
      rules: context.rules,
    };
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

  return {
    cssObjects,
    rows,
    byPath: context.byPath,
    stylesheets: context.stylesheets,
    rules: context.rules,
  };
}

export function extractCssRows(doc) {
  const section = extractCssSection(doc);
  return {
    cssObjects: section.cssObjects,
    rows: section.rows,
  };
}