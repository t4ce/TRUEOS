function uniqueTags(tags) {
  return Array.from(new Set(tags.map((t) => String(t || '').toLowerCase())));
}

export const GROUPING_CONTENT_TAGS = uniqueTags([
  'p', 'hr', 'pre', 'blockquote', 'ol', 'ul', 'menu', 'li',
  'dl', 'dt', 'dd', 'figure', 'figcaption', 'main', 'search', 'div',
]);

export const TEXT_LEVEL_SEMANTICS_TAGS = uniqueTags([
  'a', 'em', 'strong', 'small', 's', 'cite', 'q', 'dfn', 'abbr', 'ruby',
  'rt', 'rp', 'data', 'time', 'code', 'var', 'samp', 'kbd', 'sub', 'sup',
  'i', 'b', 'u', 'mark', 'bdi', 'bdo', 'span', 'br', 'wbr',
]);

export const EMBEDDED_CONTENT_TAGS = uniqueTags([
  'img',
]);

export const TABULAR_DATA_TAGS = uniqueTags([
  'table', 'caption', 'colgroup', 'col', 'tbody', 'thead', 'tfoot',
  'tr', 'td', 'th',
]);

export const BLOCK_TAGS = new Set(uniqueTags([
  'html', 'body', 'main', 'section', 'article', 'header', 'footer', 'nav',
  'aside', 'search', ...GROUPING_CONTENT_TAGS, 'address', 'h1', 'h2', 'h3',
  'h4', 'h5', 'h6', 'form', 'label', 'fieldset', 'legend', 'button', 'input',
  'textarea', 'select', 'option', 'optgroup', 'output', 'progress', 'meter',
  'slider', 'number', 'color', 'search', 'details', 'summary', 'stub',
  'dialog', ...TABULAR_DATA_TAGS, ...EMBEDDED_CONTENT_TAGS, 'svg', 'canvas',
  'iframe',
]));
 
export const SUPPORTED_STYLE_TAGS = new Set(uniqueTags([
  ...BLOCK_TAGS,
  ...TEXT_LEVEL_SEMANTICS_TAGS,
  ...EMBEDDED_CONTENT_TAGS,
]));
