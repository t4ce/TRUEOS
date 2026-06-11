const SVG_NS = 'http://www.w3.org/2000/svg';
const HTML_NS = 'http://www.w3.org/1999/xhtml';

function escapeText(value) {
  return String(value)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;');
}

function escapeAttr(value) {
  return escapeText(value).replace(/"/g, '&quot;');
}

function localName(name) {
  const value = String(name || '');
  const idx = value.indexOf(':');
  return idx >= 0 ? value.slice(idx + 1) : value;
}

class LiteNode {
  constructor(ownerDocument, nodeName, nodeType) {
    this.ownerDocument = ownerDocument || null;
    this.nodeName = nodeName;
    this.nodeType = nodeType;
    this.parentNode = null;
    this.childNodes = [];
  }

  get firstChild() {
    return this.childNodes[0] || null;
  }

  get lastChild() {
    return this.childNodes[this.childNodes.length - 1] || null;
  }

  get children() {
    return this.childNodes.filter((node) => node && node.nodeType === 1);
  }

  get firstElementChild() {
    return this.children[0] || null;
  }

  appendChild(node) {
    if (!node) return node;
    if (node.nodeType === 11) {
      while (node.firstChild) this.appendChild(node.firstChild);
      return node;
    }
    if (node.parentNode) node.parentNode.removeChild(node);
    node.parentNode = this;
    this.childNodes.push(node);
    return node;
  }

  insertBefore(node, before) {
    if (!before) return this.appendChild(node);
    const index = this.childNodes.indexOf(before);
    if (index < 0) return this.appendChild(node);
    if (node.nodeType === 11) {
      const nodes = node.childNodes.slice();
      for (const child of nodes) this.insertBefore(child, before);
      return node;
    }
    if (node.parentNode) node.parentNode.removeChild(node);
    node.parentNode = this;
    this.childNodes.splice(index, 0, node);
    return node;
  }

  removeChild(node) {
    const index = this.childNodes.indexOf(node);
    if (index >= 0) {
      this.childNodes.splice(index, 1);
      node.parentNode = null;
    }
    return node;
  }

  replaceChild(node, oldNode) {
    this.insertBefore(node, oldNode);
    this.removeChild(oldNode);
    return oldNode;
  }

  cloneNode(deep = false) {
    const clone = new LiteNode(this.ownerDocument, this.nodeName, this.nodeType);
    if (deep) {
      for (const child of this.childNodes) clone.appendChild(child.cloneNode(true));
    }
    return clone;
  }

  get textContent() {
    return this.childNodes.map((node) => node.textContent).join('');
  }

  set textContent(value) {
    this.childNodes.length = 0;
    if (value != null && String(value).length > 0) {
      this.appendChild(this.ownerDocument.createTextNode(String(value)));
    }
  }

  contains(node) {
    for (let current = node; current; current = current.parentNode) {
      if (current === this) return true;
    }
    return false;
  }
}

export class TextNode extends LiteNode {
  constructor(ownerDocument, text = '') {
    super(ownerDocument, '#text', 3);
    this.data = String(text);
  }

  get textContent() {
    return this.data;
  }

  set textContent(value) {
    this.data = String(value ?? '');
  }

  cloneNode() {
    return new TextNode(this.ownerDocument, this.data);
  }

  get outerHTML() {
    return escapeText(this.data);
  }
}

export class Element extends LiteNode {
  constructor(ownerDocument, tagName, namespaceURI = SVG_NS) {
    super(ownerDocument, localName(tagName), 1);
    this.tagName = localName(tagName);
    this.namespaceURI = namespaceURI;
    this.attributes = Object.create(null);
    this.instance = null;
  }

  setAttribute(name, value) {
    this.attributes[String(name)] = String(value);
  }

  setAttributeNS(_ns, name, value) {
    this.setAttribute(name, value);
  }

  getAttribute(name) {
    const key = String(name);
    return Object.prototype.hasOwnProperty.call(this.attributes, key) ? this.attributes[key] : null;
  }

  hasAttribute(name) {
    return Object.prototype.hasOwnProperty.call(this.attributes, String(name));
  }

  removeAttribute(name) {
    delete this.attributes[String(name)];
  }

  removeAttributeNS(_ns, name) {
    this.removeAttribute(name);
  }

  cloneNode(deep = false) {
    const clone = new Element(this.ownerDocument, this.tagName, this.namespaceURI);
    for (const key of Object.keys(this.attributes)) clone.attributes[key] = this.attributes[key];
    if (deep) {
      for (const child of this.childNodes) clone.appendChild(child.cloneNode(true));
    }
    return clone;
  }

  matches(selector) {
    const query = String(selector || '').trim();
    if (!query) return false;
    if (query[0] === '#') return this.getAttribute('id') === query.slice(1);
    if (query[0] === '.') {
      return String(this.getAttribute('class') || '').split(/\s+/).includes(query.slice(1));
    }
    return this.tagName.toLowerCase() === query.toLowerCase();
  }

  querySelector(selector) {
    return this.querySelectorAll(selector)[0] || null;
  }

  querySelectorAll(selector) {
    const out = [];
    const visit = (node) => {
      for (const child of node.children || []) {
        if (child.matches(selector)) out.push(child);
        visit(child);
      }
    };
    visit(this);
    return out;
  }

  get innerHTML() {
    return this.childNodes.map((node) => node.outerHTML).join('');
  }

  set innerHTML(_value) {
    this.childNodes.length = 0;
  }

  get outerHTML() {
    const attrs = Object.keys(this.attributes)
      .map((key) => ` ${key}="${escapeAttr(this.attributes[key])}"`)
      .join('');
    return `<${this.tagName}${attrs}>${this.innerHTML}</${this.tagName}>`;
  }
}

export class DocumentFragment extends LiteNode {
  constructor(ownerDocument) {
    super(ownerDocument, '#document-fragment', 11);
  }

  cloneNode(deep = false) {
    const clone = new DocumentFragment(this.ownerDocument);
    if (deep) {
      for (const child of this.childNodes) clone.appendChild(child.cloneNode(true));
    }
    return clone;
  }

  get outerHTML() {
    return this.childNodes.map((node) => node.outerHTML).join('');
  }
}

export class Document extends LiteNode {
  constructor(windowRef = null) {
    super(null, '#document', 9);
    this.ownerDocument = this;
    this.defaultView = windowRef;
    this.documentElement = this.createElementNS(SVG_NS, 'svg');
    this.body = null;
    this.appendChild(this.documentElement);
  }

  createElementNS(ns, name) {
    return new Element(this, name, ns || SVG_NS);
  }

  createElement(name) {
    return new Element(this, name, HTML_NS);
  }

  createDocumentFragment() {
    return new DocumentFragment(this);
  }

  createTextNode(text) {
    return new TextNode(this, text);
  }

  querySelector(selector) {
    return this.documentElement.querySelector(selector);
  }

  querySelectorAll(selector) {
    return this.documentElement.querySelectorAll(selector);
  }
}

export class Event {
  constructor(type, init = {}) {
    this.type = String(type || '');
    this.detail = init.detail;
  }
}

export class CustomEvent extends Event {}

export function createSVGWindow() {
  const win = {
    Date,
    Event,
    CustomEvent,
    Node: LiteNode,
    SVGElement: Element,
    pageXOffset: 0,
    pageYOffset: 0,
    performance: { now: () => Date.now() },
    requestAnimationFrame: (fn) => setTimeout(() => fn(Date.now()), 16),
    cancelAnimationFrame: (id) => clearTimeout(id),
    getComputedStyle: () => ({ getPropertyValue: () => '' }),
    Image: class Image {},
  };
  const doc = new Document(win);
  win.document = doc;
  return win;
}

export function installSVGWindow(target = globalThis) {
  const win = createSVGWindow();
  target.window = win;
  target.document = win.document;
  return win;
}

export default {
  createSVGWindow,
  installSVGWindow,
  Document,
  Element,
  DocumentFragment,
  TextNode,
  Event,
  CustomEvent,
};
