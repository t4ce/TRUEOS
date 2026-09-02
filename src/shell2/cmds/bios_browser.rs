use alloc::string::{String, ToString};
use core::fmt::Write;

use crate::shell2::shell2_cmd::ParseOutcome;

use super::super::{ShellBackend2, print_shell_line};
use super::bios_catalogue::{self, BiosCatalogue};
use super::bios_ifr::{
    self, BiosSchema, Form, FormSet, IfrValue, Question, QuestionDefault, QuestionStorage,
};

const MAX_FIND_RESULTS: usize = 64;

pub(crate) fn try_parse(
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> Option<ParseOutcome> {
    let trimmed = rest.trim();
    let mut split = trimmed.splitn(2, char::is_whitespace);
    let command = split.next()?;
    let tail = split.next().unwrap_or("").trim();

    if command.eq_ignore_ascii_case("packages") {
        if tail.is_empty() {
            emit_catalogue(io, append_packages);
        } else {
            usage(io);
        }
        return Some(ParseOutcome::Handled);
    }
    if command.eq_ignore_ascii_case("languages") {
        if tail.is_empty() {
            emit_catalogue(io, append_languages);
        } else {
            usage(io);
        }
        return Some(ParseOutcome::Handled);
    }
    if command.eq_ignore_ascii_case("strings") {
        if tail.eq_ignore_ascii_case("status") {
            emit_catalogue(io, append_string_status);
        } else {
            usage(io);
        }
        return Some(ParseOutcome::Handled);
    }
    if command.eq_ignore_ascii_case("schema") {
        if tail.is_empty() {
            emit_schema(io, append_schema);
        } else {
            usage(io);
        }
        return Some(ParseOutcome::Handled);
    }
    if command.eq_ignore_ascii_case("forms") {
        if tail.is_empty() {
            emit_schema(io, append_forms);
        } else {
            usage(io);
        }
        return Some(ParseOutcome::Handled);
    }
    if command.eq_ignore_ascii_case("find") {
        if tail.is_empty() {
            usage(io);
        } else {
            emit_schema_with(io, |schema, out| append_find(schema, tail, out));
        }
        return Some(ParseOutcome::Handled);
    }
    if command.eq_ignore_ascii_case("show")
        || command.eq_ignore_ascii_case("options")
        || command.eq_ignore_ascii_case("storage")
    {
        let Some(question_id) = parse_question_id(tail) else {
            usage(io);
            return Some(ParseOutcome::Handled);
        };
        if command.eq_ignore_ascii_case("show") {
            emit_schema_with(io, |schema, out| {
                append_question_lookup(schema, question_id, LookupView::Full, out)
            });
        } else if command.eq_ignore_ascii_case("options") {
            emit_schema_with(io, |schema, out| {
                append_question_lookup(schema, question_id, LookupView::Options, out)
            });
        } else {
            emit_schema_with(io, |schema, out| {
                append_question_lookup(schema, question_id, LookupView::Storage, out)
            });
        }
        return Some(ParseOutcome::Handled);
    }
    if matches!(command, "help" | "-h" | "--help") {
        usage(io);
        return Some(ParseOutcome::Handled);
    }
    None
}

#[derive(Clone, Copy)]
enum LookupView {
    Full,
    Options,
    Storage,
}

fn emit_catalogue(io: &'static dyn ShellBackend2, render: fn(&BiosCatalogue, &mut String)) {
    let text = match bios_catalogue::with_catalogue(|catalogue| {
        let mut out = String::new();
        render(catalogue, &mut out);
        out
    }) {
        Ok(text) => text,
        Err(error) => unavailable(&error),
    };
    emit(io, &text);
}

fn emit_schema(io: &'static dyn ShellBackend2, render: fn(&BiosSchema, &mut String)) {
    emit_schema_with(io, render);
}

fn emit_schema_with(
    io: &'static dyn ShellBackend2,
    render: impl FnOnce(&BiosSchema, &mut String),
) {
    let text = match bios_ifr::with_schema(|schema| {
        let mut out = String::new();
        render(schema, &mut out);
        out
    }) {
        Ok(text) => text,
        Err(error) => unavailable(&error),
    };
    emit(io, &text);
}

fn unavailable(error: &str) -> String {
    let mut out = String::new();
    writeln!(out, "state=unavailable detail=\"{}\"", escaped(error)).unwrap();
    writeln!(out, "active_write_path=none").unwrap();
    out
}

fn append_packages(catalogue: &BiosCatalogue, out: &mut String) {
    writeln!(out, "=== Captured HII Package Catalogue ===").unwrap();
    writeln!(
        out,
        "state=ready source={} cache=resident payload_bytes={} hii_bytes={} package_lists={} packages={} form_packages={} string_packages={} malformed_packages={}",
        catalogue.source,
        catalogue.payload_bytes,
        catalogue.hii_bytes,
        catalogue.package_lists.len(),
        catalogue.package_count,
        catalogue.form_package_count,
        catalogue.string_package_count,
        catalogue.malformed_packages
    )
    .unwrap();
    writeln!(
        out,
        "integrity=status_receipt:{} section_crcs:valid package_boundaries:valid capture_flags=0x{:08X} sections={} unknown_sections={}",
        yes_no(catalogue.status_receipt_valid),
        catalogue.capture_flags,
        catalogue.section_count,
        catalogue.unknown_sections
    )
    .unwrap();
    for list in &catalogue.package_lists {
        let forms = list
            .packages
            .iter()
            .filter(|package| package.package_type == bios_catalogue::HII_PACKAGE_FORMS)
            .count();
        let strings = list
            .packages
            .iter()
            .filter(|package| package.package_type == bios_catalogue::HII_PACKAGE_STRINGS)
            .count();
        writeln!(
            out,
            "list={} guid={} offset=0x{:X} bytes={} packages={} forms={} strings={}",
            list.index,
            list.guid.fmt_canonical(),
            list.offset,
            list.bytes,
            list.packages.len(),
            forms,
            strings
        )
        .unwrap();
        for package in &list.packages {
            writeln!(
                out,
                "  package={} type=0x{:02X}({}) offset=0x{:X} bytes={} decoded={}",
                package.index,
                package.package_type,
                bios_catalogue::package_type_name(package.package_type),
                package.offset,
                package.bytes,
                yes_no(package.decoded)
            )
            .unwrap();
        }
    }
    writeln!(out, "raw_firmware_strings=hidden").unwrap();
    writeln!(out, "active_write_path=none").unwrap();
}

fn append_languages(catalogue: &BiosCatalogue, out: &mut String) {
    writeln!(out, "=== Captured HII Languages ===").unwrap();
    let language_packages: usize = catalogue
        .package_lists
        .iter()
        .map(|list| list.strings.len())
        .sum();
    writeln!(
        out,
        "state=ready language_packages={} decoded_strings={} raw_firmware_strings=hidden",
        language_packages,
        catalogue.string_stats.decoded_strings
    )
    .unwrap();
    for list in &catalogue.package_lists {
        for table in &list.strings {
            let language_name = table
                .language_name
                .as_deref()
                .map(one_line)
                .unwrap_or_else(|| String::from("-"));
            writeln!(
                out,
                "list={} guid={} package={} language=\"{}\" language_name_id=0x{:04X} language_name=\"{}\" strings_decoded={}",
                list.index,
                list.guid.fmt_canonical(),
                table.package_index,
                escaped(&table.language),
                table.language_name_id,
                escaped(&language_name),
                table.strings.len()
            )
            .unwrap();
        }
    }
    writeln!(out, "active_write_path=none").unwrap();
}

fn append_string_status(catalogue: &BiosCatalogue, out: &mut String) {
    let stats = &catalogue.string_stats;
    writeln!(out, "=== Captured HII String Decoder ===").unwrap();
    writeln!(
        out,
        "state=ready cache=resident string_packages={} strings_decoded={} ucs2={} scsu_ascii={} duplicates={} duplicate_unresolved={} skipped_ids={} extensions={} opaque_blocks={} opaque_strings={} truncated_strings={} malformed_packages={}",
        catalogue.string_package_count,
        stats.decoded_strings,
        stats.ucs2_strings,
        stats.scsu_ascii_strings,
        stats.duplicate_strings,
        stats.unresolved_duplicates,
        stats.skipped_ids,
        stats.extension_blocks,
        stats.opaque_blocks,
        stats.opaque_strings,
        stats.truncated_strings,
        catalogue.malformed_packages
    )
    .unwrap();
    writeln!(
        out,
        "visibility=metadata-and-resolved-question-references-only bulk_dump=locked raw_firmware_strings=hidden"
    )
    .unwrap();
    writeln!(out, "active_write_path=none").unwrap();
}

fn append_schema(schema: &BiosSchema, out: &mut String) {
    writeln!(out, "=== Read-only BIOS Schema ===").unwrap();
    writeln!(out, "state=ready").unwrap();
    writeln!(out, "source={}", schema.source).unwrap();
    writeln!(out, "formsets={}", schema.formsets.len()).unwrap();
    writeln!(out, "forms={}", schema.stats.forms).unwrap();
    writeln!(out, "questions={}", schema.stats.questions).unwrap();
    writeln!(out, "strings_resolved={}", schema.stats.resolved_strings).unwrap();
    writeln!(out, "malformed_packages={}", schema.total_malformed_packages()).unwrap();
    writeln!(
        out,
        "form_packages={} parsed_form_packages={} malformed_form_packages={} malformed_opcodes={}",
        schema.stats.form_packages,
        schema.stats.parsed_form_packages,
        schema.stats.malformed_form_packages,
        schema.stats.malformed_opcodes
    )
    .unwrap();
    writeln!(
        out,
        "unknown_opcodes={} unknown_opcode_types={} opaque_metadata={} metadata_truncated={}",
        schema.stats.unknown_opcode_instances,
        schema.unknown_opcodes.len(),
        schema.opaque_opcodes.len(),
        schema.stats.truncated_metadata
    )
    .unwrap();
    for (opcode, count) in &schema.unknown_opcodes {
        writeln!(out, "  unknown_opcode=0x{:02X} count={}", opcode, count).unwrap();
    }
    writeln!(
        out,
        "current_values=redacted-until-valid-question-and-storage-association"
    )
    .unwrap();
    writeln!(out, "active_write_path=none").unwrap();
}

fn append_forms(schema: &BiosSchema, out: &mut String) {
    writeln!(out, "=== BIOS Form Sets and Forms ===").unwrap();
    writeln!(
        out,
        "state=ready formsets={} forms={} questions={}",
        schema.formsets.len(),
        schema.stats.forms,
        schema.stats.questions
    )
    .unwrap();
    for (formset_index, formset) in schema.formsets.iter().enumerate() {
        writeln!(
            out,
            "formset={} guid={} title=\"{}\" forms={} varstores={} defaults={} package_list={} package={}",
            formset_index,
            formset.guid.fmt_canonical(),
            escaped(&text_or_id(formset.title.as_deref(), formset.title_id)),
            formset.forms.len(),
            formset.varstores.len(),
            formset.default_stores.len(),
            formset.package_list_index,
            formset.package_index
        )
        .unwrap();
        for form in &formset.forms {
            writeln!(
                out,
                "  form_id=0x{:04X} title=\"{}\" questions={} offset=0x{:X}",
                form.form_id,
                escaped(&text_or_id(form.title.as_deref(), form.title_id)),
                form.questions.len(),
                form.package_offset
            )
            .unwrap();
        }
    }
    writeln!(out, "active_write_path=none").unwrap();
}

fn append_find(schema: &BiosSchema, query: &str, out: &mut String) {
    let folded = query.to_lowercase();
    let mut matches = 0usize;
    writeln!(out, "query=\"{}\"", escaped(query)).unwrap();
    for (formset_index, formset) in schema.formsets.iter().enumerate() {
        for form in &formset.forms {
            for question in &form.questions {
                if !question_matches(formset, form, question, &folded) {
                    continue;
                }
                matches = matches.saturating_add(1);
                if matches <= MAX_FIND_RESULTS {
                    if matches > 1 {
                        writeln!(out).unwrap();
                    }
                    append_question_record(schema, formset_index, formset, form, question, out);
                }
            }
        }
    }
    if matches == 0 {
        writeln!(out, "question_match=none").unwrap();
    } else {
        writeln!(
            out,
            "question_match=count count={} displayed={} truncated={}",
            matches,
            matches.min(MAX_FIND_RESULTS),
            yes_no(matches > MAX_FIND_RESULTS)
        )
        .unwrap();
    }
    writeln!(out, "active_write_path=none").unwrap();
}

fn append_question_lookup(
    schema: &BiosSchema,
    question_id: u16,
    view: LookupView,
    out: &mut String,
) {
    let count = schema
        .formsets
        .iter()
        .flat_map(|formset| &formset.forms)
        .flat_map(|form| &form.questions)
        .filter(|question| question.question_id == question_id)
        .count();
    if count == 0 {
        writeln!(out, "question_id=0x{:04X}", question_id).unwrap();
        writeln!(out, "question_match=none").unwrap();
        writeln!(out, "active_write_path=none").unwrap();
        return;
    }
    writeln!(
        out,
        "question_match={} count={}",
        if count == 1 { "exact" } else { "ambiguous" },
        count
    )
    .unwrap();
    let mut displayed = 0usize;
    for (formset_index, formset) in schema.formsets.iter().enumerate() {
        for form in &formset.forms {
            for question in &form.questions {
                if question.question_id != question_id {
                    continue;
                }
                if displayed > 0 {
                    writeln!(out).unwrap();
                }
                displayed += 1;
                match view {
                    LookupView::Full => {
                        append_question_record(
                            schema,
                            formset_index,
                            formset,
                            form,
                            question,
                            out,
                        );
                    }
                    LookupView::Options => {
                        append_question_identity(formset, form, question, out);
                        append_options(question, out);
                        append_defaults(schema, formset_index, question, out);
                    }
                    LookupView::Storage => {
                        append_question_identity(formset, form, question, out);
                        append_storage(&question.storage, out);
                        writeln!(out, "Current value").unwrap();
                        writeln!(out, "  state           redacted-not-decoded").unwrap();
                    }
                }
            }
        }
    }
    writeln!(out, "active_write_path=none").unwrap();
}

fn append_question_record(
    schema: &BiosSchema,
    formset_index: usize,
    formset: &FormSet,
    form: &Form,
    question: &Question,
    out: &mut String,
) {
    append_question_identity(formset, form, question, out);
    append_storage(&question.storage, out);
    append_options(question, out);
    append_defaults(schema, formset_index, question, out);
    append_policy(question, out);
    append_conditions(question, out);
    writeln!(out, "Current value").unwrap();
    writeln!(out, "  state           redacted-not-decoded").unwrap();
}

fn append_question_identity(
    formset: &FormSet,
    form: &Form,
    question: &Question,
    out: &mut String,
) {
    writeln!(out, "Question").unwrap();
    writeln!(
        out,
        "  prompt          {}",
        text_or_id(question.prompt.as_deref(), question.prompt_id)
    )
    .unwrap();
    writeln!(
        out,
        "  help            {}",
        text_or_id(question.help.as_deref(), question.help_id)
    )
    .unwrap();
    writeln!(out, "  formset         {}", formset.guid.fmt_canonical()).unwrap();
    writeln!(
        out,
        "  formset_title   {}",
        text_or_id(formset.title.as_deref(), formset.title_id)
    )
    .unwrap();
    writeln!(out, "  form_id         0x{:04X}", form.form_id).unwrap();
    writeln!(
        out,
        "  form_title      {}",
        text_or_id(form.title.as_deref(), form.title_id)
    )
    .unwrap();
    writeln!(out, "  question_id     0x{:04X}", question.question_id).unwrap();
    writeln!(out, "  kind            {}", question.kind.as_str()).unwrap();
    writeln!(out, "  ifr_offset      0x{:X}", question.package_offset).unwrap();
    if let (Some(minimum), Some(maximum), Some(step)) =
        (question.minimum, question.maximum, question.step)
    {
        writeln!(
            out,
            "  numeric_range   min={} max={} step={}",
            minimum, maximum, step
        )
        .unwrap();
    }
    if let (Some(minimum), Some(maximum)) = (question.min_chars, question.max_chars) {
        writeln!(
            out,
            "  string_range    min_chars={} max_chars={}",
            minimum, maximum
        )
        .unwrap();
    }
}

fn append_storage(storage: &QuestionStorage, out: &mut String) {
    writeln!(out).unwrap();
    writeln!(out, "Storage").unwrap();
    writeln!(out, "  backend         {}", storage.backend.as_str()).unwrap();
    writeln!(out, "  varstore_id     0x{:04X}", storage.varstore_id).unwrap();
    writeln!(
        out,
        "  variable        {}",
        storage.variable.as_deref().map(one_line).unwrap_or_else(|| String::from("-"))
    )
    .unwrap();
    writeln!(
        out,
        "  variable_guid   {}",
        storage
            .variable_guid
            .as_ref()
            .map(|guid| guid.fmt_canonical())
            .unwrap_or_else(|| String::from("-"))
    )
    .unwrap();
    writeln!(
        out,
        "  offset          {}",
        storage
            .offset
            .map(|value| alloc::format!("0x{:X}", value))
            .unwrap_or_else(|| String::from("-"))
    )
    .unwrap();
    writeln!(
        out,
        "  width           {}",
        storage
            .width
            .map(|value| value.to_string())
            .unwrap_or_else(|| String::from("-"))
    )
    .unwrap();
    if let Some(attributes) = storage.attributes {
        writeln!(out, "  attributes      0x{:08X}", attributes).unwrap();
    }
    writeln!(out, "  validated       {}", yes_no(storage.valid)).unwrap();
    writeln!(
        out,
        "  validation      {}",
        storage.reason.unwrap_or(if storage.valid { "valid" } else { "unavailable" })
    )
    .unwrap();
}

fn append_options(question: &Question, out: &mut String) {
    writeln!(out).unwrap();
    writeln!(out, "Options").unwrap();
    if question.options.is_empty() {
        writeln!(out, "  count           0").unwrap();
        return;
    }
    for option in &question.options {
        writeln!(
            out,
            "  {:<15} {}",
            value_text(&option.value),
            text_or_id(option.label.as_deref(), option.label_id)
        )
        .unwrap();
    }
}

fn append_defaults(
    schema: &BiosSchema,
    formset_index: usize,
    question: &Question,
    out: &mut String,
) {
    writeln!(out).unwrap();
    writeln!(out, "Defaults").unwrap();
    if question.defaults.is_empty() {
        writeln!(out, "  count           0").unwrap();
        return;
    }
    for (index, default) in question.defaults.iter().enumerate() {
        writeln!(out, "  default={}", index).unwrap();
        writeln!(out, "    store_id      0x{:04X}", default.store_id).unwrap();
        writeln!(
            out,
            "    store_name    {}",
            default_store_name(schema, formset_index, default.store_id)
        )
        .unwrap();
        writeln!(
            out,
            "    value         {}",
            default
                .value
                .as_ref()
                .map(value_text)
                .unwrap_or_else(|| String::from("expression-or-opaque"))
        )
        .unwrap();
        writeln!(
            out,
            "    label         {}",
            default_label(question, default).unwrap_or_else(|| String::from("-"))
        )
        .unwrap();
        writeln!(out, "    source        {}", default.source.as_str()).unwrap();
        writeln!(out, "    expression    {}", yes_no(default.expression)).unwrap();
    }
}

fn append_policy(question: &Question, out: &mut String) {
    writeln!(out).unwrap();
    writeln!(out, "Policy").unwrap();
    writeln!(
        out,
        "  requires_reset  {}",
        yes_no(question.policy.reset_required)
    )
    .unwrap();
    writeln!(out, "  callback        {}", yes_no(question.policy.callback)).unwrap();
    writeln!(
        out,
        "  firmware_ro     {}",
        yes_no(question.policy.read_only)
    )
    .unwrap();
    writeln!(
        out,
        "  reconnect       {}",
        yes_no(question.policy.reconnect_required)
    )
    .unwrap();
    writeln!(out, "  trueos_write    locked").unwrap();
}

fn append_conditions(question: &Question, out: &mut String) {
    writeln!(out).unwrap();
    writeln!(out, "Visibility").unwrap();
    if question.conditions.is_empty() {
        writeln!(out, "  conditions      0").unwrap();
    } else {
        for condition in &question.conditions {
            writeln!(
                out,
                "  condition       {} offset=0x{:X} expression_ops={} expression_truncated={} evaluated=no",
                condition.kind.as_str(),
                condition.package_offset,
                condition.expression.len(),
                yes_no(condition.expression_truncated)
            )
            .unwrap();
        }
    }
    writeln!(
        out,
        "  opaque_metadata {}",
        question.opaque.len()
    )
    .unwrap();
}

fn question_matches(formset: &FormSet, form: &Form, question: &Question, query: &str) -> bool {
    contains_folded(formset.title.as_deref(), query)
        || contains_folded(formset.help.as_deref(), query)
        || contains_folded(form.title.as_deref(), query)
        || contains_folded(question.prompt.as_deref(), query)
        || contains_folded(question.help.as_deref(), query)
        || contains_folded(question.storage.variable.as_deref(), query)
        || question
            .options
            .iter()
            .any(|option| contains_folded(option.label.as_deref(), query))
}

fn contains_folded(value: Option<&str>, query: &str) -> bool {
    value.is_some_and(|value| value.to_lowercase().contains(query))
}

fn default_store_name(schema: &BiosSchema, formset_index: usize, id: u16) -> String {
    if let Some(name) = schema.default_store_name(formset_index, id) {
        return one_line(name);
    }
    match id {
        0x0000 => String::from("standard"),
        0x0001 => String::from("manufacturing"),
        0x0002 => String::from("safe"),
        _ => alloc::format!("default-store-0x{:04X}", id),
    }
}

fn default_label(question: &Question, default: &QuestionDefault) -> Option<String> {
    let value = default.value.as_ref()?;
    question.options.iter().find_map(|option| {
        if same_value(&option.value, value) {
            Some(text_or_id(option.label.as_deref(), option.label_id))
        } else {
            None
        }
    })
}

fn same_value(left: &IfrValue, right: &IfrValue) -> bool {
    left.type_code == right.type_code
        && left.unsigned == right.unsigned
        && left.string_id == right.string_id
}

fn value_text(value: &IfrValue) -> String {
    if let Some(string_id) = value.string_id {
        return alloc::format!("string-id:0x{:04X}", string_id);
    }
    if let Some(unsigned) = value.unsigned {
        return unsigned.to_string();
    }
    alloc::format!(
        "opaque-type:0x{:02X}/{}B",
        value.type_code,
        value.encoded_width
    )
}

fn parse_question_id(text: &str) -> Option<u16> {
    if text.is_empty() || text.split_whitespace().count() != 1 {
        return None;
    }
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        u16::from_str_radix(hex, 16).ok()
    } else {
        text.parse().ok()
    }
}

