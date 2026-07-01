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
const TRUESURFER_MEDIA_PROBE_REV = 'sabr-candidate-v1';

let truesurferSubsetProfile = null;
let extractDocumentArtifactsFn = null;
let buildCssStyleRefIndexFn = null;
let parseDocumentFn = null;
let domToWidgetsFn = null;
let collectWidgetStatsFn = null;
let flattenWidgetTreeFn = null;
let createRenderTreeTraceFn = null;
let summarizeRenderTreeTraceFn = null;
let currentNavigationUrl = '';
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

function publishLatestArtifacts() {
  if (!currentArtifactsState) return null;
  const nextArtifacts = Object.assign({}, currentArtifactsState);
  root.__trueosTruesurferLastArtifacts = nextArtifacts;
  currentArtifactsState = nextArtifacts;
  return nextArtifacts;
}

function resolveSceneImageKind(url) {
  const value = safeString(url).trim();
  if (value.startsWith('data:')) {
    if (/^data:image\/svg\+xml(?:;|,)/i.test(value)) return 'svg';
    if (/^data:image\/jpe?g(?:;|,)/i.test(value)) return 'jpeg';
    if (/^data:image\/png(?:;|,)/i.test(value)) return 'png';
    return '';
  }
  const lower = value.toLowerCase();
  if (/\.png(?:$|[?#])/.test(lower)) return 'png';
  if (/\.jpe?g(?:$|[?#])/.test(lower)) return 'jpeg';
  if (/\.svg(?:$|[?#])/.test(lower)) return 'svg';
  return '';
}

function beginBrowserAssetRefs() {
  if (typeof root.__trueosBrowserAssetRefsBegin !== 'function') return 0;
  try {
    return Number(root.__trueosBrowserAssetRefsBegin(browserId) || 0) | 0;
  } catch (_) {
    return -1;
  }
}

function pushBrowserAssetRef(tag, url, kind) {
  if (typeof root.__trueosBrowserAssetRef !== 'function') return -1;
  try {
    return Number(root.__trueosBrowserAssetRef(browserId, String(tag || ''), String(url || ''), String(kind || 'asset')) || 0) | 0;
  } catch (_) {
    return -1;
  }
}

function decodeEscapedUrlFragment(value) {
  let text = safeString(value);
  if (!text) return '';
  for (let pass = 0; pass < 3; pass += 1) {
    const before = text;
    text = text
      .replace(/\\u0026/g, '&')
      .replace(/\\\//g, '/')
      .replace(/&amp;/gi, '&');
    try {
      text = decodeURIComponent(text);
    } catch (_) {}
    if (text === before) break;
  }
  return text;
}

function parseQueryFields(value) {
  const fields = Object.create(null);
  const text = safeString(value);
  const parts = text.split('&');
  for (let index = 0; index < parts.length; index += 1) {
    const part = parts[index];
    if (!part) continue;
    const eq = part.indexOf('=');
    const rawKey = eq >= 0 ? part.slice(0, eq) : part;
    const rawValue = eq >= 0 ? part.slice(eq + 1) : '';
    let key = rawKey;
    let parsedValue = rawValue;
    try {
      key = decodeURIComponent(rawKey.replace(/\+/g, ' '));
    } catch (_) {}
    try {
      parsedValue = decodeURIComponent(rawValue.replace(/\+/g, ' '));
    } catch (_) {}
    if (key && fields[key] === undefined) {
      fields[key] = parsedValue;
    }
  }
  return fields;
}

function appendQueryParam(url, key, value) {
  const base = safeString(url);
  const separator = base.includes('?') ? '&' : '?';
  return `${base}${separator}${encodeURIComponent(key)}=${encodeURIComponent(value)}`;
}

function appendProbeParam(url, key, value) {
  const text = safeString(value);
  if (!text) return url;
  return appendQueryParam(url, key, text);
}

function resolveYoutubeUrl(rawUrl) {
  const url = decodeEscapedUrlFragment(rawUrl).trim();
  if (!url) return '';
  if (/^https?:\/\//i.test(url)) return url;
  if (url.startsWith('//')) return `https:${url}`;
  if (url.startsWith('/')) return `https://www.youtube.com${url}`;
  return resolveNavigationUrl(currentNavigationUrl || 'https://www.youtube.com/', url);
}

function youtubePlayerIdFromUrl(url) {
  const match = safeString(url).match(/\/s\/player\/([^/]+)\//);
  return match ? match[1] : '';
}

function youtubeVideoIdFromUrl(url) {
  const text = safeString(url);
  let match = text.match(/[?&]v=([^&#]+)/i);
  if (match) return decodeEscapedUrlFragment(match[1]).trim();
  match = text.match(/\/(?:embed|shorts)\/([^/?#&]+)/i);
  if (match) return decodeEscapedUrlFragment(match[1]).trim();
  match = text.match(/youtu\.be\/([^/?#&]+)/i);
  return match ? decodeEscapedUrlFragment(match[1]).trim() : '';
}

function mediaKindForUrl(url) {
  const lower = safeString(url).toLowerCase();
  if (lower.includes('/initplayback') || lower.includes('initplayback?')) return '';
  if (lower.includes('.m3u8') || lower.includes('mpegurl')) return 'video/mpegurl';
  if (lower.includes('mime=video/mp4') || lower.includes('.mp4')) return 'video/mp4';
  if (lower.includes('mime=video/webm') || lower.includes('.webm')) return 'video/webm';
  if (lower.includes('videoplayback')) return 'video/stream';
  return '';
}

function codecListForYoutubeFormat(format) {
  const mimeType = safeString(format && format.mimeType);
  const match = mimeType.match(/codecs\s*=\s*"([^"]+)"/i) || mimeType.match(/codecs\s*=\s*([^;]+)/i);
  if (!match) return '';
  return match[1].toLowerCase();
}

function isLikelyH264YoutubeFormat(format) {
  const mimeType = safeString(format && format.mimeType).toLowerCase();
  const codecs = codecListForYoutubeFormat(format);
  const itag = Number(format && format.itag);
  if (mimeType.includes('video/mp4') && (codecs.includes('avc1') || codecs.includes('avc3'))) return true;
  return itag === 18 || itag === 22 || itag === 37 || itag === 38 || itag === 82 || itag === 83 || itag === 84 || itag === 85;
}

function mediaKindForFormat(format, url) {
  const mimeType = safeString(format && format.mimeType).toLowerCase();
  if (mimeType.includes('mpegurl')) return 'video/mpegurl';
  if (mimeType.includes('video/mp4')) return 'video/mp4';
  if (mimeType.includes('video/webm')) return 'video/webm';
  return mediaKindForUrl(url);
}

function pushMediaCandidate(candidates, seen, rawUrl, source, meta) {
  const url = decodeEscapedUrlFragment(rawUrl).trim();
  if (!url || seen.has(url)) return;
  const kind = meta && meta.kind ? safeString(meta.kind) : mediaKindForUrl(url);
  if (!kind) return;
  seen.add(url);
  candidates.push({ url, kind, source, meta: meta || null });
}

function extractJsonObjectAt(source, openIndex) {
  if (openIndex < 0 || source.charCodeAt(openIndex) !== 123) return '';
  let depth = 0;
  let inString = false;
  let escaped = false;
  for (let index = openIndex; index < source.length; index += 1) {
    const code = source.charCodeAt(index);
    if (inString) {
      if (escaped) {
        escaped = false;
      } else if (code === 92) {
        escaped = true;
      } else if (code === 34) {
        inString = false;
      }
      continue;
    }
    if (code === 34) {
      inString = true;
    } else if (code === 123) {
      depth += 1;
    } else if (code === 125) {
      depth -= 1;
      if (depth === 0) {
        return source.slice(openIndex, index + 1);
      }
    }
  }
  return '';
}

function parseJsonText(text) {
  if (!text) return null;
  try {
    return JSON.parse(text);
  } catch (_) {
    return null;
  }
}

function decodeJsonStringLiteralFragment(fragment) {
  try {
    return JSON.parse(`"${fragment}"`);
  } catch (_) {
    return decodeEscapedUrlFragment(fragment);
  }
}

function collectYoutubePlayerResponses(source) {
  const responses = [];
  const seenStarts = new Set();
  const marker = 'ytInitialPlayerResponse';
  let searchAt = 0;
  while (searchAt < source.length) {
    const markerIndex = source.indexOf(marker, searchAt);
    if (markerIndex < 0) break;
    const openIndex = source.indexOf('{', markerIndex + marker.length);
    if (openIndex >= 0 && openIndex - markerIndex < 256 && !seenStarts.has(openIndex)) {
      const parsed = parseJsonText(extractJsonObjectAt(source, openIndex));
      if (parsed && typeof parsed === 'object') {
        seenStarts.add(openIndex);
        responses.push(parsed);
      }
    }
    searchAt = markerIndex + marker.length;
  }

  const playerResponseRe = /"playerResponse"\s*:\s*"((?:\\.|[^"\\])*)"/g;
  let match;
  while ((match = playerResponseRe.exec(source))) {
    const decoded = decodeJsonStringLiteralFragment(match[1]);
    const parsed = parseJsonText(decoded);
    if (parsed && typeof parsed === 'object') {
      responses.push(parsed);
    }
  }

  return responses;
}

function emptyYoutubeStats() {
  return {
    responses: 0,
    total: 0,
    regular: 0,
    adaptive: 0,
    usable: 0,
    directUrl: 0,
    ciphered: 0,
    undeciphered: 0,
    missingUrl: 0,
    h264: 0,
    h264Usable: 0,
    h264Ciphered: 0,
    h264Undeciphered: 0,
    h264MissingUrl: 0,
    serverAbr: 0,
    serverAbrUrl: '',
    dashManifest: 0,
    hlsManifest: 0,
    expiresInSeconds: '',
    playabilityStatus: '',
    playabilityReason: '',
    h264Formats: [],
  };
}

function mergeYoutubeStats(left, right) {
  return {
    responses: left.responses + right.responses,
    total: left.total + right.total,
    regular: left.regular + right.regular,
    adaptive: left.adaptive + right.adaptive,
    usable: left.usable + right.usable,
    directUrl: left.directUrl + right.directUrl,
    ciphered: left.ciphered + right.ciphered,
    undeciphered: left.undeciphered + right.undeciphered,
    missingUrl: left.missingUrl + right.missingUrl,
    h264: left.h264 + right.h264,
    h264Usable: left.h264Usable + right.h264Usable,
    h264Ciphered: left.h264Ciphered + right.h264Ciphered,
    h264Undeciphered: left.h264Undeciphered + right.h264Undeciphered,
    h264MissingUrl: left.h264MissingUrl + right.h264MissingUrl,
    serverAbr: left.serverAbr + right.serverAbr,
    serverAbrUrl: left.serverAbrUrl || right.serverAbrUrl,
    dashManifest: left.dashManifest + right.dashManifest,
    hlsManifest: left.hlsManifest + right.hlsManifest,
    expiresInSeconds: left.expiresInSeconds || right.expiresInSeconds,
    playabilityStatus: left.playabilityStatus || right.playabilityStatus,
    playabilityReason: left.playabilityReason || right.playabilityReason,
    h264Formats: left.h264Formats.concat(right.h264Formats || []),
  };
}

function collectYoutubeConfig(source) {
  const config = {
    apiKey: '',
    clientName: '',
    clientVersion: '',
    visitorData: '',
    hl: '',
    gl: '',
    rolloutToken: '',
    signatureTimestamp: '',
  };
  const applyConfigObject = (obj) => {
    if (!obj || typeof obj !== 'object') return;
    config.apiKey = config.apiKey || safeString(obj.INNERTUBE_API_KEY);
    config.clientName = config.clientName || safeString(obj.INNERTUBE_CLIENT_NAME || obj.INNERTUBE_CONTEXT_CLIENT_NAME);
    config.clientVersion = config.clientVersion || safeString(obj.INNERTUBE_CLIENT_VERSION || obj.CLIENT_VERSION);
    config.visitorData = config.visitorData || safeString(obj.VISITOR_DATA);
    config.hl = config.hl || safeString(obj.HL || obj.INNERTUBE_CONTEXT_HL);
    config.gl = config.gl || safeString(obj.GL || obj.INNERTUBE_CONTEXT_GL);
    config.rolloutToken = config.rolloutToken || safeString(obj.ROLLOUT_TOKEN);
    config.signatureTimestamp = config.signatureTimestamp || safeString(obj.STS || obj.signatureTimestamp);
    const contextClient = obj.INNERTUBE_CONTEXT && obj.INNERTUBE_CONTEXT.client;
    if (contextClient && typeof contextClient === 'object') {
      config.clientName = config.clientName || safeString(contextClient.clientName);
      config.clientVersion = config.clientVersion || safeString(contextClient.clientVersion);
      config.visitorData = config.visitorData || safeString(contextClient.visitorData);
      config.hl = config.hl || safeString(contextClient.hl);
      config.gl = config.gl || safeString(contextClient.gl);
    }
  };

  let searchAt = 0;
  while (searchAt < source.length) {
    const markerIndex = source.indexOf('ytcfg.set', searchAt);
    if (markerIndex < 0) break;
    const openIndex = source.indexOf('{', markerIndex);
    if (openIndex >= 0 && openIndex - markerIndex < 128) {
      applyConfigObject(parseJsonText(extractJsonObjectAt(source, openIndex)));
    }
    searchAt = markerIndex + 9;
  }

  const pairRe = /"(INNERTUBE_API_KEY|INNERTUBE_CLIENT_NAME|INNERTUBE_CONTEXT_CLIENT_NAME|INNERTUBE_CLIENT_VERSION|CLIENT_VERSION|VISITOR_DATA|HL|GL|ROLLOUT_TOKEN|STS|signatureTimestamp)"\s*:\s*("((?:\\.|[^"\\])*)"|[0-9]+)/g;
  let match;
  while ((match = pairRe.exec(source))) {
    const key = match[1];
    const value = match[3] !== undefined ? decodeJsonStringLiteralFragment(match[3]) : safeString(match[2]);
    if (key === 'INNERTUBE_API_KEY') config.apiKey = config.apiKey || value;
    else if (key === 'INNERTUBE_CLIENT_NAME' || key === 'INNERTUBE_CONTEXT_CLIENT_NAME') config.clientName = config.clientName || value;
    else if (key === 'INNERTUBE_CLIENT_VERSION' || key === 'CLIENT_VERSION') config.clientVersion = config.clientVersion || value;
    else if (key === 'VISITOR_DATA') config.visitorData = config.visitorData || value;
    else if (key === 'HL') config.hl = config.hl || value;
    else if (key === 'GL') config.gl = config.gl || value;
    else if (key === 'ROLLOUT_TOKEN') config.rolloutToken = config.rolloutToken || value;
    else if (key === 'STS' || key === 'signatureTimestamp') config.signatureTimestamp = config.signatureTimestamp || value;
  }
  return config;
}

function collectYoutubePlayerScriptRefs(source) {
  const refs = [];
  const seen = new Set();
  const pushRef = (rawUrl, sourceLabel) => {
    const url = resolveYoutubeUrl(rawUrl);
    if (!url || seen.has(url) || !/\/s\/player\/[^/]+\/.*base\.js/i.test(url)) return;
    seen.add(url);
    refs.push({
      url,
      playerId: youtubePlayerIdFromUrl(url),
      source: sourceLabel,
    });
  };

  const jsonRe = /"(?:jsUrl|PLAYER_JS_URL)"\s*:\s*"((?:\\.|[^"\\])*\/s\/player\/(?:\\.|[^"\\])*\/base\.js(?:\\.|[^"\\])*)"/gi;
  let match;
  while ((match = jsonRe.exec(source))) {
    pushRef(decodeJsonStringLiteralFragment(match[1]), 'json-player-js');
  }

  const scriptRe = /<script\b[^>]*\bsrc\s*=\s*(["'])([^"'>]*\/s\/player\/[^"'>]*\/base\.js[^"'>]*)\1/gi;
  while ((match = scriptRe.exec(source))) {
    pushRef(match[2], 'script-src');
  }

  return refs;
}

function urlFromYoutubeFormat(format) {
  const directUrl = safeString(format && format.url).trim();
  if (directUrl) {
    return { url: directUrl, source: 'direct', ciphered: false, decipherable: true };
  }
  const cipher = safeString((format && (format.signatureCipher || format.cipher)) || '');
  if (!cipher) {
    return { url: '', source: 'missing', ciphered: false, decipherable: false };
  }
  const fields = parseQueryFields(decodeEscapedUrlFragment(cipher));
  let url = safeString(fields.url).trim();
  if (!url) {
    return { url: '', source: 'cipher', ciphered: true, decipherable: false };
  }
  const sp = safeString(fields.sp || 'signature') || 'signature';
  const signature = safeString(fields.sig || fields.signature).trim();
  if (signature) {
    url = appendQueryParam(url, sp, signature);
    return { url, source: 'cipher', ciphered: true, decipherable: true };
  }
  return { url, source: 'cipher', ciphered: true, decipherable: false };
}

function pushYoutubeFormatCandidates(candidates, seen, playerResponse) {
  const stats = emptyYoutubeStats();
  stats.responses = 1;
  const playability = playerResponse && playerResponse.playabilityStatus;
  if (playability && typeof playability === 'object') {
    stats.playabilityStatus = safeString(playability.status);
    stats.playabilityReason = safeString(playability.reason);
  }
  const streamingData = playerResponse && playerResponse.streamingData;
  if (!streamingData || typeof streamingData !== 'object') {
    return stats;
  }
  stats.serverAbrUrl = safeString(streamingData.serverAbrStreamingUrl);
  stats.serverAbr = stats.serverAbrUrl ? 1 : 0;
  stats.dashManifest = safeString(streamingData.dashManifestUrl) ? 1 : 0;
  stats.hlsManifest = safeString(streamingData.hlsManifestUrl) ? 1 : 0;
  stats.expiresInSeconds = safeString(streamingData.expiresInSeconds);

  const groups = [
    ['format', Array.isArray(streamingData.formats) ? streamingData.formats : []],
    ['adaptive', Array.isArray(streamingData.adaptiveFormats) ? streamingData.adaptiveFormats : []],
  ];
  stats.regular = groups[0][1].length;
  stats.adaptive = groups[1][1].length;
  for (let groupIndex = 0; groupIndex < groups.length; groupIndex += 1) {
    const [groupName, formats] = groups[groupIndex];
    for (let index = 0; index < formats.length; index += 1) {
      const format = formats[index];
      if (!format || typeof format !== 'object') continue;
      const mimeType = safeString(format.mimeType);
      if (!mimeType.toLowerCase().includes('video/')) continue;
      stats.total += 1;
      const h264 = isLikelyH264YoutubeFormat(format);
      if (h264) stats.h264 += 1;
      const resolved = urlFromYoutubeFormat(format);
      if (resolved.source === 'direct') stats.directUrl += 1;
      if (resolved.ciphered) stats.ciphered += 1;
      if (h264 && resolved.ciphered) stats.h264Ciphered += 1;
      if (resolved.source === 'missing') {
        stats.missingUrl += 1;
        if (h264) stats.h264MissingUrl += 1;
      }
      const kind = resolved.url && (!resolved.ciphered || resolved.decipherable) ? mediaKindForFormat(format, resolved.url) : '';
      if (h264) {
        stats.h264Formats.push({
          group: groupName,
          itag: format.itag,
          mimeType,
          codecs: codecListForYoutubeFormat(format),
          qualityLabel: format.qualityLabel || format.quality || '',
          width: format.width || 0,
          height: format.height || 0,
          bitrate: format.bitrate || 0,
          ciphered: resolved.ciphered ? 1 : 0,
          decipherable: resolved.decipherable ? 1 : 0,
          hasUrl: resolved.url ? 1 : 0,
          urlSource: resolved.source,
          usable: kind ? 1 : 0,
        });
      }
      if (resolved.ciphered && !resolved.decipherable) {
        stats.undeciphered += 1;
        if (h264) stats.h264Undeciphered += 1;
        continue;
      }
      if (!resolved.url || !kind) continue;
      stats.usable += 1;
      if (!h264) continue;
      stats.h264Usable += 1;
      pushMediaCandidate(candidates, seen, resolved.url, `youtube-${groupName}`, {
        kind,
        itag: format.itag,
        mimeType,
        codecs: codecListForYoutubeFormat(format),
        h264: 1,
        qualityLabel: format.qualityLabel || format.quality || '',
        width: format.width || 0,
        height: format.height || 0,
        bitrate: format.bitrate || 0,
        ciphered: resolved.ciphered ? 1 : 0,
      });
    }
  }
  return stats;
}

function collectMediaCandidatesFromHtml(html) {
  const source = safeString(html);
  const candidates = [];
  const seen = new Set();
  const youtubeResponses = collectYoutubePlayerResponses(source);
  const youtubeConfig = collectYoutubeConfig(source);
  const playerScripts = collectYoutubePlayerScriptRefs(source);
  let youtubeStats = emptyYoutubeStats();
  for (let index = 0; index < youtubeResponses.length; index += 1) {
    const nextStats = pushYoutubeFormatCandidates(candidates, seen, youtubeResponses[index]);
    youtubeStats = mergeYoutubeStats(youtubeStats, nextStats);
  }

  let match;
  if (youtubeResponses.length === 0) {
    const directJsonUrlRe = /"url"\s*:\s*"([^"]*(?:videoplayback|\.mp4|\.m3u8)[^"]*)"/gi;
    while ((match = directJsonUrlRe.exec(source))) {
      pushMediaCandidate(candidates, seen, match[1], 'json-url');
    }

    const cipherRe = /"(?:signatureCipher|cipher)"\s*:\s*"([^"]+)"/gi;
    while ((match = cipherRe.exec(source))) {
      const decoded = decodeEscapedUrlFragment(match[1]);
      const fields = parseQueryFields(decoded);
      if (fields.url && (fields.sig || fields.signature)) {
        const sp = safeString(fields.sp || 'signature') || 'signature';
        const signature = safeString(fields.sig || fields.signature);
        pushMediaCandidate(candidates, seen, appendQueryParam(fields.url, sp, signature), 'signature-cipher-url');
      }
    }
  }

  const htmlMediaRe = /<(?:video|source)\b[^>]*\bsrc\s*=\s*(["'])([\s\S]*?)\1/gi;
  while ((match = htmlMediaRe.exec(source))) {
    pushMediaCandidate(candidates, seen, resolveNavigationUrl(currentNavigationUrl, match[2]), 'html-media-src');
  }
  if (candidates.length === 0 && youtubeStats.serverAbrUrl && youtubeConfig.apiKey && youtubeConfig.clientVersion) {
    let probeUrl = `innertube://player?video_id=${encodeURIComponent(youtubeVideoIdFromUrl(currentNavigationUrl))}`;
    probeUrl = appendProbeParam(probeUrl, 'api_key', youtubeConfig.apiKey);
    probeUrl = appendProbeParam(probeUrl, 'client_name', youtubeConfig.clientName || 'WEB');
    probeUrl = appendProbeParam(probeUrl, 'client_version', youtubeConfig.clientVersion);
    probeUrl = appendProbeParam(probeUrl, 'visitor_data', youtubeConfig.visitorData);
    probeUrl = appendProbeParam(probeUrl, 'hl', youtubeConfig.hl || 'en');
    probeUrl = appendProbeParam(probeUrl, 'gl', youtubeConfig.gl || 'US');
    probeUrl = appendProbeParam(probeUrl, 'watch_url', currentNavigationUrl);
    probeUrl = appendProbeParam(probeUrl, 'sts', youtubeConfig.signatureTimestamp);
    log(
      `[truesurfer media] browser=${browserId} youtube_innertube_candidate=1 probe_rev=${TRUESURFER_MEDIA_PROBE_REV} action=queue-player-direct-format-probe video_id=${youtubeVideoIdFromUrl(currentNavigationUrl)} client_name=${safeString(youtubeConfig.clientName || 'WEB')} client_version=${safeString(youtubeConfig.clientVersion)} sts=${safeString(youtubeConfig.signatureTimestamp || '0')}`,
    );
    pushMediaCandidate(candidates, seen, probeUrl, 'youtube-innertube', {
      kind: 'video/youtube-innertube',
      mimeType: 'application/json; profile=youtubei-player',
      codecs: '',
      h264: 0,
      qualityLabel: 'innertube',
      width: 0,
      height: 0,
      bitrate: 0,
      ciphered: 0,
    });
  }
  if (youtubeStats.serverAbrUrl) {
    log(
      `[truesurfer media] browser=${browserId} youtube_sabr_candidate=1 probe_rev=${TRUESURFER_MEDIA_PROBE_REV} action=queue-unsupported-sabr-probe url=${youtubeStats.serverAbrUrl}`,
    );
    pushMediaCandidate(candidates, seen, youtubeStats.serverAbrUrl, 'youtube-sabr', {
      kind: 'video/sabr',
      mimeType: 'application/vnd.youtube.sabr',
      codecs: '',
      h264: 0,
      qualityLabel: 'sabr',
      width: 0,
      height: 0,
      bitrate: 0,
      ciphered: 0,
    });
  }

  return { candidates, youtubeStats, youtubeConfig, playerScripts };
}

function tagHtmlMediaRefs(html) {
  const collected = collectMediaCandidatesFromHtml(html);
  const candidates = collected.candidates || [];
  const youtubeStats = collected.youtubeStats || emptyYoutubeStats();
  const youtubeConfig = collected.youtubeConfig || collectYoutubeConfig('');
  const playerScripts = collected.playerScripts || [];
  if (youtubeStats.responses > 0 || youtubeConfig.apiKey || youtubeConfig.clientName || youtubeConfig.clientVersion) {
    const apiKey = safeString(youtubeConfig.apiKey);
    const apiKeySuffix = apiKey ? apiKey.slice(Math.max(0, apiKey.length - 6)) : '';
    log(
      `[truesurfer media] browser=${browserId} youtube_context responses=${youtubeStats.responses} playability=${safeString(youtubeStats.playabilityStatus)} reason=${safeString(youtubeStats.playabilityReason)} client_name=${safeString(youtubeConfig.clientName)} client_version=${safeString(youtubeConfig.clientVersion)} api_key=${apiKey ? 1 : 0} api_key_len=${apiKey.length} api_key_suffix=${safeString(apiKeySuffix)} visitor=${youtubeConfig.visitorData ? 1 : 0} hl=${safeString(youtubeConfig.hl)} gl=${safeString(youtubeConfig.gl)} rollout=${youtubeConfig.rolloutToken ? 1 : 0} sts=${safeString(youtubeConfig.signatureTimestamp || '0')} url=${currentNavigationUrl}`,
    );
  }
  if (youtubeStats.responses > 0) {
    log(
      `[truesurfer media] browser=${browserId} youtube_streaming responses=${youtubeStats.responses} regular=${youtubeStats.regular} adaptive=${youtubeStats.adaptive} direct_url=${youtubeStats.directUrl} ciphered=${youtubeStats.ciphered} undeciphered=${youtubeStats.undeciphered} missing_url=${youtubeStats.missingUrl} h264_missing_url=${youtubeStats.h264MissingUrl} server_abr=${youtubeStats.serverAbr} dash=${youtubeStats.dashManifest} hls=${youtubeStats.hlsManifest} expires=${safeString(youtubeStats.expiresInSeconds)} url=${currentNavigationUrl}`,
    );
  }
  if (youtubeStats.serverAbrUrl) {
    log(
      `[truesurfer media] browser=${browserId} youtube_sabr=1 action=needs-sabr-or-innertube-player-fetch url=${youtubeStats.serverAbrUrl}`,
    );
  }
  for (let index = 0; index < playerScripts.length; index += 1) {
    const script = playerScripts[index];
    log(
      `[truesurfer media] browser=${browserId} youtube_player_js=${index + 1}/${playerScripts.length} source=${script.source} player_id=${script.playerId || '<unknown>'} url=${script.url}`,
    );
  }
  for (let index = 0; index < candidates.length; index += 1) {
    const candidate = candidates[index];
    const rc = pushBrowserAssetRef(`media:${index}`, candidate.url, candidate.kind);
    const meta = candidate.meta || {};
    const metaText = meta.mimeType
      ? ` itag=${safeString(meta.itag)} mime=${safeString(meta.mimeType)} codecs=${safeString(meta.codecs)} h264=${Number(meta.h264 || 0)} quality=${safeString(meta.qualityLabel)} size=${Number(meta.width || 0)}x${Number(meta.height || 0)} bitrate=${Number(meta.bitrate || 0)} ciphered=${Number(meta.ciphered || 0)}`
      : '';
    log(
      `[truesurfer media] browser=${browserId} candidate=${index + 1}/${candidates.length} rc=${rc} source=${candidate.source} kind=${candidate.kind}${metaText} url=${candidate.url}`,
    );
  }
  const h264Formats = youtubeStats.h264Formats || [];
  for (let index = 0; index < h264Formats.length; index += 1) {
    const format = h264Formats[index];
    log(
      `[truesurfer media] browser=${browserId} youtube_h264=${index + 1}/${h264Formats.length} group=${safeString(format.group)} itag=${safeString(format.itag)} usable=${Number(format.usable || 0)} has_url=${Number(format.hasUrl || 0)} url_source=${safeString(format.urlSource)} ciphered=${Number(format.ciphered || 0)} decipherable=${Number(format.decipherable || 0)} mime=${safeString(format.mimeType)} codecs=${safeString(format.codecs)} quality=${safeString(format.qualityLabel)} size=${Number(format.width || 0)}x${Number(format.height || 0)} bitrate=${Number(format.bitrate || 0)}`,
    );
  }
  if (youtubeStats.total > 0) {
    log(
      `[truesurfer media] browser=${browserId} youtube_formats=${youtubeStats.total} regular=${youtubeStats.regular} adaptive=${youtubeStats.adaptive} usable=${youtubeStats.usable} direct_url=${youtubeStats.directUrl} ciphered=${youtubeStats.ciphered} undeciphered=${youtubeStats.undeciphered} missing_url=${youtubeStats.missingUrl} h264=${youtubeStats.h264} h264_usable=${youtubeStats.h264Usable} h264_ciphered=${youtubeStats.h264Ciphered} h264_undeciphered=${youtubeStats.h264Undeciphered} h264_missing_url=${youtubeStats.h264MissingUrl} server_abr=${youtubeStats.serverAbr} player_js=${playerScripts.length} url=${currentNavigationUrl}`,
    );
  }
  if (candidates.length === 0) {
    log(`[truesurfer media] browser=${browserId} candidates=0 url=${currentNavigationUrl}`);
  }
  return {
    total: candidates.length,
    youtubeFormats: youtubeStats.total,
    youtubeUsable: youtubeStats.usable,
    youtubeDirectUrl: youtubeStats.directUrl,
    youtubeCiphered: youtubeStats.ciphered,
    youtubeUndeciphered: youtubeStats.undeciphered,
    youtubeMissingUrl: youtubeStats.missingUrl,
    youtubeH264: youtubeStats.h264,
    youtubeH264Usable: youtubeStats.h264Usable,
    youtubeH264Ciphered: youtubeStats.h264Ciphered,
    youtubeH264Undeciphered: youtubeStats.h264Undeciphered,
    youtubeH264MissingUrl: youtubeStats.h264MissingUrl,
    youtubeServerAbr: youtubeStats.serverAbr,
    youtubePlayerJs: playerScripts.length,
  };
}

function tagWidgetTreeAssetRefs(widgetTree) {
  const urls = [];
  const unique = new Set();
  beginBrowserAssetRefs();

  const walk = (node) => {
    if (!node || typeof node !== 'object') return;
    if (node.kind === 'widget' && String(node.tag || node.widget || '').toLowerCase() === 'img') {
      const props = node.props && typeof node.props === 'object' ? node.props : {};
      const attrs = node.attrs && typeof node.attrs === 'object' ? node.attrs : {};
      const rawSrc = String(props.src ?? attrs.src ?? '').trim();
      const resolvedSrc = rawSrc ? resolveNavigationUrl(currentNavigationUrl, rawSrc) : '';
      const kind = resolveSceneImageKind(resolvedSrc);
      const assetTag = String(node.key || attrs.id || rawSrc || `img:${urls.length}`);
      const supported = Boolean(resolvedSrc && kind);
      node.props = {
        ...props,
        resolvedSrc,
        imageAsset: {
          tag: assetTag,
          src: resolvedSrc,
          kind,
          state: supported ? 'referenced' : 'unsupported',
          texId: 0,
          mime: '',
          pixelWidth: 0,
          pixelHeight: 0,
          error: '',
        },
      };
      if (supported && !unique.has(assetTag)) {
        unique.add(assetTag);
        urls.push(resolvedSrc);
        pushBrowserAssetRef(assetTag, resolvedSrc, kind);
      }
    }
    const children = Array.isArray(node.children) ? node.children : [];
    for (let i = 0; i < children.length; i += 1) walk(children[i]);
  };

  walk(widgetTree);
  return {
    total: urls.length,
    pending: 0,
    ready: 0,
    error: 0,
  };
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
  if (renderTreeArtifactLogged) return null;
  if (typeof createRenderTreeTraceFn !== 'function') {
    log(`[truesurfer render-tree] browser=${browserId} status=unavailable url=${url}`);
    return null;
  }

  try {
    const artifact = createRenderTreeTraceFn(widgetTree, {
      source: 'parse5',
      bytes,
      baseUrl: url,
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
    return {
      renderHash: safeString(summary.renderHash || (artifact.renderTree && artifact.renderTree.hash) || ''),
      layoutHash: safeString(summary.layoutHash || (artifact.layoutTrace && artifact.layoutTrace.trace && artifact.layoutTrace.trace.layoutHash) || ''),
      renderTreeJson: JSON.stringify(artifact.renderTree || null),
      layoutTraceJson: artifact.layoutTrace ? JSON.stringify(artifact.layoutTrace) : '',
    };
  } catch (error) {
    const message = error && error.stack ? String(error.stack) : String(error || 'unknown render-tree error');
    log(`[truesurfer render-tree] browser=${browserId} status=failed error=${message} url=${url}`);
    return null;
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
    './css.mjs',
    'parse5',
    '../widlib/index.mjs',
    '../rendertree/renderToTree.mjs',
  ];
  for (let index = 0; index < imports.length; index += 1) {
    await helpers.prefetch(imports[index]);
  }

  const [extractMod, cssMod, parse5Mod, widMod, renderTreeMod] = await Promise.all([
    helpers.import('./truesurfer_extract.mjs'),
    helpers.import('./css.mjs'),
    helpers.import('parse5'),
    helpers.import('../widlib/index.mjs'),
    helpers.import('../rendertree/renderToTree.mjs'),
  ]);

  const extractReady = !!extractMod && typeof extractMod.extractDocumentArtifacts === 'function';
  const cssReady =
    !!cssMod
    && typeof cssMod.extractCssSection === 'function'
    && typeof cssMod.buildCssStyleRefIndex === 'function';
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
  if (!extractReady || !cssReady || !parseReady || !widgetsReady || !renderTreeReady) {
    throw new Error(
      `browser pipeline warmup incomplete extract_ready=${extractReady ? 1 : 0} css_ready=${cssReady ? 1 : 0} parse_ready=${parseReady ? 1 : 0} widgets_ready=${widgetsReady ? 1 : 0} render_tree_ready=${renderTreeReady ? 1 : 0}`,
    );
  }

  truesurferSubsetProfile = extractMod.TRUESURFER_SUBSET_PROFILE || null;
  extractDocumentArtifactsFn = extractMod.extractDocumentArtifacts;
  buildCssStyleRefIndexFn = cssMod.buildCssStyleRefIndex;
  parseDocumentFn = parse5Mod.parse;
  domToWidgetsFn = widMod.domToWidgets;
  collectWidgetStatsFn = widMod.collectWidgetStats;
  flattenWidgetTreeFn = widMod.flattenWidgetTree;
  createRenderTreeTraceFn = renderTreeMod.createRenderTreeTrace;
  summarizeRenderTreeTraceFn = renderTreeMod.summarizeRenderTreeTrace;
  root.__trueosTruesurferModules = {
    extractReady: 1,
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
      cssReady: modules.cssReady ? 1 : 0,
      parseReady: modules.parseReady ? 1 : 0,
      widgetsReady: modules.widgetsReady ? 1 : 0,
      renderTreeReady: modules.renderTreeReady ? 1 : 0,
      baseUrl: TRUESURFER_MODULE_BASE,
    };
    root.__trueosTruesurferReady = 1;
    log(
      `[truesurfer bootstrap] browser=${browserId} ready extract=${modules.extractReady ? 1 : 0} css=${modules.cssReady ? 1 : 0} parse=${modules.parseReady ? 1 : 0} widgets=${modules.widgetsReady ? 1 : 0} render_tree=${modules.renderTreeReady ? 1 : 0}`,
    );
  } catch (error) {
    const message = error && error.stack ? String(error.stack) : String(error || 'unknown bootstrap error');
    root.__trueosTruesurferWarmup = {
      status: 'error',
      extractReady: 0,
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

  if (
    typeof extractDocumentArtifactsFn !== 'function'
    || typeof buildCssStyleRefIndexFn !== 'function'
    || typeof parseDocumentFn !== 'function'
    || typeof domToWidgetsFn !== 'function'
  ) {
    return {
      ok: 0,
      bytes: html.length,
      lines,
      error: 'truesurfer extractor/widgets are not ready',
    };
  }

  try {
    const mediaStart = Date.now();
    log(`[truesurfer media] browser=${browserId} scan_begin probe_rev=${TRUESURFER_MEDIA_PROBE_REV} bytes=${html.length} lines=${lines} url=${url}`);
    const mediaSummary = tagHtmlMediaRefs(html);
    log(
      `[truesurfer media] browser=${browserId} scan_done candidates=${mediaSummary.total} youtube_formats=${mediaSummary.youtubeFormats} direct_url=${mediaSummary.youtubeDirectUrl} missing_url=${mediaSummary.youtubeMissingUrl} h264=${mediaSummary.youtubeH264} h264_usable=${mediaSummary.youtubeH264Usable} h264_missing_url=${mediaSummary.youtubeH264MissingUrl} server_abr=${mediaSummary.youtubeServerAbr} ms=${Date.now() - mediaStart} url=${url}`,
    );

    const widgetStart = Date.now();
    const parsedDocument = parseDocumentFn(html);
    const styleStart = Date.now();
    const styleIndex = buildCssStyleRefIndexFn(parsedDocument);
    const styleIndexMs = Date.now() - styleStart;
    const widgetTree = domToWidgetsFn(parsedDocument);
    const imageSummary = tagWidgetTreeAssetRefs(widgetTree);
    const parsed = extractDocumentArtifactsFn(html, { styleIndex, styleIndexMs });
    currentArtifactsState = {
      url,
      title: parsed.title || null,
      faviconUrl: resolveNavigationUrl(url, parsed.faviconHref) || null,
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
      imageSummary,
      mediaSummary,
    };
    publishLatestArtifacts();
    logSyncPipeline(url, parsed);
    root.__trueosTruesurferLastStyleIndex = parsed.styleIndex;
    logWidgetTable(url, widgetTree, widgetStart);
    const renderTreeArtifact = logRenderTreeArtifact(url, html.length, widgetTree, widgetStart);
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
      title: parsed.title || null,
      faviconUrl: resolveNavigationUrl(url, parsed.faviconHref) || null,
      shellBytes: parsed.shellBytes,
      bodyBytes: parsed.bodyBytes,
      styleCount: parsed.styleCount,
      styleBytes: parsed.styleBytes,
      styleSlotCount: parsed.styleSlotCount,
      styledNodeCount: parsed.styledNodeCount,
      styleRuleCount: parsed.styleRuleCount,
      scriptCount: parsed.scriptCount,
      scriptBytes: parsed.scriptBytes,
      imageSummary,
      mediaSummary,
      renderHash: renderTreeArtifact ? renderTreeArtifact.renderHash : null,
      layoutHash: renderTreeArtifact ? renderTreeArtifact.layoutHash : null,
      renderTreeJson: renderTreeArtifact ? renderTreeArtifact.renderTreeJson : null,
      layoutTraceJson: renderTreeArtifact ? renderTreeArtifact.layoutTraceJson : null,
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
