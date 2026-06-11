/*
Truesurfer pipeline bridge:

html shack -> N-Browsers (Truesurfers)

In Truesurfer:
- Parse5 + CSS parse, JavaScript in parallel isolated
- Lightning CSS enrichment of the document and DOM subset
- Render-tree extraction for the kernel renderer

Runtime surface:
- Raw Parse5/Lightning extraction state for the kernel renderer
- No migrated control layer and no hosted handoff
*/

const root = globalThis;
const browserId = Number(root.__trueosTruesurferBrowserId || 0);
const TRUESURFER_MODULE_BASE = typeof import.meta === 'object' && import.meta && typeof import.meta.url === 'string'
  ? String(import.meta.url)
  : '/qjs/truesurfer/truesurfer.mjs';
const TRUESURFER_MAX_SCENE_IMAGES = 5;

let truesurferSubsetProfile = null;
let extractDocumentArtifactsFn = null;
let createBrowserAssetManagerFn = null;
let parseDocumentFn = null;
let domToWidgetsFn = null;
let collectWidgetStatsFn = null;
let flattenWidgetTreeFn = null;
let createRenderTreeTraceFn = null;
let summarizeRenderTreeTraceFn = null;
let browserAssetManager = null;
let currentNavigationUrl = '';
let currentSceneImageUrls = [];
let currentArtifactsState = null;
let renderTreeArtifactLogged = false;

