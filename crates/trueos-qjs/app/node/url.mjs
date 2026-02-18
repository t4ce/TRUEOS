// Minimal Node.js `url` module shim.
//
// Note: TRUEOS embedded modules map crates/trueos-qjs/app/** -> /qjs/**.
// Consumers should import this as "/qjs/node/url.mjs".

function parseQuery(search) {
  if (!search) return null;
  const q = search.startsWith('?') ? search.slice(1) : search;
  const params = new URLSearchParams(q);
  const obj = Object.create(null);
  for (const [k, v] of params) {
    const cur = obj[k];
    if (cur === undefined) obj[k] = v;
    else if (Array.isArray(cur)) cur.push(v);
    else obj[k] = [cur, v];
  }
  return obj;
}

export function parse(input, parseQueryString = false, _slashesDenoteHost = false) {
  const str = typeof input === 'string' ? input : String(input);
  let u;
  try {
    u = new URL(str);
  } catch {
    try {
      u = new URL(str, 'http://_');
    } catch {
      u = null;
    }
  }

  const out = {
    href: str,
    protocol: null,
    slashes: null,
    auth: null,
    host: null,
    port: null,
    hostname: null,
    hash: null,
    search: null,
    query: null,
    pathname: null,
    path: null,
  };

  if (!u) {
    const hashIdx = str.indexOf('#');
    const qIdx = str.indexOf('?');
    const pathEnd = qIdx != -1 ? qIdx : (hashIdx != -1 ? hashIdx : str.length);

    out.pathname = str.slice(0, pathEnd) || null;
    out.search = qIdx != -1 ? str.slice(qIdx, hashIdx != -1 ? hashIdx : str.length) : null;
    out.hash = hashIdx != -1 ? str.slice(hashIdx) : null;
    out.query = parseQueryString ? parseQuery(out.search) : (out.search ? out.search.slice(1) : null);
    out.path = (out.pathname || '') + (out.search || '');
    return out;
  }

  out.href = u.href;
  out.protocol = u.protocol;
  out.slashes = /:\/\//.test(str) ? true : null;
  out.hostname = u.hostname || null;
  out.port = u.port || null;
  out.host = u.host || null;
  out.pathname = u.pathname || null;
  out.search = u.search || null;
  out.hash = u.hash || null;
  out.query = parseQueryString ? parseQuery(out.search) : (out.search ? out.search.slice(1) : null);
  out.path = (out.pathname || '') + (out.search || '');
  return out;
}

export function format(urlObject) {
  if (typeof urlObject === 'string') return urlObject;
  if (urlObject && typeof urlObject === 'object') {
    if (typeof urlObject.href === 'string') return urlObject.href;
    if (typeof urlObject.toString === 'function') return urlObject.toString();
  }
  return String(urlObject);
}

export function resolve(from, to) {
  const base = typeof from === 'string' ? from : String(from);
  const rel = typeof to === 'string' ? to : String(to);
  try {
    return new URL(rel, base).toString();
  } catch {
    const slash = base.lastIndexOf('/');
    if (slash !== -1) return base.slice(0, slash + 1) + rel;
    return rel;
  }
}

const urlMod = { parse, format, resolve };
export default urlMod;
