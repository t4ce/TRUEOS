#![cfg(feature = "trueos")]

extern crate alloc;

use alloc::string::String;

pub mod ui_helloworld;
pub mod ui_html;

pub const DEFAULT_EMBEDDED_URL: &str = "trueos://ui/html";

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

    dst.push_str("G.__trueosUiHtml = ");
    dst.push_str(&html_lit);
    dst.push_str(";\n");

    dst.push_str("G.__trueosUiHelloWorldHtml = ");
    dst.push_str(&hello_lit);
    dst.push_str(";\n");

    dst.push_str(
        r#"
if (!G.__trueosBrowserEmbeddedRoutes || typeof G.__trueosBrowserEmbeddedRoutes !== 'object') {
    G.__trueosBrowserEmbeddedRoutes = Object.create(null);
}
G.__trueosBrowserEmbeddedRoutes['trueos://ui/html'] = G.__trueosUiHtml;
G.__trueosBrowserEmbeddedRoutes['trueos://ui/helloworld'] = G.__trueosUiHelloWorldHtml;
if (typeof G.__trueosBrowserUrl !== 'string' || !G.__trueosBrowserUrl) {
    G.__trueosBrowserUrl = 'trueos://ui/html';
}
if (typeof G.__trueosBrowserCurrentUrl !== 'string' || !G.__trueosBrowserCurrentUrl) {
    G.__trueosBrowserCurrentUrl = G.__trueosBrowserUrl;
}
if (typeof G.__trueosBrowserNavigate !== 'function') {
    G.__trueosBrowserNavigate = (event) => {
        const url = String(event && event.url || '').trim();
        const html = G.__trueosBrowserEmbeddedRoutes && typeof G.__trueosBrowserEmbeddedRoutes[url] === 'string'
            ? G.__trueosBrowserEmbeddedRoutes[url]
            : '';
        if (!html) {
            return { ok: 0, handled: 0, reason: 'embedded-route-not-found', url };
        }
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
    };
}
"#,
    );
}