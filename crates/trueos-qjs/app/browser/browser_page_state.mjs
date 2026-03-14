import { createBrowserUiHookStream } from './browser_ui_hook.mjs';

export function createBrowserPageState(options = {}) {
  const host = options.host;
  const nowMs = typeof options.nowMs === 'function' ? options.nowMs : () => 0;
  const getCurrentUrl = typeof options.getCurrentUrl === 'function' ? options.getCurrentUrl : () => '';
  const getHtmlBytes = typeof options.getHtmlBytes === 'function' ? options.getHtmlBytes : () => 0;
  const summarizeAssets = typeof options.summarizeAssets === 'function'
    ? options.summarizeAssets
    : (() => ({ total: 0, pending: 0, ready: 0, error: 0 }));

  // Core idea: script/DOM work can already happen before the first visible page draw.
  // We keep that lifecycle separate from asset completion, so callers can hook the
  // pre-first-draw moment while images and other async assets continue independently.
  let pageLoadSeq = 0;
  let pageRenderedSeq = 0;
  let pagePreAssetDoneSeq = 0;
  let pageCompleteSeq = 0;
  let pageAssetUrls = [];
  let pageState = 'idle';

  const hookStream = createBrowserUiHookStream(host, {
    stateKey: '__trueosBrowserPageLoaded',
    lastEventKey: '__trueosBrowserPageLastEvent',
    queueKey: '__trueosBrowserPageEventQueue',
    hookName: '__trueosBrowserPageLoadedHook',
    maxQueue: 64,
  });

  function buildState(reason = '') {
    const summary = summarizeAssets(pageAssetUrls);
    const preAssetDone = pageRenderedSeq === pageLoadSeq && pageLoadSeq > 0;
    const pageComplete = pageCompleteSeq === pageLoadSeq && pageLoadSeq > 0;
    return {
      seq: pageLoadSeq,
      loadedSeq: pageCompleteSeq,
      preAssetDoneSeq: pagePreAssetDoneSeq,
      pageCompleteSeq: pageCompleteSeq,
      state: pageState,
      reason: String(reason || ''),
      url: String(getCurrentUrl() || ''),
      htmlBytes: Math.max(0, Number(getHtmlBytes() || 0) | 0),
      rendered: preAssetDone,
      preAssetDone,
      pageComplete,
      assets: {
        total: summary.total,
        pending: summary.pending,
        ready: summary.ready,
        error: summary.error,
      },
      tMs: nowMs(),
    };
  }

  function publishState(reason = '') {
    const state = buildState(reason);
    hookStream.publishState(state);
    return state;
  }

  function pushEvent(kind, reason = '') {
    const state = buildState(reason);
    return hookStream.pushEvent(kind, state);
  }

  function beginLoad(reason = 'html-set') {
    pageLoadSeq += 1;
    pageRenderedSeq = 0;
    pagePreAssetDoneSeq = 0;
    pageCompleteSeq = 0;
    pageAssetUrls = [];
    pageState = 'loading';
    publishState(reason);
    pushEvent('page-loading', reason);
  }

  function updateAssetUrls(urls, reason = 'assets-discovered') {
    pageAssetUrls = Array.isArray(urls)
      ? urls.map((value) => String(value || '').trim()).filter(Boolean)
      : [];
    publishState(reason);
    return refresh(reason);
  }

  function markRendered(reason = 'first-page-paint') {
    if (pageLoadSeq > 0 && pageRenderedSeq !== pageLoadSeq) {
      pageRenderedSeq = pageLoadSeq;
    }
    return refresh(reason);
  }

  function refresh(reason = 'state-refresh') {
    publishState(reason);
    if (pageLoadSeq <= 0) return false;
    const summary = summarizeAssets(pageAssetUrls);
    const preAssetDone = pageRenderedSeq === pageLoadSeq;
    if (preAssetDone && pagePreAssetDoneSeq !== pageLoadSeq) {
      pagePreAssetDoneSeq = pageLoadSeq;
      pageState = 'preasset-done';
      publishState(reason);
      pushEvent('preasset-done', reason);
    }
    const pageComplete = preAssetDone && summary.pending === 0;
    const nextState = pageComplete
      ? 'page-complete'
      : (preAssetDone ? 'preasset-done' : 'loading');
    if (pageState !== nextState) {
      pageState = nextState;
      publishState(reason);
    }
    if (pageComplete && pageCompleteSeq !== pageLoadSeq) {
      pageCompleteSeq = pageLoadSeq;
      publishState(reason);
      pushEvent('page-complete', reason);
      return true;
    }
    return false;
  }

  function getState(reason = 'api') {
    return buildState(reason);
  }

  function popEvent() {
    return hookStream.popEvent();
  }

  return {
    beginLoad,
    updateAssetUrls,
    markRendered,
    refresh,
    getState,
    popEvent,
  };
}
