fn append_by_id(out: &mut String, schema: &BiosSchema, question_id: u16, view: DetailView) {
    let command = match view {
        DetailView::Full => "show",
        DetailView::Options => "options",
        DetailView::Storage => "storage",
    };
    writeln!(out, "bios {} 0x{:04X}", command, question_id).unwrap();
    let mut shown = 0usize;
    let mut total = 0usize;
    for (formset_index, formset) in schema.formsets.iter().enumerate() {
        for form in &formset.forms {
            for question in &form.questions {
                if question.id != question_id {
                    continue;
                }
                total = total.saturating_add(1);
                if shown >= MAX_ID_RESULTS {
                    continue;
                }
                shown += 1;
                if total > 1 {
                    writeln!(out, "record={}", total).unwrap();
                }
                match view {
                    DetailView::Full => append_question_detail(
                        out,
                        schema,
                        formset_index,
                        formset,
                        form,
                        question,
                    ),
                    DetailView::Options => append_options(out, question),
                    DetailView::Storage => append_storage(out, schema, question),
                }
            }
        }
    }
    if total == 0 {
        writeln!(out, "question_match=none").unwrap();
    } else {
        writeln!(out, "question_records={}", total).unwrap();
        if total > shown {
            writeln!(out, "output_truncated=yes result_limit={}", MAX_ID_RESULTS).unwrap();
        }
    }
    writeln!(out, "active_write_path=none").unwrap();
}

fn append_question_summary(
    out: &mut String,
    formset_index: usize,
    formset: &FormSet,
    form: &Form,
    question: &Question,
) {
    writeln!(
        out,
        "  prompt=\"{}\"",
        optional_text(&question.prompt, question.prompt_id)
    )
    .unwrap();
    writeln!(
        out,
        "  formset_index={} formset={} form_id=0x{:04X} question_id=0x{:04X} kind={}",
        formset_index,
        formset.guid.fmt_canonical(),
        form.id,
        question.id,
        question.kind.name()
    )
    .unwrap();
    writeln!(
        out,
        "  storage_backend={} storage_valid={} variable=\"{}\" offset={} width={}",
        question.storage.backend.name(),
        yes_no(question.storage.valid),
        question
            .storage
            .variable
            .as_deref()
            .map(|text| single_line(text, 120))
            .unwrap_or_else(|| String::from("-")),
        format_optional_hex(question.storage.offset),
        format_optional_decimal(question.storage.width)
    )
    .unwrap();
}

fn append_question_detail(
    out: &mut String,
    schema: &BiosSchema,
    formset_index: usize,
    formset: &FormSet,
    form: &Form,
    question: &Question,
) {
    writeln!(out, "Question").unwrap();
    writeln!(
        out,
        "  prompt          {}",
        optional_text(&question.prompt, question.prompt_id)
    )
    .unwrap();
    writeln!(
        out,
        "  help            {}",
        optional_text(&question.help, question.help_id)
    )
    .unwrap();
    writeln!(out, "  formset         {}", formset.guid.fmt_canonical()).unwrap();
    writeln!(out, "  formset_index   {}", formset_index).unwrap();
    writeln!(
        out,
        "  form_title      {}",
        optional_text(&form.title, form.title_id)
    )
    .unwrap();
    writeln!(out, "  form_id         0x{:04X}", form.id).unwrap();
    writeln!(out, "  question_id     0x{:04X}", question.id).unwrap();
    writeln!(out, "  kind            {}", question.kind.name()).unwrap();
    writeln!(out, "  source_offset   0x{:X}", question.source_offset).unwrap();
    if let Some(numeric) = question.numeric {
        writeln!(
            out,
            "  numeric_range   min={} max={} step={}",
            numeric.minimum,
            numeric.maximum,
            numeric.step
        )
        .unwrap();
    }
    if let Some(limits) = question.string_limits {
        writeln!(
            out,
            "  string_limits   min={} max={} multiline={}",
            limits.minimum_chars,
            limits.maximum_chars,
            yes_no(limits.multiline)
        )
        .unwrap();
    }
    append_storage(out, schema, question);
    append_options(out, question);
    writeln!(out, "Visibility").unwrap();
    writeln!(out, "  enclosing_conditions={}", question.conditions.len()).unwrap();
    for condition in &question.conditions {
        writeln!(
            out,
            "  kind={} source_offset=0x{:X} opaque_expression_opcodes={}",
            condition.kind.name(),
            condition.source_offset,
            condition.expression.len()
        )
        .unwrap();
    }
    writeln!(out, "Policy").unwrap();
    writeln!(
        out,
        "  requires_reset  {}",
        yes_no(question.requires_reset())
    )
    .unwrap();
    writeln!(out, "  callback        {}", yes_no(question.callback())).unwrap();
    writeln!(out, "  firmware_ro     {}", yes_no(question.read_only())).unwrap();
    writeln!(out, "  trueos_write    locked").unwrap();
    writeln!(
        out,
        "  current_value   {}",
        if schema.capture.config_captured && question.storage.valid {
            "captured-redacted-not-decoded-in-this-cycle"
        } else {
            "unavailable"
        }
    )
    .unwrap();
}

