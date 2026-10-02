//! Visible strip snapshots and incremental update generation for Shell3.

use alloc::{string::String, vec::Vec};

use super::{MetaFmtStr, RgbaColor, SpecialRows, StripSide};

/// One visible cell: Unicode character and optional MetaFmt foreground color.
pub(super) type RenderedLine = Vec<(char, Option<RgbaColor>)>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SegmentUpdate {
    pub row: SpecialRows,
    pub side: StripSide,
    pub offset: usize,
    pub remove: usize,
    pub text: String,
    pub colors: Vec<Option<RgbaColor>>,
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
    rendered: RenderedLine,
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
        strips: [(&[MetaFmtStr], &[MetaFmtStr]); 3],
        columns: usize,
    ) -> Self {
        Self {
            size,
            layout_generation,
            rows: strips.map(|(left, right)| VisibleRow {
                rendered: fit_meta_strips(left, right, columns),
            }),
        }
    }

    pub(super) fn size(&self) -> (usize, usize) {
        self.size
    }

    pub(super) fn rendered_lines(&self) -> [RenderedLine; 3] {
        self.rows.each_ref().map(|row| row.rendered.clone())
    }
}

pub(super) fn take_updates(
    baseline: &mut Snapshot,
    current: Snapshot,
    callbacks: &[UpdateCallback],
) -> UpdateBatch {
    let batch = build_updates(baseline, &current, callbacks);
    *baseline = current;

    batch
}

pub(super) fn build_updates(
    baseline: &Snapshot,
    current: &Snapshot,
    callbacks: &[UpdateCallback],
) -> UpdateBatch {
    let mut segments = Vec::new();
    for index in 0..3 {
        let row = row_from_index(index);
        if let Some(update) = diff_visible_segment(
            row,
            StripSide::Left,
            &baseline.rows[index].rendered,
            &current.rows[index].rendered,
        ) {
            segments.push(update);
        }
    }

    let batch = UpdateBatch {
        layout_changed: baseline.layout_generation != current.layout_generation,
        old_size: baseline.size,
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

pub(super) fn diff_rendered_lines(
    previous: Option<&[RenderedLine; 3]>,
    current: &[RenderedLine; 3],
) -> Vec<SegmentUpdate> {
    let mut updates = Vec::new();
    for (index, line) in current.iter().enumerate() {
        let row = row_from_index(index);
        let old = previous.map(|lines| lines[index].as_slice()).unwrap_or(&[]);
        if let Some(update) = diff_visible_segment(row, StripSide::Left, old, line) {
            updates.push(update);
        }
    }
    updates
}

pub(super) fn fit_meta_strips(left: &[MetaFmtStr], right: &[MetaFmtStr], columns: usize) -> RenderedLine {
    let cells = |runs: &[MetaFmtStr]| -> RenderedLine {
        runs.iter().flat_map(|run| run.text.chars().map(|ch| (ch, run.color))).collect()
    };
    let mut left = cells(left);
    let mut right = cells(right);
    if left.len() + right.len() <= columns {
        left.resize(columns - right.len(), (' ', None));
    } else if left.is_empty() {
        right.truncate(columns);
    } else if right.is_empty() {
        left.truncate(columns);
    } else if columns == 0 {
        return Vec::new();
    } else {
        let usable = columns - 1;
        let half = usable / 2;
        let (l, r) = if left.len() < half {
            (left.len(), usable - left.len())
        } else if right.len() < usable - half {
            (usable - right.len(), right.len())
        } else {
            (half, usable - half)
        };
        left.truncate(l);
        right.truncate(r);
        left.push((super::SpecialSeperator, None));
    }
    left.extend(right);
    left
}

fn row_from_index(index: usize) -> SpecialRows {
    match index {
        0 => SpecialRows::TitleRow,
        1 => SpecialRows::StatusRow,
        _ => SpecialRows::PromtRow,
    }
}

fn diff_visible_segment(
    row: SpecialRows,
    side: StripSide,
    old: &[(char, Option<RgbaColor>)],
    new: &[(char, Option<RgbaColor>)],
) -> Option<SegmentUpdate> {
    if old == new {
        return None;
    }

    let old_glyphs = old;
    let new_glyphs = new;
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
        text: new_glyphs[prefix..new_end].iter().map(|cell| cell.0).collect(),
        colors: new_glyphs[prefix..new_end].iter().map(|cell| cell.1).collect(),
    })
}
