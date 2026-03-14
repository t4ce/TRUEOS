import * as parse5 from 'parse5';
import { Buffer } from 'node:buffer';

const MAX_FETCHED_IMAGE_BINARIES = 3;

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

  const imageTextureCache = new Map();
  const imageTextureLoads = new Map();
  const fetchedImageBinaryUrls = new Set();
  const pendingImagePrimeUrls = new Set();
  let imagePrimeScheduled = false;

  function maybePrewarmUrl(url) {
    const value = String(url || '').trim();
    if (!value || value.startsWith('data:')) return;
    if (typeof host.__trueosPrewarmUrl !== 'function') return;
    try {
      host.__trueosPrewarmUrl(value);
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

  function resolveFetchableImageKind(url) {
    const normalizedUrl = String(url || '').toLowerCase();
    if (/\.png(?:$|[?#])/.test(normalizedUrl)) return 'png';
    if (/\.bmp(?:$|[?#])/.test(normalizedUrl)) return 'bmp';
    if (/\.svg(?:$|[?#])/.test(normalizedUrl)) return 'svg';
    return '';
  }

  function noteFetchedImageBinary(url) {
    const key = String(url || '').trim();
    if (!key || fetchedImageBinaryUrls.has(key)) return;
    if (fetchedImageBinaryUrls.size >= MAX_FETCHED_IMAGE_BINARIES) {
      raiseBrowserError(
        'TRUEOS_BROWSER_IMAGE_FETCH_LIMIT_REACHED',
        'Image fetch limit reached for binary image resources',
        { url: key, limit: MAX_FETCHED_IMAGE_BINARIES },
      );
    }
    fetchedImageBinaryUrls.add(key);
  }

  function queueRepaint() {
    try {
      paint();
    } catch (_) {}
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
          'Image fetch is restricted to png, bmp, or svg URLs',
          { url: value },
        );
      }
      noteFetchedImageBinary(value);
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
          const dims = readPngDimensions(bytes);
          const texId = Number(cmdStream.createTexturePngAsync(bytes) || 0);
          if (!Number.isFinite(texId) || texId <= 0) {
            throw new Error('inline image texture upload failed');
          }
          await waitForTextureReady(texId);
          ready = {
            state: 'ready',
            texId,
            url: cacheKey,
            mime: String(mime || 'image/png'),
            pixelWidth: Math.max(0, Number(dims.width || 0) | 0),
            pixelHeight: Math.max(0, Number(dims.height || 0) | 0),
            error: '',
          };
        } else {
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
        const failed = {
          state: 'error',
          texId: 0,
          url: cacheKey,
          mime: '',
          error: describeError(err),
        };
        imageTextureCache.set(cacheKey, failed);
        return failed;
      } finally {
        imageTextureLoads.delete(cacheKey);
        queueRepaint();
      }
    })();

    imageTextureLoads.set(cacheKey, task);
    return task;
  }

  function flushQueuedImagePrimes() {
    imagePrimeScheduled = false;
    if (pendingImagePrimeUrls.size <= 0) return;
    const urls = Array.from(pendingImagePrimeUrls);
    pendingImagePrimeUrls.clear();
    for (let i = 0; i < urls.length; i += 1) {
      const resolvedSrc = String(urls[i] || '').trim();
      if (!resolvedSrc) continue;
      const cached = imageTextureCache.get(resolvedSrc) || null;
      if (cached && (cached.state === 'ready' || cached.state === 'loading' || cached.state === 'error')) {
        continue;
      }
      void ensureImageTexture(resolvedSrc);
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
      pendingImagePrimeUrls.add(resolvedSrc);
    }
    scheduleImagePrimeFlush();
  }

  function collectImagePrimeUrls(node, out = null) {
    const urls = Array.isArray(out) ? out : [];
    if (!node || typeof node !== 'object') return urls;
    if (isImageNode(node)) {
      const rawSrc = String(getNodeAttr(node, 'src') || '').trim();
      const resolvedSrc = rawSrc ? resolveNavigationUrl(rawSrc) : '';
      if (resolvedSrc) {
        urls.push(resolvedSrc);
      }
    }
    const kids = Array.isArray(node.childNodes) ? node.childNodes : [];
    for (let i = 0; i < kids.length; i += 1) {
      collectImagePrimeUrls(kids[i], urls);
    }
    return urls;
  }

  function primeHtmlImageUrls(html) {
    const source = String(html || '');
    if (!source) return;
    let parsed;
    try {
      parsed = parse5.parse(source);
    } catch (_) {
      return;
    }
    const urls = collectImagePrimeUrls(parsed, []);
    for (let i = 0; i < urls.length; i += 1) {
      const resolvedSrc = String(urls[i] || '').trim();
      if (!resolvedSrc) continue;
      maybePrewarmUrl(resolvedSrc);
      const cached = imageTextureCache.get(resolvedSrc) || null;
      if (cached && (cached.state === 'ready' || cached.state === 'loading' || cached.state === 'error')) {
        continue;
      }
      pendingImagePrimeUrls.add(resolvedSrc);
    }
    scheduleImagePrimeFlush();
  }

  return {
    applyResourcesToRows,
    requestAssetsForRows,
    primeHtmlImageUrls,
  };
}
