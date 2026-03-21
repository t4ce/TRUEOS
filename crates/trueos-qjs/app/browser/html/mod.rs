#![cfg(feature = "trueos")]

extern crate alloc;

use alloc::string::String;

pub const SVG_EMBEDDED_URL: &str = "trueos://ui/svg-demo";

pub fn append_embedded_browser_globals_js(dst: &mut String) {
    dst.push_str(
        r#"
if (!G.__trueosBrowserEmbeddedRoutes || typeof G.__trueosBrowserEmbeddedRoutes !== 'object') {
    G.__trueosBrowserEmbeddedRoutes = Object.create(null);
}
if (typeof G.__trueosBrowserAllowHtmlFallback !== 'boolean') {
    G.__trueosBrowserAllowHtmlFallback = false;
}
if (typeof G.__trueosBrowserNavigate !== 'function') {
    G.__trueosBrowserNavigate = (event) => {
        const url = String(event && event.url || '').trim();
        const html = G.__trueosBrowserEmbeddedRoutes && typeof G.__trueosBrowserEmbeddedRoutes[url] === 'string'
            ? G.__trueosBrowserEmbeddedRoutes[url]
            : '';
        if (html) {
            G.__trueosBrowserCurrentUrl = url;
            G.__trueosBrowserUrl = url;
            if (G.__trueosBrowser && typeof G.__trueosBrowser.setCurrentPageUrl === 'function') {
                G.__trueosBrowser.setCurrentPageUrl(url);
            }
            if (G.__trueosBrowser && typeof G.__trueosBrowser.setHtml === 'function') {
                G.__trueosBrowser.setHtml(html);
            } else {
                G.__trueosUiHtml = html;
            }
            return { ok: 1, handled: 1, loaded: 1, embedded: 1, url };
        }
        const submit = G.__trueosBrowserNavigateSubmit;
        if (typeof submit !== 'function') {
            return { ok: 0, handled: 0, reason: 'navigate-submit-unavailable', url };
        }
        const browserInstanceId = Number(G.__trueosBrowserInstanceId || 1) || 1;
        const opId = Number(submit(url, browserInstanceId) || 0) | 0;
        if (!(opId > 0)) {
            return { ok: 0, handled: 0, reason: 'navigate-submit-failed', url };
        }
        G.__trueosBrowserCurrentUrl = url;
        G.__trueosBrowserUrl = url;
        if (G.__trueosBrowser && typeof G.__trueosBrowser.setCurrentPageUrl === 'function') {
            G.__trueosBrowser.setCurrentPageUrl(url);
        }
        return {
            ok: 1,
            handled: 1,
            queued: 1,
            loading: 1,
            external: 1,
            opId,
            url,
        };
    };
}
"#,
    );
}
