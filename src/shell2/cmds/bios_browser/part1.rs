use alloc::string::String;
use core::fmt::Write;

use crate::shell2::shell2_cmd::ParseOutcome;

use super::super::{ShellBackend2, print_shell_line};
use super::bios_hii::single_line;
use super::bios_ifr::{
    BiosSchema, Form, FormSet, IfrValue, Question, QuestionDefault, QuestionOption,
};

const MAX_FORM_ROWS: usize = 2048;
const MAX_FIND_RESULTS: usize = 64;
const MAX_ID_RESULTS: usize = 64;

pub(crate) fn try_parse(
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> Option<ParseOutcome> {
    let trimmed = rest.trim();
    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let command = parts.next()?;
    let tail = parts.next().unwrap_or("").trim();
    match command {
        "schema" => {
            if !tail.is_empty() {
                usage(io, "bios schema");
            } else {
                emit_schema(io, append_schema_status);
            }
        }
        "forms" => {
            if !tail.is_empty() {
                usage(io, "bios forms");
            } else {
                emit_schema(io, append_forms);
            }
        }
        "find" => {
            if tail.is_empty() {
                usage(io, "bios find <text>");
            } else {
                let query = String::from(tail);
                emit_schema(io, |out, schema| append_find(out, schema, &query));
            }
        }
        "show" | "options" | "storage" => {
            let Some(question_id) = parse_single_question_id(tail) else {
                usage(io, alloc::format!("bios {} <question-id>", command).as_str());
                return Some(ParseOutcome::Handled);
            };
            match command {
                "show" => emit_schema(io, |out, schema| {
                    append_by_id(out, schema, question_id, DetailView::Full)
                }),
                "options" => emit_schema(io, |out, schema| {
                    append_by_id(out, schema, question_id, DetailView::Options)
                }),
                "storage" => emit_schema(io, |out, schema| {
                    append_by_id(out, schema, question_id, DetailView::Storage)
                }),
                _ => unreachable!(),
            }
        }
        _ => return None,
    }
    Some(ParseOutcome::Handled)
}

#[derive(Clone, Copy)]
enum DetailView {
    Full,
    Options,
    Storage,
}

fn usage(io: &'static dyn ShellBackend2, syntax: &str) {
    print_shell_line(io, alloc::format!("bios: usage `{}`", syntax).as_str());
}

fn emit_schema(
    io: &'static dyn ShellBackend2,
    append: impl FnOnce(&mut String, &BiosSchema),
) {
    let mut out = String::new();
    match super::bios_ifr::with_schema(|schema| append(&mut out, schema)) {
        Ok(()) => {}
        Err(error) => {
            writeln!(out, "state=unavailable").unwrap();
            writeln!(out, "detail=\"{}\"", single_line(&error, 240)).unwrap();
            writeln!(out, "active_write_path=none").unwrap();
        }
    }
    for line in out.lines() {
        print_shell_line(io, line.trim_end_matches('\r'));
    }
}

fn append_schema_status(out: &mut String, schema: &BiosSchema) {
    writeln!(out, "bios schema").unwrap();
    writeln!(out, "  state={}", schema.state()).unwrap();
    writeln!(out, "  source={}", schema.capture.source).unwrap();
    writeln!(out, "  package_lists={}", schema.package_lists).unwrap();
    writeln!(out, "  packages={}", schema.packages).unwrap();
    writeln!(out, "  formsets={}", schema.formsets.len()).unwrap();
    writeln!(out, "  forms={}", schema.form_count()).unwrap();
    writeln!(out, "  questions={}", schema.question_count()).unwrap();
    writeln!(
        out,
        "  strings_resolved={}",
        schema.catalogue_strings_resolved
    )
    .unwrap();
    writeln!(
        out,
        "  string_references_resolved={}",
        schema.stats.string_references_resolved
    )
    .unwrap();
    writeln!(
        out,
        "  string_references_unresolved={}",
        schema.stats.string_references_unresolved
    )
    .unwrap();
    writeln!(out, "  varstores={}", schema.varstores.len()).unwrap();
    writeln!(out, "  defaultstores={}", schema.default_stores.len()).unwrap();
    writeln!(out, "  unknown_opcodes={}", schema.unknown_opcodes.len()).unwrap();
    writeln!(
        out,
        "  malformed_packages={}",
        schema.stats.malformed_packages
    )
    .unwrap();
    writeln!(out, "  opaque_metadata=retained").unwrap();
    writeln!(
        out,
        "  current_configuration={}",
        if schema.capture.config_captured {
            "captured-redacted"
        } else {
            "not-captured"
        }
    )
    .unwrap();
    writeln!(out, "  active_write_path=none").unwrap();
}

fn append_forms(out: &mut String, schema: &BiosSchema) {
    writeln!(out, "bios forms").unwrap();
    writeln!(
        out,
        "  state={} formsets={} forms={} questions={}",
        schema.state(),
        schema.formsets.len(),
        schema.form_count(),
        schema.question_count()
    )
    .unwrap();
    let mut rows = 0usize;
    for (formset_index, formset) in schema.formsets.iter().enumerate() {
        if rows >= MAX_FORM_ROWS {
            break;
        }
        writeln!(
            out,
            "formset={} guid={} title=\"{}\" forms={}",
            formset_index,
            formset.guid.fmt_canonical(),
            optional_text(&formset.title, formset.title_id),
            formset.forms.len()
        )
        .unwrap();
        rows += 1;
        for form in &formset.forms {
            if rows >= MAX_FORM_ROWS {
                break;
            }
            writeln!(
                out,
                "  form_id=0x{:04X} title=\"{}\" questions={}",
                form.id,
                optional_text(&form.title, form.title_id),
                form.questions.len()
            )
            .unwrap();
            rows += 1;
        }
    }
    if rows >= MAX_FORM_ROWS
        && schema
            .formsets
            .iter()
            .map(|formset| formset.forms.len() + 1)
            .sum::<usize>()
            > rows
    {
        writeln!(out, "output_truncated=yes row_limit={}", MAX_FORM_ROWS).unwrap();
    }
    writeln!(out, "active_write_path=none").unwrap();
}

fn append_find(out: &mut String, schema: &BiosSchema, query: &str) {
    let folded_query = query.to_ascii_lowercase();
    writeln!(out, "bios find \"{}\"", single_line(query, 160)).unwrap();
    let mut matches = 0usize;
    let mut total = 0usize;
    for (formset_index, formset) in schema.formsets.iter().enumerate() {
        for form in &formset.forms {
            for question in &form.questions {
                if !question_matches(formset, form, question, &folded_query) {
                    continue;
                }
                total = total.saturating_add(1);
                if matches >= MAX_FIND_RESULTS {
                    continue;
                }
                matches += 1;
                writeln!(out, "question_match={}", matches).unwrap();
                append_question_summary(out, formset_index, formset, form, question);
            }
        }
    }
    if total == 0 {
        writeln!(out, "question_match=none").unwrap();
    } else {
        writeln!(out, "question_matches={}", total).unwrap();
        if total > matches {
            writeln!(
                out,
                "output_truncated=yes result_limit={}",
                MAX_FIND_RESULTS
            )
            .unwrap();
        }
    }
    writeln!(out, "match_scope=validated-question-records-only").unwrap();
    writeln!(out, "active_write_path=none").unwrap();
}

