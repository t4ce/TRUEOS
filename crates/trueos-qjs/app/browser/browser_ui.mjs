import * as cmdStream from 'trueos:cmd_stream';
import {
  renderSceneRegionToCurrentTarget,
  composeSceneRegionsToCurrentTarget,
} from './scene.mjs';

const BROWSER_REGION_CACHE_MAX = 4;
const BROWSER_REGION_PREFETCH_SCREENS = 1;
const BROWSER_REGION_TILE_MIN_PX = 512;
const BROWSER_REGION_TILE_MAX_PX = 2048;
const BROWSER_REGION_TILE_ALIGN_PX = 256;
const BROWSER_REGION_MAX_WIDTH = 2048;

export function createBrowserUiBridge() {
  const state = {
    regionCache: [],
    regionCacheSeq: 0,
    regionCacheRevision: 1,
    regionCacheWidth: 0,
    regionTileHeight: 0,
  };

  function destroyBrowserRegion(entry) {
    const texId = Math.max(0, Number(entry && entry.texId || 0) | 0);
    if (texId > 0 && typeof cmdStream.destroyTexture === 'function') {
      try {
        cmdStream.destroyTexture(texId);
      } catch (_) {}
    }
  }

  function destroyBrowserRegionCache() {
    for (let i = 0; i < state.regionCache.length; i += 1) {
      destroyBrowserRegion(state.regionCache[i]);
    }
    state.regionCache.length = 0;
    state.regionCacheWidth = 0;
    state.regionTileHeight = 0;
  }

  function invalidateRegionCache(reset = false) {
    state.regionCacheRevision = (state.regionCacheRevision + 1) >>> 0;
    if (state.regionCacheRevision === 0) state.regionCacheRevision = 1;
    if (reset) {
      destroyBrowserRegionCache();
      return;
    }
    for (let i = 0; i < state.regionCache.length; i += 1) {
      state.regionCache[i].dirty = true;
    }
  }

  function computeRegionTileHeight(vh) {
    const raw = Math.max(BROWSER_REGION_TILE_MIN_PX, Math.round(Number(vh || 1) * 1.5));
    const bounded = Math.min(BROWSER_REGION_TILE_MAX_PX, raw);
    const aligned = Math.ceil(bounded / BROWSER_REGION_TILE_ALIGN_PX) * BROWSER_REGION_TILE_ALIGN_PX;
    return Math.max(BROWSER_REGION_TILE_MIN_PX, Math.min(BROWSER_REGION_TILE_MAX_PX, aligned));
  }

  function docContentWidth(doc, vw) {
    const raw = Math.max(
      Number(doc && doc.contentW || 0),
      Number(doc && doc.themeLayout && doc.themeLayout.contentW || 0),
      Number(vw || 0),
    );
    const contentW = Math.max(1, Math.round(Number.isFinite(raw) ? raw : Number(vw || 1)));
    return Math.max(
      Math.max(1, Number(vw || 1) | 0),
      Math.min(BROWSER_REGION_MAX_WIDTH, contentW),
    );
  }

  function createBrowserRegionEntry(width, height, docY) {
    const texId = Math.max(0, Number(
      typeof cmdStream.createRenderTarget === 'function'
        ? cmdStream.createRenderTarget(width, height)
        : 0,
    ) | 0);
    if (texId <= 0) return null;
    return {
      texId,
      width,
      height,
      docY,
      revision: 0,
      dirty: true,
      lastUsedSeq: 0,
    };
  }

  function browserRegionVisibleTop(scrollY) {
    return Math.max(0, Math.round(Number(scrollY || 0)));
  }

  function ensureBrowserRegions(doc, vw, vh, scrollY, contentH) {
    const width = docContentWidth(doc, vw);
    const tileHeight = computeRegionTileHeight(vh);
    if (state.regionCacheWidth !== width || state.regionTileHeight !== tileHeight) {
      destroyBrowserRegionCache();
      state.regionCacheWidth = width;
      state.regionTileHeight = tileHeight;
    }

    const visibleTop = browserRegionVisibleTop(scrollY);
    const visibleBottom = Math.max(visibleTop + 1, Math.min(contentH, visibleTop + Math.max(1, Number(vh || 1) | 0)));
    const prefetchPx = tileHeight * BROWSER_REGION_PREFETCH_SCREENS;
    const wantedTop = Math.max(0, visibleTop - prefetchPx);
    const wantedBottom = Math.max(visibleBottom, Math.min(contentH, visibleBottom + prefetchPx));
    const firstDocY = Math.max(0, Math.floor(wantedTop / tileHeight) * tileHeight);
    const wantedEntries = [];
    const wantedDocYs = new Set();

    for (let docY = firstDocY; docY < wantedBottom || wantedEntries.length <= 0; docY += tileHeight) {
      if (docY >= contentH && wantedEntries.length > 0) break;
      const height = Math.max(1, Math.min(tileHeight, contentH - docY));
      const key = `${docY}:${height}`;
      wantedDocYs.add(key);

      let entry = null;
      for (let i = 0; i < state.regionCache.length; i += 1) {
        const candidate = state.regionCache[i];
        if (candidate && candidate.docY === docY && candidate.height === height && candidate.width === width) {
          entry = candidate;
          break;
        }
      }
      if (!entry) {
        entry = createBrowserRegionEntry(width, height, docY);
        if (!entry) {
          destroyBrowserRegionCache();
          return null;
        }
        state.regionCache.push(entry);
      }

      entry.lastUsedSeq = ++state.regionCacheSeq;
      if (entry.dirty || entry.revision !== state.regionCacheRevision) {
        cmdStream.setRenderTarget(entry.texId);
        cmdStream.setViewport(entry.width, entry.height);
        renderSceneRegionToCurrentTarget(doc, entry.width, entry.docY, entry.height);
        entry.revision = state.regionCacheRevision;
        entry.dirty = false;
      }
      wantedEntries.push(entry);
    }

    for (let i = state.regionCache.length - 1; i >= 0; i -= 1) {
      const entry = state.regionCache[i];
      const key = `${entry.docY}:${entry.height}`;
      if (wantedDocYs.has(key)) continue;
      destroyBrowserRegion(entry);
      state.regionCache.splice(i, 1);
    }

    while (state.regionCache.length > BROWSER_REGION_CACHE_MAX) {
      let dropIdx = -1;
      let dropSeq = Number.POSITIVE_INFINITY;
      for (let i = 0; i < state.regionCache.length; i += 1) {
        const entry = state.regionCache[i];
        const key = `${entry.docY}:${entry.height}`;
        if (wantedDocYs.has(key)) continue;
        if (entry.lastUsedSeq < dropSeq) {
          dropSeq = entry.lastUsedSeq;
          dropIdx = i;
        }
      }
      if (dropIdx < 0) break;
      destroyBrowserRegion(state.regionCache[dropIdx]);
      state.regionCache.splice(dropIdx, 1);
    }

    wantedEntries.sort((lhs, rhs) => lhs.docY - rhs.docY);
    return wantedEntries;
  }

  function buildOverlayRuns(fpsOverlayEnabled, fpsOverlay, vw) {
    const overlayRuns = [];
    if (fpsOverlayEnabled && fpsOverlay && typeof fpsOverlay.appendRuns === 'function') {
      fpsOverlay.appendRuns(overlayRuns, vw);
    }
    return overlayRuns;
  }

  function paintToCurrentTarget(args) {
    const {
      browserCanRenderScene,
      doc,
      vw,
      vh,
      scrollX,
      scrollY,
      contentH,
      contentTopY,
      composeViewportWidth,
      composeViewportHeight,
      fpsOverlayEnabled,
      fpsOverlay,
      finalizePaintState,
    } = args;
    if (!browserCanRenderScene) {
      return false;
    }

    const overlayRuns = buildOverlayRuns(fpsOverlayEnabled, fpsOverlay, vw);
    const regions = ensureBrowserRegions(doc, vw, vh, scrollY, contentH);
    if (!regions || regions.length <= 0) {
      return false;
    }

    cmdStream.setViewport(composeViewportWidth, composeViewportHeight);
    composeSceneRegionsToCurrentTarget(regions, vw, vh, scrollX, scrollY, contentTopY, overlayRuns, null);
    finalizePaintState(doc);
    return true;
  }

  function paint(args) {
    const {
      hostedByUi2,
      browserCanRenderScene,
      doc,
      vw,
      vh,
      scrollX,
      scrollY,
      contentH,
      contentTopY,
      fpsOverlayEnabled,
      fpsOverlay,
      finalizePaintState,
    } = args;

    if (hostedByUi2) {
      if (!browserCanRenderScene || !doc) {
        return true;
      }
      // In hosted/ui2 mode, the browser window owns invalidation state but not
      // frame lifetime. Only renderToTexture() is allowed to drive cmdStream.
      finalizePaintState(doc);
      return true;
    }
    return false;
  }

  function getSurfaceState(args) {
    const {
      hostedByUi2,
      browserCanRenderScene,
      doc,
      vw,
      vh,
      scrollX,
      scrollY,
      contentW,
      contentH,
      contentTopY,
    } = args;
    void hostedByUi2;

    const safeContentW = browserCanRenderScene
      ? Math.max(1, Number(contentW || vw) | 0)
      : Math.max(1, Number(vw || 1) | 0);
    const safeContentH = browserCanRenderScene
      ? Math.max(1, Number(contentH || vh) | 0)
      : Math.max(1, Number(vh || 1) | 0);
    void doc;

    return {
      viewportWidth: vw,
      viewportHeight: vh,
      contentWidth: safeContentW,
      contentHeight: safeContentH,
      contentTopY: Math.max(0, Number(contentTopY || 0) | 0),
      scrollX,
      scrollY,
    };
  }

  function requestRepaint(onPaint) {
    invalidateRegionCache(false);
    if (globalThis && globalThis.__trueosBrowserHostedByUi2) {
      return;
    }
    if (typeof onPaint === 'function') {
      onPaint();
    }
  }

  return {
    invalidateRegionCache,
    requestRepaint,
    paint,
    paintToCurrentTarget,
    getSurfaceState,
  };
}
