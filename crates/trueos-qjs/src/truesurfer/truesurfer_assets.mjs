import * as parse5 from 'parse5';
import { Buffer } from 'node:buffer';

const MAX_FETCHED_IMAGE_BINARIES = 64;
const DEFAULT_MAX_SCENE_IMAGES = 5;
const TRUESURFER_TRACE_VIDEO_SOURCES = true;
const MAX_VIDEO_SOURCE_TRACE = 64;
const VIDEO_URL_HINT_RE = /\.(?:mp4|m4v|webm|ogv|ogg|mov|m3u8|mpd|ts)(?:[?#]|$)/i;
const VIDEO_ATTR_NAMES = [
  'src',
  'data-src',
  'data-video-src',
  'data-hd-src',
  'data-file',
  'data-url',
];

function getNodeAttr(node, name) {
  const wanted = String(name || '').toLowerCase();
  if (!wanted || !node || !Array.isArray(node.attrs)) return '';
  for (let i = 0; i < node.attrs.length; i += 1) {
    const attr = node.attrs[i];
    if (!attr || String(attr.name || '').toLowerCase() !== wanted) continue;
    return String(attr.value || '');
  }
  return '';
}

function isImageNode(node) {
  return !!node && typeof node === 'object' && String(node.tagName || '').toLowerCase() === 'img';
}

function isVideoMime(value) {
  const text = String(value || '').trim().toLowerCase();
  return text.startsWith('video/') || text.includes('mpegurl') || text.includes('dash+xml');
}

function trimForLog(value, maxLen = 360) {
  const text = String(value || '').replace(/[\r\n\t]+/g, ' ').trim();
  if (text.length <= maxLen) return text;
  return `${text.slice(0, maxLen - 3)}...`;
}

function firstSrcsetCandidate(value) {
  const text = String(value || '').trim();
  if (!text) return '';
  const first = text.split(',')[0] || '';
  return String(first).trim().split(/\s+/)[0] || '';
}

function isLikelyVideoSource(tagName, attrName, rawUrl, typeValue, insideVideo) {
  if (!rawUrl) return false;
  if (insideVideo) return true;
  if (tagName === 'video' || tagName === 'track') return true;
  if (tagName === 'source' && isVideoMime(typeValue)) return true;
  if (isVideoMime(typeValue)) return true;
  if (VIDEO_URL_HINT_RE.test(rawUrl)) return true;
  return attrName.includes('video');
}

function collectVideoSourceTargets(node, options = {}, out = null, insideVideo = false) {
  const targets = Array.isArray(out) ? out : [];
  if (!node || typeof node !== 'object') return targets;

  const tagName = String(node.tagName || '').toLowerCase();
  const nextInsideVideo = insideVideo || tagName === 'video';
  const typeValue = getNodeAttr(node, 'type');

  for (let i = 0; i < VIDEO_ATTR_NAMES.length; i += 1) {
    const attrName = VIDEO_ATTR_NAMES[i];
    const rawUrl = String(getNodeAttr(node, attrName) || '').trim();
    if (!isLikelyVideoSource(tagName, attrName, rawUrl, typeValue, nextInsideVideo)) continue;
    targets.push({
      tag: tagName || 'node',
      attr: attrName,
      type: typeValue,
      url: options.resolveUrl ? options.resolveUrl(rawUrl) : rawUrl,
    });
  }

  const srcsetUrl = firstSrcsetCandidate(getNodeAttr(node, 'srcset'));
  if (isLikelyVideoSource(tagName, 'srcset', srcsetUrl, typeValue, nextInsideVideo)) {
    targets.push({
      tag: tagName || 'node',
      attr: 'srcset',
      type: typeValue,
      url: options.resolveUrl ? options.resolveUrl(srcsetUrl) : srcsetUrl,
    });
  }

  const poster = String(getNodeAttr(node, 'poster') || '').trim();
  if (tagName === 'video' && poster) {
    targets.push({
      tag: 'video',
      attr: 'poster',
      type: 'poster',
      url: options.resolveUrl ? options.resolveUrl(poster) : poster,
    });
  }

  const kids = Array.isArray(node.childNodes) ? node.childNodes : [];
  for (let i = 0; i < kids.length; i += 1) {
    collectVideoSourceTargets(kids[i], options, targets, nextInsideVideo);
  }
  return targets;
}

function traceVideoSourcesFromParsed(parsed, options = {}) {
  if (!TRUESURFER_TRACE_VIDEO_SOURCES) return [];
  const logLine = typeof options.logLine === 'function' ? options.logLine : null;
  if (!logLine || !parsed) return [];

  const seen = new Set();
  const pageUrl = trimForLog(options.pageUrl || '', 240);
  const browserId = Math.max(0, Number(options.browserId || 0) | 0);
  const targets = collectVideoSourceTargets(parsed, {
    resolveUrl: typeof options.resolveUrl === 'function' ? options.resolveUrl : (url) => String(url || ''),
  }, []);

  let emitted = 0;
  for (let i = 0; i < targets.length && emitted < MAX_VIDEO_SOURCE_TRACE; i += 1) {
    const target = targets[i] || {};
    const url = String(target.url || '').trim();
    if (!url) continue;
    const key = `${target.tag}|${target.attr}|${url}`;
    if (seen.has(key)) continue;
    seen.add(key);
    logLine(
      `video-src: browser=${browserId} tag=${trimForLog(target.tag, 32)} attr=${trimForLog(target.attr, 32)} type=${trimForLog(target.type || '', 80)} page=${pageUrl} url=${trimForLog(url)}`,
    );
    emitted += 1;
  }
  return Array.from(seen);
}

export function createBrowserAssetManager(options = {}) {
  const cmdStream = options.cmdStream;
  const host = options.host;
  const paint = typeof options.paint === 'function' ? options.paint : () => {};
  const resolveNavigationUrl = typeof options.resolveNavigationUrl === 'function'
    ? options.resolveNavigationUrl
    : (url) => String(url || '');
  const raiseBrowserError = typeof options.raiseBrowserError === 'function'
    ? options.raiseBrowserError
    : ((code, message) => {
      const err = new Error(String(message || 'browser asset error'));
      err.code = String(code || 'TRUEOS_BROWSER_ASSET_ERROR');
      throw err;
    });
  const describeError = typeof options.describeError === 'function'
    ? options.describeError
    : ((err) => String(err && err.message ? err.message : err || 'unknown error'));
  const onAssetStateChanged = typeof options.onAssetStateChanged === 'function'
    ? options.onAssetStateChanged
    : () => {};
  const traceVideoSourceLine = typeof options.traceVideoSourceLine === 'function'
    ? options.traceVideoSourceLine
    : null;
  const browserId = Math.max(0, Number(options.browserId || 0) | 0);

  const imageTextureCache = new Map();
  const imageTextureLoads = new Map();
  const fetchedImageBinaryUrls = new Set();
  const prewarmedUrls = new Set();
  const pendingPriorityImageUrls = new Set();
  const pendingNormalImageUrls = new Set();
  let imagePrimeScheduled = false;

  function queuedImageUrlCount() {
    return pendingPriorityImageUrls.size + pendingNormalImageUrls.size;
  }

  function dequeuePendingImageUrl() {
    const priority = pendingPriorityImageUrls.values().next();
    if (!priority.done) {
      const value = String(priority.value || '').trim();
      pendingPriorityImageUrls.delete(priority.value);
      pendingNormalImageUrls.delete(priority.value);
      return value;
    }
    const normal = pendingNormalImageUrls.values().next();
    if (!normal.done) {
      const value = String(normal.value || '').trim();
      pendingNormalImageUrls.delete(normal.value);
      return value;
    }
    return '';
  }

  function enqueuePendingImageUrl(url, priority = false) {
    const resolvedSrc = String(url || '').trim();
    if (!resolvedSrc) return false;
    if (priority) {
      pendingNormalImageUrls.delete(resolvedSrc);
      pendingPriorityImageUrls.add(resolvedSrc);
      return true;
    }
    if (pendingPriorityImageUrls.has(resolvedSrc) || pendingNormalImageUrls.has(resolvedSrc)) {
      return false;
    }
    pendingNormalImageUrls.add(resolvedSrc);
    return true;
  }

  function maybePrewarmUrl(url) {
    const value = String(url || '').trim();
    if (!value || value.startsWith('data:')) return;
    if (prewarmedUrls.has(value)) return;
    if (typeof host.__trueosPrewarmUrl !== 'function') return;
    try {
      const rc = Number(host.__trueosPrewarmUrl(value) || 0);
      if (rc >= 0) {
        prewarmedUrls.add(value);
      }
    } catch (_) {}
  }

  function dataUrlToBytes(url) {
    const value = String(url || '');
    const match = value.match(/^data:([^,]*?),(.*)$/s);
    if (!match) {
      raiseBrowserError('TRUEOS_BROWSER_IMAGE_DATA_URL_INVALID', 'Image data URL could not be decoded', {
        url: value,
      });
    }

    const meta = String(match[1] || '');
    const payload = String(match[2] || '');
    const parts = meta.split(';').map((part) => String(part || '').trim().toLowerCase()).filter(Boolean);
    const mime = parts.length > 0 && !parts[0].includes('=') ? parts[0] : 'text/plain';
    const isBase64 = parts.includes('base64');

    if (isBase64) {
      const decoded = Buffer.from(payload, 'base64');
      return {
        mime,
        bytes: new Uint8Array(decoded.buffer, decoded.byteOffset, decoded.byteLength).slice(),
      };
    }

    const text = decodeURIComponent(payload);
    const bytes = new Uint8Array(text.length);
    for (let i = 0; i < text.length; i += 1) {
      bytes[i] = text.charCodeAt(i) & 0xFF;
    }
    return { mime, bytes };
  }

  function readPngDimensions(bytes) {
    const data = bytes instanceof Uint8Array ? bytes : null;
    if (!data || data.length < 24) {
      return { width: 0, height: 0 };
    }
    if (
      data[0] !== 0x89 || data[1] !== 0x50 || data[2] !== 0x4E || data[3] !== 0x47
      || data[4] !== 0x0D || data[5] !== 0x0A || data[6] !== 0x1A || data[7] !== 0x0A
    ) {
      return { width: 0, height: 0 };
    }
    if (String.fromCharCode(data[12], data[13], data[14], data[15]) !== 'IHDR') {
      return { width: 0, height: 0 };
    }
    const view = new DataView(data.buffer, data.byteOffset, data.byteLength);
    return {
      width: Math.max(0, view.getUint32(16, false) | 0),
      height: Math.max(0, view.getUint32(20, false) | 0),
    };
  }

  function readJpegDimensions(bytes) {
    const data = bytes instanceof Uint8Array ? bytes : null;
    if (!data || data.length < 4 || data[0] !== 0xFF || data[1] !== 0xD8) {
      return { width: 0, height: 0 };
    }

    let offset = 2;
    while (offset + 1 < data.length) {
      while (offset < data.length && data[offset] !== 0xFF) {
        offset += 1;
      }
      while (offset < data.length && data[offset] === 0xFF) {
        offset += 1;
      }
      if (offset >= data.length) break;

      const marker = data[offset];
      offset += 1;

      if (marker === 0xD8 || marker === 0xD9) {
        continue;
      }
      if (marker === 0x01 || (marker >= 0xD0 && marker <= 0xD7)) {
        continue;
      }
      if (offset + 1 >= data.length) break;

      const segmentLen = (data[offset] << 8) | data[offset + 1];
      if (segmentLen < 2 || offset + segmentLen > data.length) {
        break;
      }

      if (
        (marker >= 0xC0 && marker <= 0xC3)
        || (marker >= 0xC5 && marker <= 0xC7)
        || (marker >= 0xC9 && marker <= 0xCB)
        || (marker >= 0xCD && marker <= 0xCF)
      ) {
        if (segmentLen >= 7) {
          const height = (data[offset + 3] << 8) | data[offset + 4];
          const width = (data[offset + 5] << 8) | data[offset + 6];
          return {
            width: Math.max(0, width | 0),
            height: Math.max(0, height | 0),
          };
        }
        break;
      }

      offset += segmentLen;
    }

    return { width: 0, height: 0 };
  }

  function isSvgMime(mime) {
    const value = String(mime || '').trim().toLowerCase();
    return value === 'image/svg+xml' || value.endsWith('+svg') || value.includes('svg+xml');
  }

  function isJpegMime(mime) {
    const value = String(mime || '').trim().toLowerCase();
    return value === 'image/jpeg' || value === 'image/jpg';
  }

  function waitForNextImageUploadTick() {
    return new Promise((resolve) => {
      if (typeof setTimeout === 'function') {
        setTimeout(() => resolve(), 1);
        return;
      }
      throw new Error('image upload wait requires setTimeout');
    });
  }

  async function waitForTextureReady(texId) {
    const id = Math.max(0, Number(texId || 0) | 0);
    if (id <= 0) {
      throw new Error('invalid texture id');
    }
    while (true) {
      const status = Math.round(Number(cmdStream.getTextureStatus(id) || 0) || 0);
      if (status === 2) {
        return true;
      }
      if (status < 0) {
        throw new Error(`texture upload failed (${status})`);
      }
      await waitForNextImageUploadTick();
    }
  }

  function isSupportedSceneImageUrl(url) {
    const value = String(url || '').trim();
    if (!value) return false;
    if (value.startsWith('data:')) {
      return /^data:image\/(png|jpe?g|svg\+xml)(?:;|,)/i.test(value);
    }
    return !!resolveFetchableImageKind(value);
  }

  function resolveFetchableImageKind(url) {
    const normalizedUrl = String(url || '').toLowerCase();
    if (/\.png(?:$|[?#])/.test(normalizedUrl)) return 'png';
    if (/\.jpe?g(?:$|[?#])/.test(normalizedUrl)) return 'jpeg';
    if (/\.svg(?:$|[?#])/.test(normalizedUrl)) return 'svg';
    return '';
  }

  function resolveSceneImageKind(url) {
    const value = String(url || '').trim();
    if (value.startsWith('data:')) {
      if (/^data:image\/svg\+xml(?:;|,)/i.test(value)) return 'svg';
      if (/^data:image\/jpe?g(?:;|,)/i.test(value)) return 'jpeg';
      if (/^data:image\/png(?:;|,)/i.test(value)) return 'png';
      return '';
    }
    return resolveFetchableImageKind(value);
  }

  function beginPageLoad() {
    fetchedImageBinaryUrls.clear();
  }

  function noteFetchedImageBinary(url) {
    const key = String(url || '').trim();
    if (!key || fetchedImageBinaryUrls.has(key)) return true;
    if (fetchedImageBinaryUrls.size >= MAX_FETCHED_IMAGE_BINARIES) {
      return false;
    }
    fetchedImageBinaryUrls.add(key);
    return true;
  }

  function queueRepaint() {
    try {
      paint();
    } catch (_) {}
  }

  function createFailedImageTexture(url, error) {
    return {
      state: 'error',
      texId: 0,
      url: String(url || '').trim(),
      mime: '',
      error: String(error || 'image unavailable'),
    };
  }

  async function requestReadyImageTexture(url) {
    const value = String(url || '').trim();
    if (!value) {
      raiseBrowserError('TRUEOS_BROWSER_IMAGE_URL_MISSING', 'Image request failed because src was empty');
    }
    maybePrewarmUrl(value);

    if (!value.startsWith('data:')) {
      const requestedKind = resolveFetchableImageKind(value);
      if (!requestedKind) {
        raiseBrowserError(
          'TRUEOS_BROWSER_IMAGE_FETCH_KIND_UNSUPPORTED',
          'Image fetch is restricted to png, jpeg, jpg, or svg URLs',
          { url: value },
        );
      }
      if (!noteFetchedImageBinary(value)) {
        throw new Error(`image fetch limit reached (${MAX_FETCHED_IMAGE_BINARIES})`);
      }
    }

    if (typeof host.__trueosResolveReadyImageTexture !== 'function') {
      raiseBrowserError('TRUEOS_BROWSER_IMAGE_NATIVE_UNAVAILABLE', 'Native image request is unavailable', {
        url: value,
      });
    }

    const result = await host.__trueosResolveReadyImageTexture(value);
    const texId = Math.max(0, Number(result && result.texId || 0) | 0);
    if (texId <= 0) {
      raiseBrowserError('TRUEOS_BROWSER_IMAGE_NATIVE_FAILED', 'Native image request did not yield a texture id', {
        url: value,
      });
    }
    return {
      texId,
      mime: String(result && result.mime || ''),
      pixelWidth: Math.max(0, Number(result && result.width || 0) | 0),
      pixelHeight: Math.max(0, Number(result && result.height || 0) | 0),
    };
  }

  async function ensureImageTexture(resolvedUrl) {
    const cacheKey = String(resolvedUrl || '').trim();
    if (!cacheKey) return null;

    const cached = imageTextureCache.get(cacheKey) || null;
    if (cached && (cached.state === 'ready' || cached.state === 'error')) {
      return cached;
    }

    const inFlight = imageTextureLoads.get(cacheKey) || null;
    if (inFlight) {
      return inFlight;
    }

    imageTextureCache.set(cacheKey, {
      state: 'loading',
      texId: 0,
      url: cacheKey,
      mime: '',
      error: '',
    });

    const task = (async () => {
      try {
        let ready;
        if (cacheKey.startsWith('data:')) {
          const { mime, bytes } = dataUrlToBytes(cacheKey);
          const dims = isSvgMime(mime)
            ? { width: 0, height: 0 }
            : (isJpegMime(mime) ? readJpegDimensions(bytes) : readPngDimensions(bytes));
          const texId = Number(
            (isSvgMime(mime)
              ? cmdStream.createTextureSvgAsync(bytes)
              : (isJpegMime(mime)
                ? cmdStream.createTextureJpegAsync(bytes)
                : cmdStream.createTexturePngAsync(bytes))) || 0,
          );
          if (!Number.isFinite(texId) || texId <= 0) {
            throw new Error('inline image texture upload failed');
          }
          await waitForTextureReady(texId);
          ready = {
            state: 'ready',
            texId,
            url: cacheKey,
            mime: String(mime || (isJpegMime(mime) ? 'image/jpeg' : 'image/png')),
            pixelWidth: Math.max(0, Number(dims.width || 0) | 0),
            pixelHeight: Math.max(0, Number(dims.height || 0) | 0),
            error: '',
          };
        } else {
          const requestedKind = resolveFetchableImageKind(cacheKey);
          if (!requestedKind) {
            ready = createFailedImageTexture(cacheKey, 'unsupported image URL kind');
            imageTextureCache.set(cacheKey, ready);
            return ready;
          }
          const loaded = await requestReadyImageTexture(cacheKey);
          ready = {
            state: 'ready',
            texId: Math.max(0, Number(loaded.texId || 0) | 0),
            url: cacheKey,
            mime: String(loaded.mime || 'image/png'),
            pixelWidth: Math.max(0, Number(loaded.pixelWidth || 0) | 0),
            pixelHeight: Math.max(0, Number(loaded.pixelHeight || 0) | 0),
            error: '',
          };
        }
        imageTextureCache.set(cacheKey, ready);
        return ready;
      } catch (err) {
        const failed = createFailedImageTexture(cacheKey, describeError(err));
        imageTextureCache.set(cacheKey, failed);
        return failed;
      } finally {
        imageTextureLoads.delete(cacheKey);
        try { onAssetStateChanged(cacheKey); } catch (_) {}
        queueRepaint();
      }
    })();

    imageTextureLoads.set(cacheKey, task);
    return task;
  }

  function flushQueuedImagePrimes() {
    imagePrimeScheduled = false;
    while (queuedImageUrlCount() > 0) {
      const resolvedSrc = dequeuePendingImageUrl();
      if (!resolvedSrc) continue;
      const cached = imageTextureCache.get(resolvedSrc) || null;
      if (cached && (cached.state === 'ready' || cached.state === 'loading' || cached.state === 'error')) {
        continue;
      }
      void ensureImageTexture(resolvedSrc).catch(() => {});
    }
  }

  function scheduleImagePrimeFlush() {
    if (imagePrimeScheduled) return;
    imagePrimeScheduled = true;
    const job = () => {
      try {
        flushQueuedImagePrimes();
      } catch (_) {}
    };
    if (typeof Promise === 'function' && typeof Promise.resolve === 'function') {
      Promise.resolve().then(job).catch(() => {
        imagePrimeScheduled = false;
      });
      return;
    }
    job();
  }

  function applyResourcesToRows(rows) {
    const list = Array.isArray(rows) ? rows : [];
    for (let i = 0; i < list.length; i += 1) {
      const row = list[i];
      if (!row || String(row.kind || '') !== 'image') continue;
      const rawSrc = String(row.src || '').trim();
      const resolvedSrc = rawSrc ? resolveNavigationUrl(rawSrc) : '';
      row.resolvedSrc = resolvedSrc;
      row.texId = 0;
      row.imagePixelWidth = 0;
      row.imagePixelHeight = 0;
      if (!resolvedSrc) continue;
      const cached = imageTextureCache.get(resolvedSrc) || null;
      if (cached && cached.state === 'ready') {
        row.texId = Number(cached.texId || 0);
        row.imagePixelWidth = Math.max(0, Number(cached.pixelWidth || 0) | 0);
        row.imagePixelHeight = Math.max(0, Number(cached.pixelHeight || 0) | 0);
      }
    }
  }

  function requestAssetsForRows(rows) {
    const list = Array.isArray(rows) ? rows : [];
    for (let i = 0; i < list.length; i += 1) {
      const row = list[i];
      if (!row || String(row.kind || '') !== 'image') continue;
      const resolvedSrc = String(row.resolvedSrc || '').trim();
      if (!resolvedSrc) continue;
      maybePrewarmUrl(resolvedSrc);
      const cached = imageTextureCache.get(resolvedSrc) || null;
      if (cached && (cached.state === 'ready' || cached.state === 'loading' || cached.state === 'error')) {
        continue;
      }
      enqueuePendingImageUrl(resolvedSrc, false);
    }
    scheduleImagePrimeFlush();
  }

  function imageAssetTagForUrl(resolvedSrc, kind, defaultState = 'unsupported') {
    const cached = imageTextureCache.get(resolvedSrc) || null;
    const state = cached && cached.state ? String(cached.state) : defaultState;
    return {
      src: String(resolvedSrc || ''),
      kind: String(kind || ''),
      state,
      texId: cached && cached.state === 'ready' ? Math.max(0, Number(cached.texId || 0) | 0) : 0,
      mime: cached ? String(cached.mime || '') : '',
      pixelWidth: cached ? Math.max(0, Number(cached.pixelWidth || 0) | 0) : 0,
      pixelHeight: cached ? Math.max(0, Number(cached.pixelHeight || 0) | 0) : 0,
      error: cached ? String(cached.error || '') : '',
    };
  }

  function tagWidgetTreeImages(widgetTree, options = {}) {
    const maxCount = Math.max(0, Number(options.maxCount || DEFAULT_MAX_SCENE_IMAGES) | 0);
    const urls = [];
    const unique = new Set();

    const walk = (node) => {
      if (!node || typeof node !== 'object') return;
      if (node.kind === 'widget' && String(node.tag || node.widget || '').toLowerCase() === 'img') {
        const props = node.props && typeof node.props === 'object' ? node.props : {};
        const attrs = node.attrs && typeof node.attrs === 'object' ? node.attrs : {};
        const rawSrc = String(props.src ?? attrs.src ?? '').trim();
        const resolvedSrc = rawSrc ? resolveNavigationUrl(rawSrc) : '';
        const kind = resolveSceneImageKind(resolvedSrc);
        const supported = Boolean(resolvedSrc && kind);
        const alreadySeen = supported && unique.has(resolvedSrc);
        const withinLimit = supported && unique.size < maxCount;
        const shouldPrime = supported && (alreadySeen || withinLimit);

        node.props = {
          ...props,
          resolvedSrc,
          imageAsset: imageAssetTagForUrl(
            resolvedSrc,
            kind,
            supported ? (shouldPrime ? 'queued' : 'deferred') : 'unsupported',
          ),
        };

        if (supported && !alreadySeen && withinLimit) {
          unique.add(resolvedSrc);
          urls.push(resolvedSrc);
          maybePrewarmUrl(resolvedSrc);
          const cached = imageTextureCache.get(resolvedSrc) || null;
          if (!cached || (cached.state !== 'ready' && cached.state !== 'loading' && cached.state !== 'error')) {
            enqueuePendingImageUrl(resolvedSrc, false);
          }
        }
      }

      const children = Array.isArray(node.children) ? node.children : [];
      for (let i = 0; i < children.length; i += 1) walk(children[i]);
    };

    walk(widgetTree);
    scheduleImagePrimeFlush();
    return urls;
  }

  function collectPrimeTargets(node, out = null) {
    const targets = Array.isArray(out) ? out : [];
    if (!node || typeof node !== 'object') return targets;
    if (isImageNode(node)) {
      const rawSrc = String(getNodeAttr(node, 'src') || '').trim();
      const resolvedSrc = rawSrc ? resolveNavigationUrl(rawSrc) : '';
      if (resolvedSrc && isSupportedSceneImageUrl(resolvedSrc)) {
        targets.push({ url: resolvedSrc, priority: false });
      }
    }
    const kids = Array.isArray(node.childNodes) ? node.childNodes : [];
    for (let i = 0; i < kids.length; i += 1) {
      collectPrimeTargets(kids[i], targets);
    }
    return targets;
  }

  function primeHtmlImageUrls(html, options = {}) {
    const source = String(html || '');
    if (!source) return;
    const maxCount = Math.max(0, Number(options.maxCount || DEFAULT_MAX_SCENE_IMAGES) | 0);
    if (maxCount <= 0) return [];
    let parsed;
    try {
      parsed = parse5.parse(source);
    } catch (_) {
      return;
    }
    const targets = collectPrimeTargets(parsed, []);
    const urls = [];
    const unique = new Set();
    for (let i = 0; i < targets.length; i += 1) {
      const entry = targets[i] || {};
      const resolvedSrc = String(entry.url || '').trim();
      if (!resolvedSrc) continue;
      if (!isSupportedSceneImageUrl(resolvedSrc)) continue;
      if (unique.has(resolvedSrc)) continue;
      if (unique.size >= maxCount) break;
      unique.add(resolvedSrc);
      urls.push(resolvedSrc);
      maybePrewarmUrl(resolvedSrc);
      const cached = imageTextureCache.get(resolvedSrc) || null;
      if (cached && (cached.state === 'ready' || cached.state === 'loading' || cached.state === 'error')) {
        continue;
      }
      enqueuePendingImageUrl(resolvedSrc, entry.priority === true);
    }
    scheduleImagePrimeFlush();
    return urls;
  }

  function traceHtmlVideoSources(html, options = {}) {
    const source = String(html || '');
    if (!source || !TRUESURFER_TRACE_VIDEO_SOURCES || !traceVideoSourceLine) return [];
    let parsed;
    try {
      parsed = parse5.parse(source);
    } catch (_) {
      return [];
    }
    return traceVideoSourcesFromParsed(parsed, {
      browserId,
      pageUrl: String(options.pageUrl || ''),
      logLine: traceVideoSourceLine,
      resolveUrl: resolveNavigationUrl,
    });
  }

  function summarizeImageUrls(urls) {
    const unique = new Set();
    const source = Array.isArray(urls) ? urls : [];
    for (let i = 0; i < source.length; i += 1) {
      const value = String(source[i] || '').trim();
      if (value) unique.add(value);
    }
    let pending = 0;
    let ready = 0;
    let error = 0;
    for (const url of unique) {
      const cached = imageTextureCache.get(url) || null;
      if (!cached) continue;
      if (cached.state === 'ready') {
        ready += 1;
      } else if (cached.state === 'error') {
        error += 1;
      } else {
        pending += 1;
      }
    }
    return {
      total: unique.size,
      pending,
      ready,
      error,
    };
  }

  function getCachedImageTexture(url) {
    const key = String(url || '').trim();
    if (!key) return null;
    return imageTextureCache.get(key) || null;
  }

  return {
    beginPageLoad,
    applyResourcesToRows,
    getCachedImageTexture,
    requestAssetsForRows,
    tagWidgetTreeImages,
    primeHtmlImageUrls,
    traceHtmlVideoSources,
    summarizeImageUrls,
  };
}
