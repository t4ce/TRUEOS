// Minimal HTML "UA-ish" defaults for the demo.
// These tags are treated as block-ish containers in our simplified render tree.
function uniqueTags(tags) {
    // Preserve first occurrence order.
    return Array.from(new Set(tags.map((t) => t.toLowerCase())));
}
// 4.4 Grouping content (WHATWG HTML)
export const GROUPING_CONTENT_TAGS = uniqueTags([
    'p',
    'hr',
    'pre',
    'blockquote',
    'ol',
    'ul',
    'menu',
    'li',
    'dl',
    'dt',
    'dd',
    'figure',
    'figcaption',
    'main',
    'search',
    'div',
]);
// 4.5 Text-level semantics (WHATWG HTML)
// NOTE: These are inline-ish for our renderer (we generally fold them into text).
export const TEXT_LEVEL_SEMANTICS_TAGS = uniqueTags([
    'a',
    'em',
    'strong',
    'small',
    's',
    'cite',
    'q',
    'dfn',
    'abbr',
    'ruby',
    'rt',
    'rp',
    'data',
    'time',
    'code',
    'var',
    'samp',
    'kbd',
    'sub',
    'sup',
    'i',
    'b',
    'u',
    'mark',
    'bdi',
    'bdo',
    'span',
    'br',
    'wbr',
]);
// 4.8 Embedded content (WHATWG HTML)
export const EMBEDDED_CONTENT_TAGS = uniqueTags([
    'img',
]);
// 4.9 Tabular data (WHATWG HTML)
export const TABULAR_DATA_TAGS = uniqueTags([
    'table',
    'caption',
    'colgroup',
    'col',
    'tbody',
    'thead',
    'tfoot',
    'tr',
    'td',
    'th',
]);
export const BLOCK_TAGS = new Set(uniqueTags([
    // Document structure
    'html',
    'body',
    // Sections / semantic layout
    'main',
    'section',
    'article',
    'header',
    'footer',
    'nav',
    'aside',
    'search',
    // Grouping content
    ...GROUPING_CONTENT_TAGS,
    'address',
    // Headings
    'h1',
    'h2',
    'h3',
    'h4',
    'h5',
    'h6',
    // Lists
    // (Already covered by GROUPING_CONTENT_TAGS)
    // Figures
    // (Already covered by GROUPING_CONTENT_TAGS)
    // Forms
    'form',
    'label',
    'fieldset',
    'legend',
    'button',
    'input',
    'textarea',
    'select',
    'option',
    'optgroup',
    'output',
    // Simple value widgets
    'progress',
    'meter',
    'slider',
    'number',
    'color',
    // Composite widgets
    'search',
    // Disclosure
    'details',
    'summary',
    'stub',
    // Popups
    'dialog',
    // 4.9 Tabular data
    ...TABULAR_DATA_TAGS,
    // Embedded content
    ...EMBEDDED_CONTENT_TAGS,
    // Inline graphics / drawing
    'svg',
    'canvas',
    // Nested documents
    'iframe',
]));
