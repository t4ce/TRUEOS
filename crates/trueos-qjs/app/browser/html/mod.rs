#![cfg(feature = "trueos")]

extern crate alloc;

use alloc::string::String;

pub mod ui_html;

pub const UI_EMBEDDED_URL: &str = "trueos://ui/ui-html";
pub const SVG_EMBEDDED_URL: &str = "trueos://ui/svg-demo";

fn append_js_string_literal(dst: &mut String, value: &str) {
    dst.push('"');
    for ch in value.chars() {
        match ch {
            '"' => dst.push_str("\\\""),
            '\\' => dst.push_str("\\\\"),
            '\u{0008}' => dst.push_str("\\b"),
            '\u{000C}' => dst.push_str("\\f"),
            '\n' => dst.push_str("\\n"),
            '\r' => dst.push_str("\\r"),
            '\t' => dst.push_str("\\t"),
            ch if ch <= '\u{001F}' => {
                dst.push_str(alloc::format!("\\u{:04X}", ch as u32).as_str());
            }
            _ => dst.push(ch),
        }
    }
    dst.push('"');
}

fn append_embedded_route(dst: &mut String, url: &str, html: &str) {
    dst.push_str("G.__trueosBrowserEmbeddedRoutes[");
    append_js_string_literal(dst, url);
    dst.push_str("] = ");
    append_js_string_literal(dst, html);
    dst.push_str(";\n");
}

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
        const browserInstanceId = Number(G.__trueosBrowserInstanceId || 1) || 1;
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
            } else if (browserInstanceId === 1) {
                G.__trueosUiHtml = html;
            }
            return { ok: 1, handled: 1, loaded: 1, embedded: 1, url };
        }
        const submit = G.__trueosBrowserNavigateSubmit;
        if (typeof submit !== 'function') {
            return { ok: 0, handled: 0, reason: 'navigate-submit-unavailable', url };
        }
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
    append_embedded_route(dst, UI_EMBEDDED_URL, ui_html::UI_HTML);
}
