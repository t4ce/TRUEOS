fn append_schema_records(out: &mut String, schema: &BiosSchema) {
    push_record(
        out,
        serde_json::json!({
            "record": "schema",
            "state": schema.state(),
            "source": schema.capture.source,
            "package_lists": schema.package_lists,
            "packages": schema.packages,
            "formsets": schema.formsets.len(),
            "forms": schema.form_count(),
            "questions": schema.question_count(),
            "strings_resolved": schema.catalogue_strings_resolved,
            "string_references_resolved": schema.stats.string_references_resolved,
            "string_references_unresolved": schema.stats.string_references_unresolved,
            "varstores": schema.varstores.len(),
            "defaultstores": schema.default_stores.len(),
            "unknown_opcodes": schema.unknown_opcodes.len(),
            "malformed_packages": schema.stats.malformed_packages,
            "current_configuration": if schema.capture.config_captured {
                "captured-redacted"
            } else {
                "not-captured"
            },
            "active_write_path": "none",
        }),
    );

    for (formset_index, formset) in schema.formsets.iter().enumerate() {
        let formset_key = formset_key(formset_index, formset);
        push_record(
            out,
            serde_json::json!({
                "record": "formset",
                "key": formset_key.as_str(),
                "formset_index": formset_index,
                "list": formset.list_index,
                "package": formset.package_index,
                "guid": formset.guid.fmt_canonical(),
                "title_id": formset.title_id,
                "title": formset.title.as_deref(),
                "help_id": formset.help_id,
                "help": formset.help.as_deref(),
                "flags": formset.flags,
                "forms": formset.forms.len(),
            }),
        );

        for form in &formset.forms {
            let form_key = form_key(&formset_key, form);
            push_record(
                out,
                serde_json::json!({
                    "record": "form",
                    "key": form_key.as_str(),
                    "formset_key": formset_key.as_str(),
                    "formset_index": formset_index,
                    "form_id": form.id,
                    "form_id_hex": alloc::format!("0x{:04X}", form.id),
                    "title_id": form.title_id,
                    "title": form.title.as_deref(),
                    "source_offset": form.source_offset,
                    "questions": form.questions.len(),
                }),
            );

            for question in &form.questions {
                append_question_records(
                    out,
                    schema,
                    formset_index,
                    formset,
                    form,
                    &form_key,
                    question,
                );
            }
        }
    }

    for (index, varstore) in schema.varstores.iter().enumerate() {
        push_record(
            out,
            serde_json::json!({
                "record": "varstore",
                "index": index,
                "formset_index": varstore.formset_index,
                "list": varstore.list_index,
                "package": varstore.package_index,
                "varstore_id": varstore.id,
                "varstore_id_hex": alloc::format!("0x{:04X}", varstore.id),
                "backend": varstore.backend.name(),
                "guid": varstore.guid.fmt_canonical(),
                "name": varstore.name.as_deref(),
                "size": varstore.size,
                "attributes": varstore.attributes,
                "source_offset": varstore.source_offset,
            }),
        );
    }

    for (index, default_store) in schema.default_stores.iter().enumerate() {
        push_record(
            out,
            serde_json::json!({
                "record": "defaultstore",
                "index": index,
                "formset_index": default_store.formset_index,
                "list": default_store.list_index,
                "package": default_store.package_index,
                "default_id": default_store.id,
                "default_id_hex": alloc::format!("0x{:04X}", default_store.id),
                "name_id": default_store.name_id,
                "name": default_store.name.as_deref(),
                "source_offset": default_store.source_offset,
            }),
        );
    }

    for (index, opcode) in schema.unknown_opcodes.iter().enumerate() {
        append_opaque_record(out, "unknown-opcode", None, index, opcode);
    }
}