fn text_or_id(text: Option<&str>, id: u16) -> String {
    text.map(one_line)
        .unwrap_or_else(|| alloc::format!("<unresolved:0x{:04X}>", id))
}

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(
        io,
        "bios: read-only catalogue `bios packages` | `bios languages` | `bios strings status`",
    );
    print_shell_line(
        io,
        "bios: read-only schema `bios schema` | `bios forms` | `bios find <text>` | `bios show <question-id>` | `bios options <question-id>` | `bios storage <question-id>`",
    );
    print_shell_line(
        io,
        "bios: legacy `bios [all|status|services|setup|handoff|hints]` | `bios capture [status|sections]`",
    );
}

fn emit(io: &'static dyn ShellBackend2, text: &str) {
    for line in text.lines() {
        print_shell_line(io, line.trim_end_matches('\r'));
    }
}

fn one_line(text: &str) -> String {
    let mut out = String::new();
    let mut truncated = false;
    for (index, ch) in text.chars().enumerate() {
        if index >= 240 {
            truncated = true;
            break;
        }
        if ch.is_control() {
            out.push(' ');
        } else {
            out.push(ch);
        }
    }
    if truncated {
        out.push_str("...");
    }
    out
}

fn escaped(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' | '\r' | '\t' => out.push(' '),
            ch if ch.is_control() => out.push(' '),
            ch => out.push(ch),
        }
    }
    out
}

const fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}
