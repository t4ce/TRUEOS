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

/// Bounded, styled source rectangle for the renderer's optional pan cache.
/// The history stays with MatrixSlots; this carries visible rows plus guards.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MatrixAreaSnapshot {
    pub offset: usize,
    pub first: usize,
    pub columns: usize,
    pub rows: usize,
    pub revision: u64,
    pub blink_phase: Option<bool>,
    pub identity: (Option<String>, Option<u64>),
    pub cells: Vec<RenderedLine>,
}

impl MatrixAreaSnapshot {
    pub fn cell(&self, x: i64, y: i64) -> (char, Option<RgbaColor>) {
        usize::try_from(y).ok().and_then(|y| y.checked_sub(self.first))
            .and_then(|y| self.cells.get(y))
            .and_then(|row| usize::try_from(x).ok().and_then(|x| row.get(x)))
            .copied().unwrap_or((' ', None))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Snapshot {
    size: (usize, usize),
    layout_generation: usize,
    rows: Vec<VisibleRow>,
    matrix_generation: u64,
    tui_revision: u64,
    terminal_active: bool,
    blink_phase: Option<bool>,
    matrix_area: Option<MatrixAreaSnapshot>,
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
            matrix_generation: 0,
            tui_revision: 0,
            terminal_active: false,
            blink_phase: None,
            matrix_area: None,
            rows: strips
                .map(|(left, right)| VisibleRow {
                    rendered: fit_meta_strips(left, right, columns),
                })
                .into(),
        }
    }

    pub(super) fn with_matrix(self, lines: &[String], generation: u64) -> Self {
        self.with_matrix_offset(lines, generation, 0)
    }

    pub(super) fn with_matrix_offset(self, lines: &[String], generation: u64, offset: usize) -> Self {
        self.with_matrix_guard(lines, generation, offset, 0)
    }

    pub(super) fn with_matrix_guard(mut self, lines: &[String], generation: u64, offset: usize, guard: usize) -> Self {
        self.matrix_generation = generation;
        let count = self.size.1.saturating_sub(3);
        let first = offset.min(lines.len().saturating_sub(count));
        let start = first.saturating_sub(guard);
        let end = first.saturating_add(count).saturating_add(guard).min(lines.len());
        let cells: Vec<_> = lines[start.min(end)..end].iter().map(|text| {
            fit_meta_strips(&[MetaFmtStr::new(text)], &[], self.size.0.saturating_add(guard))
        }).collect();
        for index in 0..count {
            let rendered = cells.get(first + index - start)
                .map(|row| row[..self.size.0].to_vec())
                .unwrap_or_else(|| fit_meta_strips(&[], &[], self.size.0));
            self.rows.push(VisibleRow {
                rendered,
            });
        }
        self.matrix_area = Some(MatrixAreaSnapshot { offset: first, first: start,
            columns: self.size.0, rows: count, revision: generation,
            blink_phase: None,
            identity: (None, None), cells });
        self
    }

    pub(super) fn with_matrix_identity(mut self, name: Option<String>, lifetime: Option<u64>) -> Self {
        if let Some(area) = &mut self.matrix_area { area.identity = (name, lifetime); }
        self
    }

    pub(super) fn matrix_area(&self) -> Option<&MatrixAreaSnapshot> { self.matrix_area.as_ref() }

    pub(super) fn terminal(size: (usize, usize), layout_generation: usize, lines: Vec<RenderedLine>, revision: u64) -> Self {
        Self {size, layout_generation, rows: lines.into_iter().map(|rendered| VisibleRow {rendered}).collect(), matrix_generation: 0, tui_revision: revision, terminal_active: true, blink_phase: None, matrix_area: None}
    }
    pub(super) fn with_tui_revision(mut self, revision: u64) -> Self { self.tui_revision = revision; self }
    pub(super) fn tui_revision(&self) -> u64 { self.tui_revision }
    pub(super) fn terminal_active(&self) -> bool { self.terminal_active }

    pub(super) fn blink_phase(&self) -> Option<bool> { self.blink_phase }

    pub(super) fn with_blink_phase(mut self, visible: bool) -> Self {
        for row in &mut self.rows {
            for (_, style) in &mut row.rendered {
                if let Some(color) = *style && color.blink() {
                    self.blink_phase = Some(visible);
                    *style = Some(super::RgbaColor::Terminal {
                        foreground: if visible { color.rgba() } else { super::RgbaColor::BlackTransparent.rgba() },
                        background: if visible { color.background().unwrap_or(super::RgbaColor::BlackTransparent.rgba()) } else { super::RgbaColor::BlackTransparent.rgba() },
                        underline: visible && color.underline(),
                    });
                }
            }
        }
        if let Some(area) = &mut self.matrix_area {
            // Mirror the visible blink styling into the guarded source cells.
            for row in &mut area.cells {
                for (_, style) in row {
                    if let Some(color) = *style && color.blink() {
                        self.blink_phase = Some(visible);
                        area.blink_phase = Some(visible);
                        *style = Some(super::RgbaColor::Terminal {
                            foreground: if visible { color.rgba() } else { super::RgbaColor::BlackTransparent.rgba() },
                            background: if visible { color.background().unwrap_or(super::RgbaColor::BlackTransparent.rgba()) } else { super::RgbaColor::BlackTransparent.rgba() },
                            underline: visible && color.underline(),
                        });
                    }
                }
            }
        }
        self
    }

