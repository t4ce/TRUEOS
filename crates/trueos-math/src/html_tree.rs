#![cfg(any(feature = "alloc", test))]

use alloc::string::String;
use alloc::vec::Vec;

use crate::ascii_tree::{AsciiTreeTraversal, Frame};

pub(crate) const DEFAULT_MAX_ITEMS: usize = 1000;

pub(crate) fn tree_to_html_string<T, F>(
    tree: &T,
    root: T::NodeId,
    max_items: usize,
    mut fmt_node: F,
) -> String
where
    T: AsciiTreeTraversal,
    F: FnMut(T::NodeId, &mut String),
{
    let mut out = String::new();
    out.push_str("<ul>");

    if max_items == 0 || !tree.is_valid(root) {
        out.push_str("</ul>");
        return out;
    }

    let mut stack: Vec<Frame<T::NodeId>> = Vec::new();
    stack.push(Frame {
        id: root,
        depth: 0,
        is_last: true,
    });

    let mut printed = 0usize;
    let mut have_any = false;
    let mut prev_depth = 0usize;

    while let Some(Frame { id, depth, .. }) = stack.pop() {
        if printed >= max_items {
            begin_node_at_depth(&mut out, depth, &mut have_any, &mut prev_depth);
            push_escaped(&mut out, "... (max ");
            push_usize(&mut out, max_items);
            push_escaped(&mut out, " entries)");
            // Do not descend further.
            break;
        }

        begin_node_at_depth(&mut out, depth, &mut have_any, &mut prev_depth);

        let mut label = String::new();
        fmt_node(id, &mut label);
        push_escaped(&mut out, label.as_str());

        printed += 1;
        tree.push_children_rev(id, depth + 1, &mut stack);
    }

    if have_any {
        out.push_str("</li>");
        for _ in 0..prev_depth {
            out.push_str("</ul></li>");
        }
    }

    out.push_str("</ul>");
    out
}

fn begin_node_at_depth(out: &mut String, depth: usize, have_any: &mut bool, prev_depth: &mut usize) {
    if !*have_any {
        *have_any = true;
        *prev_depth = depth;
        out.push_str("<li>");
        return;
    }

    if depth == *prev_depth {
        out.push_str("</li><li>");
    } else if depth == *prev_depth + 1 {
        out.push_str("<ul><li>");
    } else if depth < *prev_depth {
        out.push_str("</li>");
        for _ in 0..(*prev_depth - depth) {
            out.push_str("</ul></li>");
        }
        out.push_str("<li>");
    } else {
        // Depth should not normally jump by more than 1 in a well-formed traversal.
        // Still, generate something sane.
        for _ in 0..(depth - *prev_depth) {
            out.push_str("<ul>");
        }
        out.push_str("<li>");
    }

    *prev_depth = depth;
}

fn push_usize(out: &mut String, mut v: usize) {
    // Avoid pulling in formatting; just write decimal.
    let mut buf = [0u8; 20];
    let mut i = buf.len();

    if v == 0 {
        out.push('0');
        return;
    }

    while v > 0 {
        let digit = (v % 10) as u8;
        v /= 10;
        i -= 1;
        buf[i] = b'0' + digit;
    }

    // SAFETY: only ASCII digits written.
    out.push_str(unsafe { core::str::from_utf8_unchecked(&buf[i..]) });
}

fn push_escaped(out: &mut String, s: &str) {
    // Escape minimal HTML entities so user-provided text can't break markup.
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
}
