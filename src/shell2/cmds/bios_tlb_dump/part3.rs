fn append_question_records(
    out: &mut String,
    schema: &BiosSchema,
    formset_index: usize,
    formset: &FormSet,
    form: &Form,
    form_key: &str,
    question: &Question,
) {
    let question_key = question_key(form_key, question);
    let numeric = question.numeric.map(|bounds| {
        serde_json::json!({
            "minimum": bounds.minimum,
            "maximum": bounds.maximum,
            "step": bounds.step,
        })
    });
    let string_limits = question.string_limits.map(|limits| {
        serde_json::json!({
            "minimum_chars": limits.minimum_chars,
            "maximum_chars": limits.maximum_chars,
            "multiline": limits.multiline,
        })
    });
    let variable_guid = question
        .storage
        .variable_guid
        .as_ref()
        .map(|guid| guid.fmt_canonical());

    push_record(
        out,
        serde_json::json!({
            "record": "question",
            "key": question_key.as_str(),
            "form_key": form_key,
            "formset_index": formset_index,
            "formset_guid": formset.guid.fmt_canonical(),
            "form_id": form.id,
            "question_id": question.id,
            "question_id_hex": alloc::format!("0x{:04X}", question.id),
            "prompt_id": question.prompt_id,
            "prompt": question.prompt.as_deref(),
            "help_id": question.help_id,
            "help": question.help.as_deref(),
            "kind": question.kind.name(),
            "varstore_id": question.varstore_id,
            "varstore_info": question.varstore_info,
            "width": question.width,
            "question_flags": question.question_flags,
            "kind_flags": question.kind_flags,
            "numeric": numeric,
            "string_limits": string_limits,
            "source_offset": question.source_offset,
            "option_count": question.options.len(),
            "default_count": question.defaults.len(),
            "condition_count": question.conditions.len(),
            "storage": {
                "backend": question.storage.backend.name(),
                "varstore_id": question.storage.varstore_id,
                "variable": question.storage.variable.as_deref(),
                "variable_guid": variable_guid,
                "offset": question.storage.offset,
                "width": question.storage.width,
                "attributes": question.storage.attributes,
                "validated": question.storage.valid,
                "detail": question.storage.detail,
                "config_content": if schema.capture.config_captured {
                    "captured-redacted"
                } else {
                    "not-captured"
                },
            },
            "policy": {
                "requires_reset": question.requires_reset(),
                "callback": question.callback(),
                "firmware_ro": question.read_only(),
                "trueos_write": "locked",
                "current_value": if schema.capture.config_captured && question.storage.valid {
                    "captured-redacted-not-decoded-in-this-cycle"
                } else {
                    "unavailable"
                },
                "active_write_path": "none",
            },
        }),
    );

    for (index, option) in question.options.iter().enumerate() {
        push_record(
            out,
            serde_json::json!({
                "record": "option",
                "question_key": question_key.as_str(),
                "index": index,
                "text_id": option.text_id,
                "text": option.text.as_deref(),
                "flags": option.flags,
                "value": ifr_value_json(&option.value),
                "source_offset": option.source_offset,
            }),
        );
    }

    for (index, default) in question.defaults.iter().enumerate() {
        push_record(
            out,
            serde_json::json!({
                "record": "default",
                "question_key": question_key.as_str(),
                "index": index,
                "default_id": default.default_id,
                "label": default.label.as_str(),
                "value": default.value.as_ref().map(ifr_value_json),
                "source": default.source,
                "source_offset": default.source_offset,
            }),
        );
    }

    for (condition_index, condition) in question.conditions.iter().enumerate() {
        push_record(
            out,
            serde_json::json!({
                "record": "condition",
                "question_key": question_key.as_str(),
                "index": condition_index,
                "kind": condition.kind.name(),
                "source_offset": condition.source_offset,
                "expression_opcodes": condition.expression.len(),
            }),
        );
        for (opcode_index, opcode) in condition.expression.iter().enumerate() {
            append_opaque_record(
                out,
                "condition-opcode",
                Some(question_key.as_str()),
                opcode_index,
                opcode,
            );
        }
    }
}
