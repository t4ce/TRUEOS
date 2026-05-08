/*
Truesurfer pipeline bridge:

html shack -> N-Browsers (Truesurfers)

In Truesurfer:
- Parse5 + CSS parse, JavaScript in parallel isolated
- Lightning CSS enrichment of the document and DOM subset
- Pipeline channel handoff to Yoga for layout enrichment

In UI2:
- N-window compositor-pattern UI that can already render the minimal demo
- Conceptually the full bridge is in place: acquired HTML can flow through parse,
  enrichment, layout, and minimal hosted composition for visual feedback
*/

const root = globalThis;
const browserId = Number(root.__trueosTruesurferBrowserId || 0);
const TRUESURFER_MODULE_BASE = typeof import.meta === 'object' && import.meta && typeof import.meta.url === 'string'
  ? String(import.meta.url)
  : '/qjs/truesurfer/truesurfer.mjs';
const TRUESURFER_MAX_SCENE_IMAGES = 5;
const TRUESURFER_IMAGE_VERTICAL_GAP_PX = 12;
const TRUESURFER_IMAGE_NODE_ID_BASE = 0x01000000;
const TRUESURFER_FALLBACK_IMAGE_SIZE_PX = 96;

let truesurferSubsetProfile = null;
let extractDocumentArtifactsFn = null;
let createBrowserAssetManagerFn = null;
let browserAssetManager = null;
let currentNavigationUrl = '';
let currentBaseGadgetSnapshot = { version: 1, gadgets: [] };
let currentSceneImageUrls = [];
let currentArtifactsState = null;
let currentGadgetSnapshotVersion = 1;

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

function cloneGadgetSnapshot(snapshot) {
  const source = snapshot && typeof snapshot === 'object' ? snapshot : null;
  const gadgets = source && Array.isArray(source.gadgets)
    ? source.gadgets.map((gadget) => Object.assign({}, gadget || {}))
    : [];
  const version = Math.max(1, Number(source && source.version || 0) | 0);
  const backgroundColorRgb = Math.max(0, Number(source && source.backgroundColorRgb || 0) >>> 0) & 0xFFFFFF;
  return { version, backgroundColorRgb, gadgets };
}

function nextGadgetSnapshotVersion() {
  currentGadgetSnapshotVersion = (currentGadgetSnapshotVersion + 1) >>> 0;
  if (currentGadgetSnapshotVersion <= 0) {
    currentGadgetSnapshotVersion = 1;
  }
  return currentGadgetSnapshotVersion;
}

function gadgetBottomPx(gadget) {
  const item = gadget && typeof gadget === 'object' ? gadget : null;
  if (!item) return 0;
  const yPx = Math.max(0, Number(item.yPx || 0) | 0);
  const heightPx = Math.max(
    Number(item.heightPx || 0) | 0,
    Number(item.lineHeightPx || 0) | 0,
    0,
  );
  return yPx + heightPx;
}

function snapshotBottomPx(snapshot) {
  const items = snapshot && Array.isArray(snapshot.gadgets) ? snapshot.gadgets : [];
  let bottom = 0;
  for (let index = 0; index < items.length; index += 1) {
    bottom = Math.max(bottom, gadgetBottomPx(items[index]));
  }
  return bottom;
}

function ensureBrowserAssetManager() {
  if (browserAssetManager || typeof createBrowserAssetManagerFn !== 'function') {
    return browserAssetManager;
  }
  const publish = () => {
    try {
      publishLatestArtifactsSnapshot();
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

function buildSceneImageGadgets(imageUrls) {
  const urls = Array.isArray(imageUrls) ? imageUrls : [];
  const manager = ensureBrowserAssetManager();
  if (!manager) return [];

  const out = [];
  let yCursor = snapshotBottomPx(currentBaseGadgetSnapshot);
  for (let index = 0; index < urls.length && out.length < TRUESURFER_MAX_SCENE_IMAGES; index += 1) {
    const resolvedUrl = String(urls[index] || '').trim();
    if (!resolvedUrl) continue;
    const cached = manager.getCachedImageTexture(resolvedUrl);
    if (!cached || String(cached.state || '') !== 'ready') continue;
    const texId = Math.max(0, Number(cached.texId || 0) | 0);
    if (texId <= 0) continue;

    const widthPx = Math.max(1, Number(cached.pixelWidth || 0) | 0 || TRUESURFER_FALLBACK_IMAGE_SIZE_PX);
    const heightPx = Math.max(1, Number(cached.pixelHeight || 0) | 0 || TRUESURFER_FALLBACK_IMAGE_SIZE_PX);
    if (yCursor > 0) {
      yCursor += TRUESURFER_IMAGE_VERTICAL_GAP_PX;
    }
    out.push({
      nodeId: TRUESURFER_IMAGE_NODE_ID_BASE + out.length + 1,
      tag: 'img',
      text: '',
      xPx: 0,
      yPx: yCursor,
      widthPx,
      heightPx,
      fontSizePx: 0,
      lineHeightPx: heightPx,
      textColorRgb: 0,
      buttonLike: false,
      changed: false,
      texId,
    });
    yCursor += heightPx;
  }
  return out;
}

function composeCurrentGadgetSnapshot() {
  const snapshot = cloneGadgetSnapshot(currentBaseGadgetSnapshot);
  const imageGadgets = buildSceneImageGadgets(currentSceneImageUrls);
  if (imageGadgets.length > 0) {
    snapshot.gadgets = snapshot.gadgets.concat(imageGadgets);
  }
  snapshot.version = nextGadgetSnapshotVersion();
  return snapshot;
}

function publishLatestArtifactsSnapshot() {
  if (!currentArtifactsState) return null;
  const nextArtifacts = Object.assign({}, currentArtifactsState);
  nextArtifacts.gadgetSnapshot = composeCurrentGadgetSnapshot();
  if (browserAssetManager) {
    nextArtifacts.imageSummary = browserAssetManager.summarizeImageUrls(currentSceneImageUrls);
  }
  root.__trueosTruesurferLastArtifacts = nextArtifacts;
  currentArtifactsState = nextArtifacts;
  return nextArtifacts.gadgetSnapshot;
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
    currentBaseGadgetSnapshot = cloneGadgetSnapshot(parsed.gadgetSnapshot);
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
      gadgetSnapshot: currentBaseGadgetSnapshot,
      styleCount: parsed.styleCount,
      styleBytes: parsed.styleBytes,
      styleSlotCount: parsed.styleSlotCount,
      styledNodeCount: parsed.styledNodeCount,
      styleRuleCount: parsed.styleRuleCount,
      scriptCount: parsed.scriptCount,
      scriptBytes: parsed.scriptBytes,
    };
    const composedGadgetSnapshot = publishLatestArtifactsSnapshot() || composeCurrentGadgetSnapshot();
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
      gadgetSnapshot: composedGadgetSnapshot,
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
