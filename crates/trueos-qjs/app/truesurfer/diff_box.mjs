const htmlByUrl = new Map();

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

  if (!url || previousHtml === undefined || previousHtml === nextHtml) {
    log(`[diff box] Diff Box New url=${url}`);
  } else {
    log(`[diff box] Diff Box Difference url=${url}`);
  }

  if (url) {
    htmlByUrl.set(url, nextHtml);
  }

  return {
    html: nextHtml,
    url,
    changed: previousHtml !== undefined && previousHtml !== nextHtml,
  };
}