    pub(super) fn matrix_generation(&self) -> u64 {
        self.matrix_generation
    }

    pub(super) fn status_matches(&self, left: &[MetaFmtStr], right: &[MetaFmtStr]) -> bool {
        self.rows.get(1).is_some_and(|row| {
            row.rendered == fit_meta_strips(left, right, self.size.0)
        })
    }

    pub(super) fn size(&self) -> (usize, usize) {
        self.size
    }

    pub(super) fn rendered_lines(&self) -> Vec<RenderedLine> {
        self.rows.iter().map(|row| row.rendered.clone()).collect()
    }

    pub(super) fn rendered_ui4_lines(&self) -> Vec<RenderedLine> {
        let mut lines = self.rendered_lines();
        if !self.terminal_active {
            // Black is already the default; extra opacity makes the controls
            // slightly darker over the desktop, including their padded blanks.
            let background = [0, 0, 0, 160];
            for row in lines.iter_mut().take(3) {
                for (_, style) in row {
                    let color = style.unwrap_or(RgbaColor::White);
                    if color.background().is_none_or(|bg| bg == RgbaColor::BlackTransparent.rgba()) {
                        *style = Some(RgbaColor::Terminal {
                            foreground: color.rgba(),
                            background,
                            underline: color.underline(),
                        });
                    }
                }
            }
        }
        lines
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
    for index in 0..baseline.rows.len().max(current.rows.len()) {
        let row = row_from_index(index);
        if let Some(update) = diff_visible_segment(
            row,
            StripSide::Left,
            baseline
                .rows
                .get(index)
                .map(|row| row.rendered.as_slice())
                .unwrap_or(&[]),
            current
                .rows
                .get(index)
                .map(|row| row.rendered.as_slice())
                .unwrap_or(&[]),
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
    previous: Option<&[RenderedLine]>,
    current: &[RenderedLine],
) -> Vec<SegmentUpdate> {
    let mut updates = Vec::new();
    let count = current.len().max(previous.map_or(0, |lines| lines.len()));
    for index in 0..count {
        let row = row_from_index(index);
        let line = current.get(index).map(Vec::as_slice).unwrap_or(&[]);
        if previous.is_none() {
            // A fresh frame (or a cleared retry) already has its background.
            // Emit only occupied spans, never a blit per padded blank cell.
            let mut start = 0;
            while start < line.len() {
                if line[start].0 == ' ' && line[start].1.and_then(RgbaColor::background).is_none() {
                    start += 1;
                    continue;
                }
                let mut end = start + 1;
                while end < line.len() && (line[end].0 != ' ' || line[end].1.and_then(RgbaColor::background).is_some()) {
                    end += 1;
                }
                if let Some(mut update) =
                    diff_visible_segment(row, StripSide::Left, &[], &line[start..end])
                {
                    update.offset = start;
                    updates.push(update);
                }
                start = end;
            }
        } else {
            // Existing pixels must still be erased when text/rows disappear.
            let old = previous
                .and_then(|lines| lines.get(index))
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            if let Some(update) = diff_visible_segment(row, StripSide::Left, old, line) {
                updates.push(update);
            }
        }
    }
    updates
}

pub(super) fn fit_meta_strips(
    left: &[MetaFmtStr],
    right: &[MetaFmtStr],
    columns: usize,
) -> RenderedLine {
    let cells = |runs: &[MetaFmtStr]| -> RenderedLine {
        runs.iter()
            .flat_map(|run| {
                run.text.chars().map(|ch| {
                    (
                        ch,
                        if run.blink {
                            let color = run.color.unwrap_or(RgbaColor::White);
                            Some(RgbaColor::Blinking {foreground: color.rgba(), background: color.background(), underline: run.underline || color.underline()})
                        } else if run.underline {
                            Some(RgbaColor::Underlined {foreground: run.color.unwrap_or(RgbaColor::White).rgba()})
                        } else { run.color }
                    )
                })
            })
            .collect()
    };
    fit_strips(cells(left), cells(right), columns, (' ', None), (super::SpecialSeperator, None))
}

/// Rendering and pointer targets must use exactly the same clipping and alignment.
pub(super) fn fit_strips<T: Clone>(
    mut left: Vec<T>,
    mut right: Vec<T>,
    columns: usize,
    blank: T,
    separator: T,
) -> Vec<T> {
    if left.len() + right.len() <= columns {
        left.resize(columns - right.len(), blank);
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
        left.push(separator);
    }
    left.extend(right);
    left
}

fn row_from_index(index: usize) -> SpecialRows {
    match index {
        0 => SpecialRows::TitleRow,
        1 => SpecialRows::StatusRow,
        2 => SpecialRows::PromtRow,
        _ => SpecialRows::MatrixRow(index - 3),
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
        text: new_glyphs[prefix..new_end]
            .iter()
            .map(|cell| cell.0)
            .collect(),
        colors: new_glyphs[prefix..new_end]
            .iter()
            .map(|cell| cell.1)
            .collect(),
    })
}
