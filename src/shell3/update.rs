//! Visible strip snapshots and incremental update generation for Shell3.

use alloc::{string::{String, ToString}, vec::Vec};

use super::{SpecialRows, StripSide};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SegmentUpdate {
    pub row: SpecialRows,
    pub side: StripSide,
    pub offset: usize,
    pub remove: usize,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateBatch {
    pub layout_changed: bool,
    pub old_size: (usize, usize),
    pub new_size: (usize, usize),
    pub segments: Vec<SegmentUpdate>,
}

pub type UpdateCallback = fn(&UpdateBatch);

#[derive(Clone, Debug, PartialEq, Eq)]
struct VisibleRow {
    left: String,
    right: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Snapshot {
    size: (usize, usize),
    layout_generation: usize,
    rows: [VisibleRow; 3],
}

impl Snapshot {
    pub(super) fn new(
        size: (usize, usize),
        layout_generation: usize,
        strips: [(&str, &str); 3],
        columns: usize,
    ) -> Self {
        Self {
            size,
            layout_generation,
            rows: strips.map(|(left, right)| visible_lr_strips(left, right, columns)),
        }
    }
}

pub(super) fn take_updates(
    baseline: &mut Snapshot,
    current: Snapshot,
    callbacks: &[UpdateCallback],
) -> UpdateBatch {
    let old = core::mem::replace(baseline, current.clone());
    let mut segments = Vec::new();

    for index in 0..3 {
        let row = row_from_index(index);
        let old_row = &old.rows[row_index(row)];
        let new_row = &current.rows[row_index(row)];

        if let Some(update) =
            diff_visible_segment(row, StripSide::Left, &old_row.left, &new_row.left)
        {
            segments.push(update);
        }
        if let Some(update) =
            diff_visible_segment(row, StripSide::Right, &old_row.right, &new_row.right)
        {
            segments.push(update);
        }
    }

    let batch = UpdateBatch {
        layout_changed: old.layout_generation != current.layout_generation,
        old_size: old.size,
        new_size: current.size,
        segments,
    };

    if batch.layout_changed || !batch.segments.is_empty() {
        for callback in callbacks {
            callback(&batch);
        }
    }

    batch
}

pub(super) fn fit_lr_strips(left: &str, right: &str, columns: usize) -> String {
    let visible = visible_lr_strips(left, right, columns);
    let left_len = visible_len(&visible.left);
    let right_len = visible_len(&visible.right);
    let original_left_len = visible_len(left);
    let original_right_len = visible_len(right);
    let overflowed = original_left_len + original_right_len > columns
        && original_left_len > 0
        && original_right_len > 0;

    if overflowed {
        let mut output = String::with_capacity(columns);
        output.push_str(&visible.left);
        output.push(super::SpecialSeperator);
        output.push_str(&visible.right);
        return output;
    }

    let gap = columns.saturating_sub(left_len + right_len);
    let mut output = String::with_capacity(columns);
    output.push_str(&visible.left);
    output.extend(core::iter::repeat(' ').take(gap));
    output.push_str(&visible.right);
    output
}

fn row_index(row: SpecialRows) -> usize {
    match row {
        SpecialRows::TitleRow => 0,
        SpecialRows::StatusRow => 1,
        SpecialRows::PromtRow => 2,
    }
}

fn row_from_index(index: usize) -> SpecialRows {
    match index {
        0 => SpecialRows::TitleRow,
        1 => SpecialRows::StatusRow,
        _ => SpecialRows::PromtRow,
    }
}

fn styled_glyphs(text: &str) -> Vec<String> {
    let mut glyphs = Vec::new();
    let mut pending = String::new();
    for ch in text.chars() {
        pending.push(ch);
        glyphs.push(core::mem::take(&mut pending));
    }

    if !pending.is_empty() {
        if let Some(last) = glyphs.last_mut() {
            last.push_str(&pending);
        }
    }

    glyphs
}

fn visible_len(text: &str) -> usize {
    styled_glyphs(text).len()
}

fn take_visible(text: &str, limit: usize) -> String {
    styled_glyphs(text)
        .into_iter()
        .take(limit)
        .collect::<Vec<_>>()
        .concat()
}

fn visible_lr_strips(left: &str, right: &str, columns: usize) -> VisibleRow {
    let left_len = visible_len(left);
    let right_len = visible_len(right);

    if left_len + right_len <= columns {
        return VisibleRow {
            left: left.to_string(),
            right: right.to_string(),
        };
    }
    if left_len == 0 {
        return VisibleRow {
            left: String::new(),
            right: take_visible(right, columns),
        };
    }
    if right_len == 0 {
        return VisibleRow {
            left: take_visible(left, columns),
            right: String::new(),
        };
    }

    let usable = columns.saturating_sub(1);
    let left_half = usable / 2;
    let right_half = usable - left_half;
    let (left_limit, right_limit) = if left_len < left_half {
        (left_len, usable - left_len)
    } else if right_len < right_half {
        (usable - right_len, right_len)
    } else {
        (left_half, right_half)
    };

    VisibleRow {
        left: take_visible(left, left_limit),
        right: take_visible(right, right_limit),
    }
}

fn diff_visible_segment(
    row: SpecialRows,
    side: StripSide,
    old: &str,
    new: &str,
) -> Option<SegmentUpdate> {
    if old == new {
        return None;
    }

    let old_glyphs = styled_glyphs(old);
    let new_glyphs = styled_glyphs(new);
    let mut prefix = 0;
    let prefix_limit = old_glyphs.len().min(new_glyphs.len());
    while prefix < prefix_limit && old_glyphs[prefix] == new_glyphs[prefix] {
        prefix += 1;
    }

    let mut suffix = 0;
    let old_remaining = old_glyphs.len() - prefix;
    let new_remaining = new_glyphs.len() - prefix;
    while suffix < old_remaining.min(new_remaining)
        && old_glyphs[old_glyphs.len() - 1 - suffix] == new_glyphs[new_glyphs.len() - 1 - suffix]
    {
        suffix += 1;
    }

    let old_end = old_glyphs.len() - suffix;
    let new_end = new_glyphs.len() - suffix;
    Some(SegmentUpdate {
        row,
        side,
        offset: prefix,
        remove: old_end - prefix,
        text: new_glyphs[prefix..new_end].concat(),
    })
}
