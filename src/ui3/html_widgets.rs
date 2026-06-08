use alloc::string::String;

const ROOT_ID: u32 = 1;
const ROOT_IFRAME_ID: u32 = 2;
const H1_BLOCK_ID: u32 = 3;
const H1_TEXT_ID: u32 = 4;
const P_BLOCK_ID: u32 = 5;
const P_TEXT_ID: u32 = 6;

const KIND_CONTAINER: u32 = 0;
const KIND_TEXT: u32 = 2;

const ROOT_X: f32 = 32.0;
const ROOT_Y: f32 = 32.0;
const H1_BLOCK_X: f32 = 0.0;
const H1_BLOCK_Y: f32 = 0.0;
const H1_TEXT_X: f32 = 12.0;
const H1_TEXT_Y: f32 = 6.0;
const P_BLOCK_X: f32 = 0.0;
const P_BLOCK_Y: f32 = 56.0;
const P_TEXT_X: f32 = 12.0;
const P_TEXT_Y: f32 = 4.0;

const DEFAULT_H1: &str = "Hello UI3";
const DEFAULT_P: &str = "Parse5 native widget baseline.";

struct DemoTextDocument {
    h1: String,
    p: String,
}

impl DemoTextDocument {
    fn from_html(html: &str) -> Self {
        Self {
            h1: extract_tag_text(html, "h1").unwrap_or_else(|| String::from(DEFAULT_H1)),
            p: extract_tag_text(html, "p").unwrap_or_else(|| String::from(DEFAULT_P)),
        }
    }
}

pub(super) fn submit_h1_p_text_widget_scene(browser_id: u32, html: &str) -> (u32, u32) {
    let doc = DemoTextDocument::from_html(html);
    let mut submitted = 0u32;

    if submit_begin(browser_id, ROOT_ID) == 0 {
        return (0, ROOT_ID);
    }

    submitted += submit_node(browser_id, ROOT_IFRAME_ID, KIND_CONTAINER);
    submitted += submit_node(browser_id, H1_BLOCK_ID, KIND_CONTAINER);
    submitted += submit_node(browser_id, H1_TEXT_ID, KIND_TEXT);
    submitted += submit_node(browser_id, P_BLOCK_ID, KIND_CONTAINER);
    submitted += submit_node(browser_id, P_TEXT_ID, KIND_TEXT);

    submitted += submit_add_child(browser_id, ROOT_ID, ROOT_IFRAME_ID);
    submitted += submit_add_child(browser_id, ROOT_IFRAME_ID, H1_BLOCK_ID);
    submitted += submit_add_child(browser_id, H1_BLOCK_ID, H1_TEXT_ID);
    submitted += submit_add_child(browser_id, ROOT_IFRAME_ID, P_BLOCK_ID);
    submitted += submit_add_child(browser_id, P_BLOCK_ID, P_TEXT_ID);

    submitted += submit_position(browser_id, ROOT_IFRAME_ID, ROOT_X, ROOT_Y);
    submitted += submit_position(browser_id, H1_BLOCK_ID, H1_BLOCK_X, H1_BLOCK_Y);
    submitted += submit_position(browser_id, H1_TEXT_ID, H1_TEXT_X, H1_TEXT_Y);
    submitted += submit_position(browser_id, P_BLOCK_ID, P_BLOCK_X, P_BLOCK_Y);
    submitted += submit_position(browser_id, P_TEXT_ID, P_TEXT_X, P_TEXT_Y);

    submitted += submit_text_fill(browser_id, H1_TEXT_ID, 0x111111, 1.0);
    submitted += submit_text(browser_id, H1_TEXT_ID, doc.h1.as_str());
    submitted += submit_text_fill(browser_id, P_TEXT_ID, 0x333333, 1.0);
    submitted += submit_text(browser_id, P_TEXT_ID, doc.p.as_str());

    if submit_render(browser_id, ROOT_ID) > 0 {
        submitted = submitted.saturating_add(1);
    }

    crate::log!(
        "ui3-html-widgets: h1-p text widget scene browser={} root={} ops={} h1_bytes={} p_bytes={} structure=root>iframe>h1>text,p>text\n",
        browser_id,
        ROOT_ID,
        submitted,
        doc.h1.len(),
        doc.p.len()
    );

    (submitted, ROOT_ID)
}

fn submit_node(browser_id: u32, node_id: u32, kind: u32) -> u32 {
    pixi_op(browser_id, 1, node_id, kind as f32, 0.0, 0.0, 0.0, None)
}

fn submit_add_child(browser_id: u32, parent: u32, child: u32) -> u32 {
    pixi_op(browser_id, 2, parent, child as f32, 0.0, 0.0, 0.0, None)
}

fn submit_position(browser_id: u32, node_id: u32, x: f32, y: f32) -> u32 {
    pixi_op(browser_id, 3, node_id, x, y, 0.0, 0.0, None)
}

fn submit_text(browser_id: u32, node_id: u32, text: &str) -> u32 {
    pixi_op(browser_id, 8, node_id, 0.0, 0.0, 0.0, 0.0, Some(text))
}

fn submit_text_fill(browser_id: u32, node_id: u32, rgb: u32, alpha: f32) -> u32 {
    pixi_op(browser_id, 9, node_id, rgb as f32, alpha, 0.0, 0.0, None)
}

fn submit_begin(browser_id: u32, root_id: u32) -> u32 {
    pixi_op(browser_id, 0, root_id, 0.0, 0.0, 0.0, 0.0, None)
}

fn submit_render(browser_id: u32, root_id: u32) -> u32 {
    pixi_op(browser_id, 21, root_id, 0.0, 0.0, 0.0, 0.0, None)
}

fn pixi_op(
    browser_id: u32,
    op_code: u32,
    node: u32,
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    text: Option<&str>,
) -> u32 {
    let (text_ptr, text_len) = text
        .map(|text| (text.as_ptr(), text.len()))
        .unwrap_or((core::ptr::null(), 0));
    (unsafe {
        super::trueos_cabi_ui3_pixi_op(browser_id, op_code, node, a, b, c, d, text_ptr, text_len)
    } >= 0) as u32
}

fn extract_tag_text(html: &str, tag: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let open_start = find_open_tag(lower.as_str(), tag)?;
    let open_end = html[open_start..].find('>')? + open_start;
    let close = {
        let mut close_tag = String::from("</");
        close_tag.push_str(tag);
        close_tag.push('>');
        lower[open_end + 1..].find(close_tag.as_str())? + open_end + 1
    };
    let inner = &html[open_end + 1..close];
    let stripped = strip_tags(inner);
    let decoded = decode_basic_entities(stripped.as_str());
    let collapsed = collapse_whitespace(decoded.as_str());
    (!collapsed.is_empty()).then_some(collapsed)
}

fn find_open_tag(lower: &str, tag: &str) -> Option<usize> {
    let mut needle = String::from("<");
    needle.push_str(tag);
    let mut cursor = 0usize;
    while cursor < lower.len() {
        let idx = lower[cursor..].find(needle.as_str())? + cursor;
        let next = lower
            .as_bytes()
            .get(idx + needle.len())
            .copied()
            .unwrap_or(b'>');
        if next == b'>' || next == b'/' || next.is_ascii_whitespace() {
            return Some(idx);
        }
        cursor = idx + needle.len();
    }
    None
}

fn strip_tags(input: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

fn decode_basic_entities(input: &str) -> String {
    input
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

fn collapse_whitespace(input: &str) -> String {
    let mut out = String::new();
    let mut pending_space = false;
    for ch in input.chars() {
        if ch.is_whitespace() {
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        out.push(ch);
    }
    out
}
