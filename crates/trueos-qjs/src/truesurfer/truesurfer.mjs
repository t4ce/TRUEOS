/*
Truesurfer pipeline bridge:

html shack -> N-Browsers (Truesurfers)

In Truesurfer:
- Parse5 + CSS parse, JavaScript in parallel isolated
- Lightning CSS enrichment of the document and DOM subset
- Pipeline channel handoff to Yoga for layout enrichment

Runtime surface:
- Raw Parse5/Lightning/Yoga extraction state for the kernel renderer
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
let browserAssetManager = null;
let currentNavigationUrl = '';
let currentSceneImageUrls = [];
let currentArtifactsState = null;

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
    `[truesurfer pipeline] browser=${browserId} mode=minimal_subset entry=signal stages=subset_scan>head+title>body_outline shell_bytes=${parsed.shellBytes} body_bytes=${parsed.bodyBytes} body_nodes=${parsed.bodyHierarchy.length} max_roots=${Number(profile.maxBodyHierarchyRoots || 0)} max_children=${Number(profile.maxBodyHierarchyChildrenPerNode || 0)} max_depth=${Number(profile.maxBodyHierarchyDepth || 0)} url=${url}`,
  );
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
  ];
  for (let index = 0; index < imports.length; index += 1) {
    await helpers.prefetch(imports[index]);
  }

  const [extractMod, assetsMod, cssMod] = await Promise.all([
    helpers.import('./truesurfer_extract.mjs'),
    helpers.import('./truesurfer_assets.mjs'),
    helpers.import('./css.mjs'),
  ]);

  const extractReady = !!extractMod && typeof extractMod.extractDocumentArtifacts === 'function';
  const assetsReady = !!assetsMod && typeof assetsMod.createBrowserAssetManager === 'function';
  const cssReady = !!cssMod && typeof cssMod.extractCssSection === 'function';
  if (!extractReady || !assetsReady || !cssReady) {
    throw new Error(
      `browser pipeline warmup incomplete extract_ready=${extractReady ? 1 : 0} assets_ready=${assetsReady ? 1 : 0} css_ready=${cssReady ? 1 : 0}`,
    );
  }

  truesurferSubsetProfile = extractMod.TRUESURFER_SUBSET_PROFILE || null;
  extractDocumentArtifactsFn = extractMod.extractDocumentArtifacts;
  createBrowserAssetManagerFn = assetsMod.createBrowserAssetManager;
  root.__trueosTruesurferModules = {
    extractReady: 1,
    assetsReady: 1,
    cssReady: 1,
  };
}

async function bootstrapTruesurfer() {
  root.__trueosTruesurferWarmup = {
      status: 'warming',
      extractReady: 0,
      assetsReady: 0,
      cssReady: 0,
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
      baseUrl: TRUESURFER_MODULE_BASE,
    };
    root.__trueosTruesurferReady = 1;
    log(
      `[truesurfer bootstrap] browser=${browserId} ready extract=${modules.extractReady ? 1 : 0} assets=${modules.assetsReady ? 1 : 0} css=${modules.cssReady ? 1 : 0}`,
    );
  } catch (error) {
    const message = error && error.stack ? String(error.stack) : String(error || 'unknown bootstrap error');
    root.__trueosTruesurferWarmup = {
      status: 'error',
      extractReady: 0,
      assetsReady: 0,
      cssReady: 0,
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
  const assetManager = ensureBrowserAssetManager();

  currentNavigationUrl = url;
  currentSceneImageUrls = [];
  if (assetManager && typeof assetManager.beginPageLoad === 'function') {
    assetManager.beginPageLoad();
  }

  if (typeof extractDocumentArtifactsFn !== 'function') {
    return {
      ok: 0,
      bytes: html.length,
      lines,
      error: 'truesurfer extractor is not ready',
    };
  }

  try {
    const parsed = extractDocumentArtifactsFn(html);
    if (assetManager && typeof assetManager.traceHtmlVideoSources === 'function') {
      assetManager.traceHtmlVideoSources(html, { pageUrl: url });
    }
    currentSceneImageUrls = assetManager
      ? assetManager.primeHtmlImageUrls(html, { maxCount: TRUESURFER_MAX_SCENE_IMAGES })
      : [];
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
    const imageSummary = assetManager
      ? assetManager.summarizeImageUrls(currentSceneImageUrls)
      : { total: 0, pending: 0, ready: 0, error: 0 };
    logSyncPipeline(url, parsed);
    root.__trueosTruesurferLastStyleIndex = parsed.styleIndex;
    log(
      `[truesurfer extract] browser=${browserId} title=${parsed.title} shell_bytes=${parsed.shellBytes} body_bytes=${parsed.bodyBytes} body_nodes=${parsed.bodyHierarchy.length} body_outline=${parsed.bodyHierarchySummary} style_count=${parsed.styleCount} style_slots=${parsed.styleSlotCount} styled_nodes=${parsed.styledNodeCount} style_rules=${parsed.styleRuleCount} script_count=${parsed.scriptCount} images=${imageSummary.total} image_pending=${imageSummary.pending} image_ready=${imageSummary.ready} dom_ms=${parsed.domParseMs} css_ms=${parsed.styleIndexMs} ms=${parsed.parseMs} url=${url}`,
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
