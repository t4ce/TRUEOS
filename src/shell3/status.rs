//! Status links share the renderer's clipping, so hidden text cannot be clicked.
use super::{MetaFmtStr, RowStrips, Shell3, matrix_slots, matrix_slots_meta, update};
use alloc::{string::String, vec::Vec};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Target {
    Default,
    Slot(String),
    Alias(String),
}

type Entry = (MetaFmtStr, Option<Target>);

const GO2: [char; 9] = ['⢈', '⡈', '⡐', '⡠', '⣀', '⢄', '⢂', '⢁', '⡁'];

fn working_marker() -> char {
    GO2[(crate::chronos::monotonic_nanos() / 100_000_000 % GO2.len() as u64) as usize]
}

fn entries(ids: &[String], active: Option<&str>, aliases: &[String], working: &[String]) -> (Vec<Entry>, Vec<Entry>) {
    let marker = working_marker();
    let left = matrix_slots_meta(ids, active)
        .into_iter()
        .enumerate()
        .flat_map(|(index, run)| {
            let target = if index % 3 == 0 {
                None
            } else {
                Some(Target::Slot(ids[index / 3].clone()))
            };
            let suffix = if index % 3 == 2
                && working.contains(&ids[index / 3]) {
                Some((MetaFmtStr { color: run.color, ..MetaFmtStr::new(alloc::format!("{marker}")) }, None))
            } else { None };
            core::iter::once((run, target)).chain(suffix)
        })
        .collect();
    let mut right = Vec::new();
    if !aliases.is_empty() {
        right.push((MetaFmtStr::new("Aka["), None));
        for (index, alias) in aliases.iter().enumerate() {
            if index != 0 { right.push((MetaFmtStr::new(" "), None)); }
            right.push((MetaFmtStr::new(alias), Some(Target::Alias(alias.clone()))));
        }
        right.push((MetaFmtStr::new("]"), None));
    }
    (left, right)
}

pub(super) fn alias_runs(aliases: &[String]) -> Vec<MetaFmtStr> {
    entries(&[], None, aliases, &[])
        .1
        .into_iter()
        .map(|entry| entry.0)
        .collect()
}

pub(super) fn runs(
    ids: &[String],
    active: Option<&str>,
    aliases: &[String],
    hover: Option<&Target>,
    working: &[String],
) -> RowStrips {
    let (left, right) = entries(ids, active, aliases, working);
    let style = |entries: Vec<Entry>| {
        entries
            .into_iter()
            .map(|(run, target)| {
                if target.is_some() && target.as_ref() == hover {
                    run.underline()
                } else {
                    run
                }
            })
            .collect()
    };
    RowStrips {
        left: style(left),
        right: style(right),
    }
}

pub(super) fn hit(
    ids: &[String],
    aliases: &[String],
    columns: usize,
    column: usize,
    working: &[String],
) -> Option<Target> {
    hit_with_width(ids, aliases, columns, column, working, None)
}

fn hit_with_width(ids: &[String], aliases: &[String], columns: usize, column: usize,
    working: &[String], animated_width: Option<usize>) -> Option<Target> {
    let (left, mut right) = entries(ids, None, aliases, working);
    if let Some(width) = animated_width {
        // Retiring aliases cannot launch stale apps. Keep the same footprint as
        // the animated snapshot so left-hand slot targets still clip correctly.
        right = alloc::vec![(MetaFmtStr::new(" ".repeat(width)), None)];
    }
    let cells = |entries: Vec<Entry>| {
        entries
            .into_iter()
            .flat_map(|(run, target)| run.text.chars().map(|_| target.clone()).collect::<Vec<_>>())
            .collect()
    };
    update::fit_strips(cells(left), cells(right), columns, None, None)
        .get(column)
        .cloned()
        .flatten()
}

impl Shell3 {
    pub(super) fn handle_controls_pointer(&mut self, position: Option<(usize, usize)>, pressed: bool) -> bool {
        if let Some((0, column)) = position {
            let title = self.capture_controls_snapshot();
            if column == 7 && title.rendered_lines()[0].get(column).map(|cell| cell.0) == Some(super::OPERATOR) {
                let changed = self.status_hover != Some(Target::Default);
                self.status_hover = Some(Target::Default);
                if pressed { self.select_matrix_slot_index(0); }
                return changed || pressed;
            }
        }
        self.handle_status_pointer(position.filter(|(row, _)| *row == 1).map(|(_, column)| column), pressed)
    }

    pub(super) fn handle_status_pointer(&mut self, column: Option<usize>, pressed: bool) -> bool {
        let target = column.and_then(|column| {
            let working = super::service::working_vmx_slots();
            let slots = matrix_slots().lock();
            let width = self.aka_transition.frame(crate::chronos::monotonic_nanos())
                .map(|frame| frame.source.iter().map(|run| run.text.chars().count()).sum());
            hit_with_width(&slots.ids, &self.aka_names, self.columns, column, &working, width)
        });
        let changed = target != self.status_hover;
        self.status_hover = target.clone();
        if pressed {
            match target {
                Some(Target::Default) => {
                    self.select_matrix_slot_index(0);
                }
                Some(Target::Slot(name)) => {
                    self.parse_operator(&alloc::format!("§{name}"));
                }
                Some(Target::Alias(name)) => {
                    self.launch_named_app(&name, true);
                }
                None => return changed,
            }
            return true;
        }
        changed
    }
}
