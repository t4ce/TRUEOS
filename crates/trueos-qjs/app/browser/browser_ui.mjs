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
  function destroyBrowserRegion(entry) {
    void entry;
  }

  function destroyBrowserRegionCache() {
    return;
  }

  function invalidateRegionCache(reset = false) {
    if (reset) destroyBrowserRegionCache();
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
      fpsOverlayEnabled,
      fpsOverlay,
      finalizePaintState,
    } = args;
    if (!browserCanRenderScene) {
      return false;
    }

    void buildOverlayRuns(fpsOverlayEnabled, fpsOverlay, 0);
    finalizePaintState(doc);
    return false;
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
