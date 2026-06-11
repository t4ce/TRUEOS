export function isElement(node) {
  return Boolean(node && typeof node === 'object' && node.nodeName && node.nodeName !== '#text');
}

export function isText(node) {
  return Boolean(node && node.nodeName === '#text');
}

export function getBody(doc) {
  if (!doc) return undefined;
  if ((doc.tagName ?? doc.nodeName) === 'body') return doc;

  const html = (doc.childNodes ?? []).find((node) => isElement(node) && node.tagName === 'html');
  if (!html) return doc;

  return (html.childNodes ?? []).find((node) => isElement(node) && node.tagName === 'body') ?? html;
}

export function attrsToObject(node) {
  const attrs = node?.attrs;
  if (!Array.isArray(attrs) || attrs.length === 0) return {};

  const out = {};
  for (const attr of attrs) {
    if (attr && typeof attr.name === 'string') out[attr.name] = String(attr.value ?? '');
  }
  return out;
}

export function normalizeWhitespace(text) {
  let out = '';
  let inWs = false;

  for (const ch of String(text ?? '')) {
    if (/\s/.test(ch)) {
      if (!inWs) out += ' ';
      inWs = true;
    } else {
      out += ch;
      inWs = false;
    }
  }

  return out.trim();
}

export function extractText(node) {
  if (isText(node)) return node.value ?? '';
  if (!isElement(node)) return '';
  return (node.childNodes ?? []).map(extractText).join(' ');
}

export function directChildElements(node, tagName) {
  const wanted = tagName == null ? null : String(tagName).toLowerCase();
  return (node?.childNodes ?? []).filter((child) => {
    if (!isElement(child)) return false;
    if (wanted == null) return true;
    return String(child.tagName ?? child.nodeName).toLowerCase() === wanted;
  });
}

