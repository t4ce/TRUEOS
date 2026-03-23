import parse5 from 'parse5';
import { passHtmlThroughDiffBox } from '/qjs/truesurfer/diff_box.mjs';

const root = globalThis;
const browserId = Number(root.__trueosTruesurferBrowserId || 0);
const LOG_REAR_ENABLE = true;
const LOG_PARSE_ENABLE = true;

function log(line) {
  if (typeof console !== 'undefined' && console && typeof console.log === 'function') {
    console.log(line);
  }
}

function splitLines(source) {
  if (!source) {
    return [''];
  }
  return String(source).split(/\r\n|\n|\r/);
}

function rearLines(source, count) {
  const lines = splitLines(source);
  const tailCount = Math.max(0, count | 0);
  if (tailCount === 0) {
    return [];
  }
  return lines.slice(Math.max(0, lines.length - tailCount));
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

function previewLine(line, maxChars) {
  const text = safeString(line);
  if (text.length <= maxChars) {
    return text;
  }
  return `${text.slice(0, maxChars)}...`;
}

function countTreeNodes(node) {
  if (!node || typeof node !== 'object') {
    return 0;
  }
  let total = 1;
  const children = Array.isArray(node.childNodes) ? node.childNodes : [];
  for (const child of children) {
    total += countTreeNodes(child);
  }
  return total;
}

function summarizeDocument(doc) {
  const childNodes = Array.isArray(doc && doc.childNodes) ? doc.childNodes : [];
  return {
    roots: childNodes.length,
    nodes: countTreeNodes(doc),
  };
}

function logRearLines(source) {
  if (!LOG_REAR_ENABLE) {
    return;
  }
  const tail = rearLines(source, 3);
  log(`[truesurfer rear] browser=${browserId} lines=${tail.length}`);
  for (let index = 0; index < tail.length; index += 1) {
    log(
      `[truesurfer rear] browser=${browserId} [${index + 1}] ${previewLine(
        tail[index],
        180,
      )}`,
    );
  }
}

function parseHtmlDocument(source, url) {
  if (LOG_PARSE_ENABLE) {
    log(`[truesurfer parse5] browser=${browserId} enter bytes=${source.length} url=${url}`);
  }
  const startedAt = Date.now();
  const doc = parse5.parse(source);
  const summary = summarizeDocument(doc);
  const elapsedMs = Date.now() - startedAt;
  if (LOG_PARSE_ENABLE) {
    log(
      `[truesurfer parse5] browser=${browserId} ok ms=${elapsedMs} roots=${summary.roots} nodes=${summary.nodes}`,
    );
  }
  return {
    doc,
    elapsedMs,
    summary,
  };
}

function setHtml(nextHtml, meta) {
  const forwarded = passHtmlThroughDiffBox(nextHtml, meta || {});
  const html = forwarded.html;
  const url = forwarded.url;
  const lines = splitLines(html);

  log(
    `[surfer ok] browser=${browserId} html_bytes=${html.length} lines=${lines.length} url=${url}`,
  );
  logRearLines(html);

  try {
    const parsed = parseHtmlDocument(html, url);
    return {
      ok: 1,
      bytes: html.length,
      lines: lines.length,
      parseMs: parsed.elapsedMs,
      roots: parsed.summary.roots,
      nodes: parsed.summary.nodes,
    };
  } catch (error) {
    const message =
      error && error.stack ? String(error.stack) : error ? String(error) : 'unknown parse5 error';
    log(`[truesurfer parse5] browser=${browserId} fail error=${message}`);
    return {
      ok: 0,
      bytes: html.length,
      lines: lines.length,
      error: message,
    };
  }
}

root.__trueosTruesurfer = {
  setHtml,
};
root.__trueosTruesurferReady = 1;

log(`[truesurfer bootstrap] browser=${browserId} ready`);
