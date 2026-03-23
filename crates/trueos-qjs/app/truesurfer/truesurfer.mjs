import parse5 from 'parse5';
import { Worker } from 'node:worker_threads';
import { passHtmlThroughDiffBox } from '/qjs/truesurfer/diff_box.mjs';

const root = globalThis;
const browserId = Number(root.__trueosTruesurferBrowserId || 0);
const BODY_CLOSE_TAG = '</body>';
const BRANCH_PARSE5_BODY_WORKER_SOURCE = `
import parse5 from 'parse5';
import { parentPort } from 'node:worker_threads';

function safeString(value) {
  if (typeof value === 'string') {
    return value;
  }
  if (value === null || value === undefined) {
    return '';
  }
  return String(value);
}

function parseBodyBranch(bodyHtml) {
  const startedAt = Date.now();
  const fragment = parse5.parseFragment(bodyHtml);
  const elapsedMs = Date.now() - startedAt;
  return {
    ok: 1,
    parseMs: elapsedMs,
    topLevelCount: Array.isArray(fragment.childNodes) ? fragment.childNodes.length : 0,
  };
}

parentPort.onMessage((raw) => {
  try {
    const message = typeof raw === 'string' ? JSON.parse(raw) : {};
    const result = parseBodyBranch(safeString(message && message.bodyHtml));
    parentPort.postMessage(JSON.stringify(result));
  } catch (error) {
    const message =
      error && error.stack ? String(error.stack) : error ? String(error) : 'body branch parse failed';
    parentPort.postMessage(JSON.stringify({ ok: 0, error: message, parseMs: 0, topLevelCount: 0 }));
  }
});
`;

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

function parseJsonString(raw) {
  if (typeof raw !== 'string' || !raw) {
    return null;
  }
  try {
    return JSON.parse(raw);
  } catch (_error) {
    return null;
  }
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

function parseHtmlDocument(source) {
  const startedAt = Date.now();
  const doc = parse5.parse(source);
  const elapsedMs = Date.now() - startedAt;
  return {
    elapsedMs,
    title: extractDocumentTitle(doc),
  };
}

function splitDocumentForBranchParse(source) {
  const html = safeString(source);
  const lower = html.toLowerCase();
  const bodyOpenStart = lower.indexOf('<body');
  if (bodyOpenStart < 0) {
    return null;
  }

  const bodyOpenEnd = html.indexOf('>', bodyOpenStart);
  if (bodyOpenEnd < 0) {
    return null;
  }

  const bodyCloseStart = lower.lastIndexOf(BODY_CLOSE_TAG);
  if (bodyCloseStart < 0 || bodyCloseStart < bodyOpenEnd) {
    return null;
  }

  const beforeBody = html.slice(0, bodyOpenStart);
  const bodyOpenTag = html.slice(bodyOpenStart, bodyOpenEnd + 1);
  const bodyHtml = html.slice(bodyOpenEnd + 1, bodyCloseStart);
  const afterBody = html.slice(bodyCloseStart + BODY_CLOSE_TAG.length);

  return {
    bodyHtml,
    shellHtml: `${beforeBody}${bodyOpenTag}${BODY_CLOSE_TAG}${afterBody}`,
  };
}

function parseBodyBranchLocal(bodyHtml) {
  const startedAt = Date.now();
  const fragment = parse5.parseFragment(bodyHtml);
  const elapsedMs = Date.now() - startedAt;
  return {
    ok: 1,
    parseMs: elapsedMs,
    topLevelCount: Array.isArray(fragment.childNodes) ? fragment.childNodes.length : 0,
  };
}

function parseBodyBranchInWorker(bodyHtml) {
  return new Promise((resolve) => {
    if (typeof Worker !== 'function') {
      resolve(parseBodyBranchLocal(bodyHtml));
      return;
    }

    let settled = false;
    let worker = null;
    const finish = (result) => {
      if (settled) {
        return;
      }
      settled = true;
      try {
        if (worker && typeof worker.terminate === 'function') {
          worker.terminate();
        }
      } catch (_error) {}
      resolve(result);
    };

    try {
      worker = new Worker(BRANCH_PARSE5_BODY_WORKER_SOURCE);
      worker.onMessage((raw) => {
        const message = parseJsonString(raw);
        if (!message || !message.ok) {
          finish(parseBodyBranchLocal(bodyHtml));
          return;
        }
        finish({
          ok: 1,
          parseMs: Number(message.parseMs || 0) | 0,
          topLevelCount: Number(message.topLevelCount || 0) | 0,
        });
      });
      worker.postMessage(JSON.stringify({ bodyHtml }));
    } catch (_error) {
      finish(parseBodyBranchLocal(bodyHtml));
    }
  });
}

async function branchParse5Document(source) {
  const split = splitDocumentForBranchParse(source);
  if (!split) {
    const parsed = parseHtmlDocument(source);
    return {
      title: parsed.title,
      parseMs: parsed.elapsedMs,
      shellParseMs: parsed.elapsedMs,
      bodyParseMs: 0,
      bodyTopLevelCount: 0,
      mode: 'full-document',
    };
  }

  const bodyPromise = parseBodyBranchInWorker(split.bodyHtml);
  const shellStartedAt = Date.now();
  const shellDoc = parse5.parse(split.shellHtml);
  const shellParseMs = Date.now() - shellStartedAt;
  const bodyResult = await bodyPromise;

  return {
    title: extractDocumentTitle(shellDoc),
    parseMs: shellParseMs + Number(bodyResult.parseMs || 0),
    shellParseMs,
    bodyParseMs: Number(bodyResult.parseMs || 0),
    bodyTopLevelCount: Number(bodyResult.topLevelCount || 0),
    mode: 'branch-parse5',
  };
}

async function setHtml(nextHtml, meta) {
  const forwarded = passHtmlThroughDiffBox(nextHtml, meta || {});
  const html = forwarded.html;
  const url = forwarded.url;
  const lines = countLines(html);

  try {
    const parsed = await branchParse5Document(html);
    log(
      `[truesurfer branch_parse5] browser=${browserId} title=${parsed.title} shell_ms=${parsed.shellParseMs} body_ms=${parsed.bodyParseMs} body_top=${parsed.bodyTopLevelCount} mode=${parsed.mode} url=${url}`,
    );
    return {
      ok: 1,
      bytes: html.length,
      lines,
      parseMs: parsed.parseMs,
      title: parsed.title,
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
