#![cfg(feature = "trueos")]

extern crate alloc;

use alloc::string::String;

pub mod svg_html;
pub mod ui_helloworld;
pub mod ui_html;

pub const DEFAULT_EMBEDDED_URL: &str = "trueos://ui/html";
pub const SVG_EMBEDDED_URL: &str = "trueos://ui/svg-demo";

fn js_single_quoted_literal(src: &str) -> String {
    let mut out = String::with_capacity(src.len() + 32);
    out.push('\'');
    for ch in src.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(ch),
        }
    }
    out.push('\'');
    out
}

pub fn append_embedded_browser_globals_js(dst: &mut String) {
    let html_lit = js_single_quoted_literal(ui_html::UI_HTML);
    let hello_lit = js_single_quoted_literal(ui_helloworld::UI_HELLOWORLD_HTML);
    let svg_lit = js_single_quoted_literal(svg_html::SVG_HTML);

    dst.push_str("G.__trueosUiHtml = ");
    dst.push_str(&html_lit);
    dst.push_str(";\n");

    dst.push_str("G.__trueosUiHelloWorldHtml = ");
    dst.push_str(&hello_lit);
    dst.push_str(";\n");

    dst.push_str("G.__trueosUiSvgHtml = ");
    dst.push_str(&svg_lit);
    dst.push_str(";\n");

    dst.push_str(
        r#"
if (!G.__trueosBrowserEmbeddedRoutes || typeof G.__trueosBrowserEmbeddedRoutes !== 'object') {
    G.__trueosBrowserEmbeddedRoutes = Object.create(null);
}
G.__trueosBrowserEmbeddedRoutes['trueos://ui/html'] = G.__trueosUiHtml;
G.__trueosBrowserEmbeddedRoutes['trueos://ui/helloworld'] = G.__trueosUiHelloWorldHtml;
G.__trueosBrowserEmbeddedRoutes['trueos://ui/svg-demo'] = G.__trueosUiSvgHtml;
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
