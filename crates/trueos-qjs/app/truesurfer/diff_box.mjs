const htmlByUrl = new Map();
// surfer internal diff box, for now just should md5 for example to detect a basic
// has not changed, then we could later atleast play a "quasi refresh, effect, as it would be honest still"
function safeString(value) {
  if (typeof value === 'string') {
    return value;
  }
  if (value === null || value === undefined) {
    return '';
  }
  return String(value);
}

function log(line) {
  if (typeof console !== 'undefined' && console && typeof console.log === 'function') {
    console.log(line);
  }
}

export function passHtmlThroughDiffBox(html, meta = {}) {
  const nextHtml = safeString(html);
  const url = safeString(meta.url);
  const previousHtml = url ? htmlByUrl.get(url) : undefined;

  if (url) {
    htmlByUrl.set(url, nextHtml);
  }

  return {
    html: nextHtml,
    url,
    changed: previousHtml !== undefined && previousHtml !== nextHtml,
  };
}
