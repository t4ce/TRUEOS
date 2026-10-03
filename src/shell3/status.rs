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

fn entries(ids: &[String], active: Option<&str>, aliases: &[String]) -> (Vec<Entry>, Vec<Entry>) {
    let left = matrix_slots_meta(ids, active)
        .into_iter()
        .enumerate()
        .map(|(index, run)| {
            let target = if index == 0 {
                Some(Target::Default)
            } else if (index - 1) % 3 == 0 {
                None
            } else {
                Some(Target::Slot(ids[(index - 1) / 3].clone()))
            };
            (run, target)
        })
        .collect();
    let mut right = Vec::new();
    if !aliases.is_empty() {
        right.push((MetaFmtStr::new("[Aka"), None));
        for alias in aliases {
            right.push((MetaFmtStr::new(" "), None));
            right.push((MetaFmtStr::new(alias), Some(Target::Alias(alias.clone()))));
        }
        right.push((MetaFmtStr::new("]"), None));
    }
    (left, right)
}

pub(super) fn alias_runs(aliases: &[String]) -> Vec<MetaFmtStr> {
    entries(&[], None, aliases)
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
) -> RowStrips {
    let (left, right) = entries(ids, active, aliases);
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
) -> Option<Target> {
    let (left, right) = entries(ids, None, aliases);
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
    pub(super) fn handle_status_pointer(&mut self, column: Option<usize>, pressed: bool) -> bool {
        let target = column.and_then(|column| {
            let slots = matrix_slots().lock();
            hit(&slots.ids, &self.aka_names, self.columns, column)
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