function getUrlOrigin(url) {
  const value = typeof url === 'string' ? url.trim() : '';
  const match = value.match(/^[a-z][a-z0-9+.-]*:\/\/[^/?#]+/i);
  return match ? match[0] : '';
}

function getUrlDirectory(url) {
  const value = typeof url === 'string' ? url.trim() : '';
  const origin = getUrlOrigin(value);
  if (!origin) return '';
  const rest = value.slice(origin.length);
  const qIndex = rest.search(/[?#]/);
  const pathOnly = qIndex >= 0 ? rest.slice(0, qIndex) : rest;
  const slash = pathOnly.lastIndexOf('/');
  if (slash < 0) return `${origin}/`;
  return `${origin}${pathOnly.slice(0, slash + 1)}`;
}

function resolveNavigationUrl(baseUrl, href) {
  const value = safeString(href).trim();
  if (!value) return '';
  if (/^[a-z][a-z0-9+.-]*:/i.test(value)) return value;
  const base = safeString(baseUrl).trim();
  const origin = getUrlOrigin(base);
  if (value.startsWith('//')) {
    const schemeMatch = base.match(/^([a-z][a-z0-9+.-]*:)/i);
    return schemeMatch ? `${schemeMatch[1]}${value}` : `https:${value}`;
  }
  if (value.startsWith('/')) {
    return origin ? `${origin}${value}` : value;
  }
  const dir = getUrlDirectory(base);
  return dir ? `${dir}${value}` : value;
}

function log(line) {
  if (typeof console !== 'undefined' && console && typeof console.log === 'function') {
    console.log(line);
  }
}

function globalLogLine(line) {
  if (typeof root.__trueosGlobalLogLine !== 'function') return;
  try {
    root.__trueosGlobalLogLine(String(line || ''));
  } catch (_) {}
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

function ensureBrowserAssetManager() {
  if (browserAssetManager || typeof createBrowserAssetManagerFn !== 'function') {
    return browserAssetManager;
  }
  const publish = () => {
    try {
      publishLatestArtifacts();
    } catch (_) {}
  };
  browserAssetManager = createBrowserAssetManagerFn({
    host: root,
    browserId,
    paint: publish,
    resolveNavigationUrl: (href) => resolveNavigationUrl(currentNavigationUrl, href),
    onAssetStateChanged: publish,
    traceVideoSourceLine: globalLogLine,
  });
  return browserAssetManager;
}

function publishLatestArtifacts() {
  if (!currentArtifactsState) return null;
  const nextArtifacts = Object.assign({}, currentArtifactsState);
  if (browserAssetManager) {
    nextArtifacts.imageSummary = browserAssetManager.summarizeImageUrls(currentSceneImageUrls);
  }
  root.__trueosTruesurferLastArtifacts = nextArtifacts;
  currentArtifactsState = nextArtifacts;
  return nextArtifacts;
}

function logSyncPipeline(url, parsed) {
  const profile = truesurferSubsetProfile || {};
  log(
    `[truesurfer pipeline] browser=${browserId} mode=minimal_subset entry=signal stages=subset_scan>head+title>body_outline shell_bytes=${parsed.shellBytes} body_bytes=${parsed.bodyBytes} subset_body_roots=${parsed.bodyHierarchy.length} max_roots=${Number(profile.maxBodyHierarchyRoots || 0)} max_children=${Number(profile.maxBodyHierarchyChildrenPerNode || 0)} max_depth=${Number(profile.maxBodyHierarchyDepth || 0)} url=${url}`,
  );
}

function compactLogText(value, maxLength = 48) {
  const text = safeString(value).replace(/\s+/g, ' ').trim();
  if (text.length <= maxLength) return text;
  return `${text.slice(0, Math.max(0, maxLength - 1))}~`;
}

function sortedCountPairs(counts, maxItems = 12) {
  return Object.keys(counts || {})
    .sort((a, b) => {
      const delta = Number(counts[b] || 0) - Number(counts[a] || 0);
      return delta || a.localeCompare(b);
    })
    .slice(0, maxItems)
    .map((key) => `${key}:${Number(counts[key] || 0)}`)
    .join(',');
}

function compactLogValue(value) {
  if (value === true || value === false) return value ? 'true' : 'false';
  if (value === null) return 'null';
  if (value === undefined) return 'undefined';
  if (Array.isArray(value)) return `[${value.length}]`;
  if (typeof value === 'object') return `{${Object.keys(value).length}}`;
  const text = compactLogText(value, 32).replace(/"/g, "'");
  if (text.length === 0) return '""';
  return text.includes(' ') ? `"${text}"` : text;
}

function compactKeyValuePairs(object, maxItems = 8) {
  if (!object || typeof object !== 'object') return '-';
  const keys = Object.keys(object).sort().slice(0, maxItems);
  if (keys.length === 0) return '-';
  return keys.map((key) => `${key}:${compactLogValue(object[key])}`).join(',');
}

function widgetRowSummary(entry, index) {
  const node = entry && entry.node ? entry.node : {};
  const depth = Math.max(0, Number(entry && entry.depth) || 0);
  const rowId = node.kind === 'widget-root' ? 'root' : String(index);
  if (node.kind === 'text') {
    return [
      `#=${rowId}`,
      `depth=${depth}`,
      'kind=text',
      'key=-',
      'tag=#text',
      `chars=${safeString(node.text).length}`,
      `text="${compactLogText(node.text)}"`,
    ].join(' ');
  }

  const attrs = node.attrs && typeof node.attrs === 'object' ? node.attrs : {};
  const props = node.props && typeof node.props === 'object' ? node.props : {};
  const attrKeys = Object.keys(attrs).sort().slice(0, 8).join(',');
  return [
    `#=${rowId}`,
    `depth=${depth}`,
    `kind=${safeString(node.kind || '')}`,
    `key=${safeString(node.key || '-')}`,
    `tag=${safeString(node.tag || '')}`,
    `widget=${safeString(node.widget || '')}`,
    `category=${safeString(node.category || '')}`,
    `role=${safeString(node.role || '')}`,
    `children=${Array.isArray(node.children) ? node.children.length : 0}`,
    `attrs=${attrKeys || '-'}`,
    `props=${compactKeyValuePairs(props)}`,
  ].join(' ');
}

function logWidgetTable(url, widgetTree, startedAt) {
  if (!widgetTree || typeof collectWidgetStatsFn !== 'function' || typeof flattenWidgetTreeFn !== 'function') {
    log(`[truesurfer widgets] browser=${browserId} status=unavailable url=${url}`);
    return;
  }

  const stats = collectWidgetStatsFn(widgetTree);
  const flat = flattenWidgetTreeFn(widgetTree);
  const rootRow = flat.find((entry) => entry && entry.node && entry.node.kind === 'widget-root');
  const rows = flat.filter((entry) => entry && entry.node && entry.node.kind !== 'widget-root');
  const maxRows = 80;
  const elapsed = Date.now() - startedAt;

  log(
    `[truesurfer widgets] browser=${browserId} widget_nodes=${stats.nodes} widgets=${stats.widgets} text=${stats.text} complex=${stats.complex} interactive=${stats.interactive} tags=${sortedCountPairs(stats.tags)} categories=${sortedCountPairs(stats.categories)} rows=${rows.length} shown=${Math.min(rows.length, maxRows)} ms=${elapsed} url=${url}`,
  );
  log('[truesurfer widgets table] columns="# depth kind key tag widget category role children attrs props/text"');
  if (rootRow) {
    log(`[truesurfer widgets row] browser=${browserId} ${widgetRowSummary(rootRow, 0)}`);
  }

  for (let index = 0; index < rows.length && index < maxRows; index += 1) {
    log(`[truesurfer widgets row] browser=${browserId} ${widgetRowSummary(rows[index], index)}`);
  }
  if (rows.length > maxRows) {
    log(`[truesurfer widgets table] browser=${browserId} truncated=${rows.length - maxRows}`);
  }
}

function logRenderTreeArtifact(url, bytes, widgetTree, startedAt) {
  if (renderTreeArtifactLogged) return;
  if (typeof createRenderTreeTraceFn !== 'function') {
    log(`[truesurfer render-tree] browser=${browserId} status=unavailable url=${url}`);
    return;
  }

  try {
    const artifact = createRenderTreeTraceFn(widgetTree, {
      source: 'parse5',
      bytes,
      includeLayout: true,
    });
    const summary =
      typeof summarizeRenderTreeTraceFn === 'function'
        ? summarizeRenderTreeTraceFn(artifact)
        : { renderNodes: 0, renderHash: artifact.renderTree && artifact.renderTree.hash };
    const elapsed = Date.now() - startedAt;
    log(
      `[truesurfer render-tree] browser=${browserId} status=ready nodes=${Number(summary.renderNodes || 0)} render_hash=${safeString(summary.renderHash)} layout_hash=${safeString(summary.layoutHash)} ms=${elapsed} url=${url}`,
    );
    log(`[truesurfer render-tree ndjson] browser=${browserId} ${JSON.stringify(artifact.renderTree)}`);
    if (artifact.layoutTrace) {
      log(`[truesurfer render-tree ndjson] browser=${browserId} ${JSON.stringify(artifact.layoutTrace)}`);
    }
    renderTreeArtifactLogged = true;
  } catch (error) {
    const message = error && error.stack ? String(error.stack) : String(error || 'unknown render-tree error');
    log(`[truesurfer render-tree] browser=${browserId} status=failed error=${message} url=${url}`);
  }
}

function getImportHelpers(baseUrl) {
  if (typeof root.createImportHelpers === 'function') {
    return root.createImportHelpers(baseUrl);
  }
  return {
    prefetch(specifier) {
      return Promise.resolve(String(specifier));
    },
    import(specifier) {
      return import(String(specifier));
    },
  };
}

async function warmBrowserPipelineModules() {
  const helpers = getImportHelpers(TRUESURFER_MODULE_BASE);
  const imports = [
    './truesurfer_extract.mjs',
    './truesurfer_assets.mjs',
    './css.mjs',
    'parse5',
    '../widlib/index.mjs',
    '../rendertree/renderToTree.mjs',
  ];
  for (let index = 0; index < imports.length; index += 1) {
    await helpers.prefetch(imports[index]);
  }

  const [extractMod, assetsMod, cssMod, parse5Mod, widMod, renderTreeMod] = await Promise.all([
    helpers.import('./truesurfer_extract.mjs'),
    helpers.import('./truesurfer_assets.mjs'),
    helpers.import('./css.mjs'),
    helpers.import('parse5'),
    helpers.import('../widlib/index.mjs'),
    helpers.import('../rendertree/renderToTree.mjs'),
  ]);

  const extractReady = !!extractMod && typeof extractMod.extractDocumentArtifacts === 'function';
  const assetsReady = !!assetsMod && typeof assetsMod.createBrowserAssetManager === 'function';
  const cssReady = !!cssMod && typeof cssMod.extractCssSection === 'function';
  const parseReady = !!parse5Mod && typeof parse5Mod.parse === 'function';
  const widgetsReady =
    !!widMod
    && typeof widMod.domToWidgets === 'function'
    && typeof widMod.collectWidgetStats === 'function'
    && typeof widMod.flattenWidgetTree === 'function';
  const renderTreeReady =
    !!renderTreeMod
    && typeof renderTreeMod.createRenderTreeTrace === 'function'
    && typeof renderTreeMod.summarizeRenderTreeTrace === 'function';
  if (!extractReady || !assetsReady || !cssReady || !parseReady || !widgetsReady || !renderTreeReady) {
    throw new Error(
      `browser pipeline warmup incomplete extract_ready=${extractReady ? 1 : 0} assets_ready=${assetsReady ? 1 : 0} css_ready=${cssReady ? 1 : 0} parse_ready=${parseReady ? 1 : 0} widgets_ready=${widgetsReady ? 1 : 0} render_tree_ready=${renderTreeReady ? 1 : 0}`,
    );
  }

  truesurferSubsetProfile = extractMod.TRUESURFER_SUBSET_PROFILE || null;
  extractDocumentArtifactsFn = extractMod.extractDocumentArtifacts;
  createBrowserAssetManagerFn = assetsMod.createBrowserAssetManager;
  parseDocumentFn = parse5Mod.parse;
  domToWidgetsFn = widMod.domToWidgets;
  collectWidgetStatsFn = widMod.collectWidgetStats;
  flattenWidgetTreeFn = widMod.flattenWidgetTree;
  createRenderTreeTraceFn = renderTreeMod.createRenderTreeTrace;
  summarizeRenderTreeTraceFn = renderTreeMod.summarizeRenderTreeTrace;
  root.__trueosTruesurferModules = {
    extractReady: 1,
    assetsReady: 1,
    cssReady: 1,
    parseReady: 1,
    widgetsReady: 1,
    renderTreeReady: 1,
  };
}

async function bootstrapTruesurfer() {
  root.__trueosTruesurferWarmup = {
      status: 'warming',
      extractReady: 0,
      assetsReady: 0,
      cssReady: 0,
      parseReady: 0,
      widgetsReady: 0,
      renderTreeReady: 0,
      baseUrl: TRUESURFER_MODULE_BASE,
    };
  try {
    log(`[truesurfer bootstrap] browser=${browserId} warming modules base=${TRUESURFER_MODULE_BASE}`);
    await warmBrowserPipelineModules();
    const modules = root.__trueosTruesurferModules || {};
    root.__trueosTruesurferSubsetProfile = truesurferSubsetProfile;
    root.__trueosTruesurferWarmup = {
      status: 'ready',
      extractReady: modules.extractReady ? 1 : 0,
      assetsReady: modules.assetsReady ? 1 : 0,
      cssReady: modules.cssReady ? 1 : 0,
      parseReady: modules.parseReady ? 1 : 0,
      widgetsReady: modules.widgetsReady ? 1 : 0,
      renderTreeReady: modules.renderTreeReady ? 1 : 0,
      baseUrl: TRUESURFER_MODULE_BASE,
    };
    root.__trueosTruesurferReady = 1;
    log(
      `[truesurfer bootstrap] browser=${browserId} ready extract=${modules.extractReady ? 1 : 0} assets=${modules.assetsReady ? 1 : 0} css=${modules.cssReady ? 1 : 0} parse=${modules.parseReady ? 1 : 0} widgets=${modules.widgetsReady ? 1 : 0} render_tree=${modules.renderTreeReady ? 1 : 0}`,
    );
  } catch (error) {
    const message = error && error.stack ? String(error.stack) : String(error || 'unknown bootstrap error');
    root.__trueosTruesurferWarmup = {
      status: 'error',
      extractReady: 0,
      assetsReady: 0,
      cssReady: 0,
      parseReady: 0,
      widgetsReady: 0,
      renderTreeReady: 0,
      baseUrl: TRUESURFER_MODULE_BASE,
      error: message,
    };
    root.__trueosTruesurferReady = 0;
    log(`[truesurfer bootstrap] browser=${browserId} failed error=${message}`);
  }
}

function setHtml(nextHtml, meta) {
  const html = safeString(nextHtml);
  const url = safeString(meta && meta.url);
  const lines = countLines(html);

  currentNavigationUrl = url;
  currentSceneImageUrls = [];

  if (typeof extractDocumentArtifactsFn !== 'function' || typeof parseDocumentFn !== 'function' || typeof domToWidgetsFn !== 'function') {
    return {
      ok: 0,
      bytes: html.length,
      lines,
      error: 'truesurfer extractor/widgets are not ready',
    };
  }

  try {
    const widgetStart = Date.now();
    const parsedDocument = parseDocumentFn(html);
    const widgetTree = domToWidgetsFn(parsedDocument);
    const parsed = extractDocumentArtifactsFn(html);
    currentArtifactsState = {
      url,
      title: parsed.title,
      faviconUrl: resolveNavigationUrl(url, parsed.faviconHref),
      shellBytes: parsed.shellBytes,
      bodyBytes: parsed.bodyBytes,
      bodyHierarchy: parsed.bodyHierarchy,
      bodyHierarchySummary: parsed.bodyHierarchySummary,
      styleCount: parsed.styleCount,
      styleBytes: parsed.styleBytes,
      styleSlotCount: parsed.styleSlotCount,
      styledNodeCount: parsed.styledNodeCount,
      styleRuleCount: parsed.styleRuleCount,
      scriptCount: parsed.scriptCount,
      scriptBytes: parsed.scriptBytes,
    };
    publishLatestArtifacts();
    const imageSummary = { total: 0, pending: 0, ready: 0, error: 0 };
    logSyncPipeline(url, parsed);
    root.__trueosTruesurferLastStyleIndex = parsed.styleIndex;
    logWidgetTable(url, widgetTree, widgetStart);
    logRenderTreeArtifact(url, html.length, widgetTree, widgetStart);
    log(
      `[truesurfer extract] browser=${browserId} title=${parsed.title} shell_bytes=${parsed.shellBytes} body_bytes=${parsed.bodyBytes} subset_body_roots=${parsed.bodyHierarchy.length} body_outline=${parsed.bodyHierarchySummary} style_count=${parsed.styleCount} style_slots=${parsed.styleSlotCount} styled_nodes=${parsed.styledNodeCount} style_rules=${parsed.styleRuleCount} script_count=${parsed.scriptCount} images=${imageSummary.total} image_pending=${imageSummary.pending} image_ready=${imageSummary.ready} dom_ms=${parsed.domParseMs} css_ms=${parsed.styleIndexMs} ms=${parsed.parseMs} url=${url}`,
    );
    return {
      ok: 1,
      bytes: html.length,
      lines,
      parseMs: parsed.parseMs,
      domParseMs: parsed.domParseMs,
      styleIndexMs: parsed.styleIndexMs,
      title: parsed.title,
      faviconUrl: resolveNavigationUrl(url, parsed.faviconHref),
      shellBytes: parsed.shellBytes,
      bodyBytes: parsed.bodyBytes,
      styleCount: parsed.styleCount,
      styleBytes: parsed.styleBytes,
      styleSlotCount: parsed.styleSlotCount,
      styledNodeCount: parsed.styledNodeCount,
      styleRuleCount: parsed.styleRuleCount,
      scriptCount: parsed.scriptCount,
      scriptBytes: parsed.scriptBytes,
    };
  } catch (error) {
    const message =
      error && error.stack ? String(error.stack) : error ? String(error) : 'unknown minimal extract error';
    log(`[truesurfer extract] browser=${browserId} fail error=${message}`);
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
root.__trueosTruesurferSubsetProfile = null;
root.__trueosTruesurferReady = 0;
void bootstrapTruesurfer();
