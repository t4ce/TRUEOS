fn attach_storage_and_defaults(schema: &mut BiosSchema, catalogue: &HiiCatalogue) {
    let varstores = &schema.varstores;
    let default_stores = &schema.default_stores;
    for (formset_index, formset) in schema.formsets.iter_mut().enumerate() {
        for form in &mut formset.forms {
            for question in &mut form.questions {
                question.storage = bind_storage(
                    varstores,
                    catalogue,
                    formset_index,
                    formset.list_index,
                    question,
                );
                for default in &mut question.defaults {
                    default.label = default_store_label(
                        default_stores,
                        formset_index,
                        default.default_id,
                    );
                }
                let mut implicit = Vec::<QuestionDefault>::new();
                if question.kind == QuestionKind::OneOf {
                    for option in &question.options {
                        if option.flags & OPTION_DEFAULT != 0
                            && !has_default(&question.defaults, 0)
                            && !has_default(&implicit, 0)
                        {
                            implicit.push(QuestionDefault {
                                default_id: 0,
                                label: default_store_label(default_stores, formset_index, 0),
                                value: Some(option.value.clone()),
                                source: "one-of-option",
                                source_offset: option.source_offset,
                            });
                        }
                        if option.flags & OPTION_DEFAULT_MFG != 0
                            && !has_default(&question.defaults, 1)
                            && !has_default(&implicit, 1)
                        {
                            implicit.push(QuestionDefault {
                                default_id: 1,
                                label: default_store_label(default_stores, formset_index, 1),
                                value: Some(option.value.clone()),
                                source: "one-of-option",
                                source_offset: option.source_offset,
                            });
                        }
                    }
                } else if question.kind == QuestionKind::Checkbox {
                    if question.kind_flags & CHECKBOX_DEFAULT != 0
                        && !has_default(&question.defaults, 0)
                    {
                        implicit.push(QuestionDefault {
                            default_id: 0,
                            label: default_store_label(default_stores, formset_index, 0),
                            value: Some(boolean_value(true)),
                            source: "checkbox-flag",
                            source_offset: question.source_offset,
                        });
                    }
                    if question.kind_flags & CHECKBOX_DEFAULT_MFG != 0
                        && !has_default(&question.defaults, 1)
                    {
                        implicit.push(QuestionDefault {
                            default_id: 1,
                            label: default_store_label(default_stores, formset_index, 1),
                            value: Some(boolean_value(true)),
                            source: "checkbox-flag",
                            source_offset: question.source_offset,
                        });
                    }
                }
                question.defaults.extend(implicit);
            }
        }
    }
}

fn bind_storage(
    varstores: &[VarStore],
    catalogue: &HiiCatalogue,
    formset_index: usize,
    list_index: usize,
    question: &Question,
) -> StorageBinding {
    if question.kind == QuestionKind::Action {
        return StorageBinding {
            backend: VarStoreBackend::None,
            varstore_id: 0,
            variable: None,
            variable_guid: None,
            offset: None,
            width: None,
            attributes: None,
            valid: true,
            detail: "action-has-no-value-storage",
        };
    }
    let Some(varstore) = varstores
        .iter()
        .find(|varstore| varstore.formset_index == formset_index && varstore.id == question.varstore_id)
    else {
        return unresolved_storage(question.varstore_id, question.width);
    };
    match varstore.backend {
        VarStoreBackend::Buffer | VarStoreBackend::Efi => {
            let offset = question.varstore_info;
            let valid = match (question.width, varstore.size) {
                (Some(width), Some(size)) => offset
                    .checked_add(width)
                    .is_some_and(|end| end <= size),
                _ => false,
            };
            StorageBinding {
                backend: varstore.backend,
                varstore_id: varstore.id,
                variable: varstore.name.clone(),
                variable_guid: Some(varstore.guid),
                offset: Some(offset),
                width: question.width,
                attributes: varstore.attributes,
                valid,
                detail: if valid {
                    "validated"
                } else {
                    "offset-or-width-outside-varstore"
                },
            }
        }
        VarStoreBackend::NameValue => {
            let variable = catalogue.resolve_string_owned(list_index, question.varstore_info);
            let valid = variable.as_ref().is_some_and(|name| !name.is_empty());
            StorageBinding {
                backend: varstore.backend,
                varstore_id: varstore.id,
                variable,
                variable_guid: Some(varstore.guid),
                offset: None,
                width: question.width,
                attributes: None,
                valid,
                detail: if valid {
                    "validated"
                } else {
                    "name-value-key-unresolved"
                },
            }
        }
        _ => unresolved_storage(question.varstore_id, question.width),
    }
}

fn unresolved_storage(varstore_id: u16, width: Option<u16>) -> StorageBinding {
    StorageBinding {
        backend: VarStoreBackend::Missing,
        varstore_id,
        variable: None,
        variable_guid: None,
        offset: None,
        width,
        attributes: None,
        valid: false,
        detail: "varstore-not-resolved",
    }
}

fn active_conditions(stack: &[ScopeFrame]) -> Vec<VisibilityCondition> {
    stack
        .iter()
        .filter_map(|frame| match frame {
            ScopeFrame::Condition(condition) => Some(condition.clone()),
            _ => None,
        })
        .collect()
}

fn current_formset(stack: &[ScopeFrame]) -> Option<usize> {
    stack.iter().rev().find_map(|frame| match frame {
        ScopeFrame::FormSet(index) => Some(*index),
        ScopeFrame::Form { formset_index, .. }
        | ScopeFrame::Question { formset_index, .. } => Some(*formset_index),
        _ => None,
    })
}

fn current_form(stack: &[ScopeFrame]) -> Option<(usize, usize)> {
    stack.iter().rev().find_map(|frame| match frame {
        ScopeFrame::Form {
            formset_index,
            form_index,
        }
        | ScopeFrame::Question {
            formset_index,
            form_index,
            ..
        } => Some((*formset_index, *form_index)),
        _ => None,
    })
}

fn current_question(stack: &[ScopeFrame]) -> Option<(usize, usize, usize)> {
    stack.iter().rev().find_map(|frame| match frame {
        ScopeFrame::Question {
            formset_index,
            form_index,
            question_index,
        } => Some((*formset_index, *form_index, *question_index)),
        _ => None,
    })
}

fn parse_common_question(bytes: &[u8]) -> Result<CommonQuestion, String> {
    Ok(CommonQuestion {
        prompt_id: read_u16(bytes, 2)?,
        help_id: read_u16(bytes, 4)?,
        id: read_u16(bytes, 6)?,
        varstore_id: read_u16(bytes, 8)?,
        varstore_info: read_u16(bytes, 10)?,
        flags: bytes[12],
    })
}

fn numeric_width(flags: u8) -> usize {
    match flags & 0x03 {
        0 => 1,
        1 => 2,
        2 => 4,
        _ => 8,
    }
}

fn parse_value(bytes: &[u8], type_code: u8, start: usize) -> Result<IfrValue, String> {
    let width = value_width(type_code, bytes.len().saturating_sub(start));
    let end = checked_end(start, width, bytes.len(), "IFR typed value")?;
    let raw = bytes[start..end].to_vec();
    let unsigned = match type_code {
        0x00 | 0x04 => raw.first().copied().map(u64::from),
        0x01 | 0x07 => Some(read_unsigned(&raw, 0, 2)?),
        0x02 => Some(read_unsigned(&raw, 0, 4)?),
        0x03 => Some(read_unsigned(&raw, 0, 8)?),
        _ => None,
    };
    Ok(IfrValue {
        type_code,
        boolean: if type_code == 0x04 {
            raw.first().map(|value| *value != 0)
        } else {
            None
        },
        string_id: if type_code == 0x07 {
            Some(read_u16(&raw, 0)?)
        } else {
            None
        },
        raw,
        unsigned,
    })
}

fn value_width(type_code: u8, remaining: usize) -> usize {
    match type_code {
        0x00 | 0x04 => 1,
        0x01 | 0x07 => 2,
        0x02 => 4,
        0x03 => 8,
        0x05 => 3,
        0x06 => 4,
        0x08..=0x0a => 0,
        0x0b | 0x0c => remaining,
        _ => 0,
    }
}

fn boolean_value(value: bool) -> IfrValue {
    IfrValue {
        type_code: 0x04,
        raw: alloc::vec![u8::from(value)],
        unsigned: Some(if value { 1 } else { 0 }),
        boolean: Some(value),
        string_id: None,
    }
}

fn resolve_string(
    catalogue: &HiiCatalogue,
    stats: &mut SchemaStats,
    list_index: usize,
    string_id: u16,
) -> Option<String> {
    if string_id == 0 {
        return None;
    }
    let resolved = catalogue.resolve_string_owned(list_index, string_id);
    if resolved.is_some() {
        stats.string_references_resolved = stats.string_references_resolved.saturating_add(1);
    } else {
        stats.string_references_unresolved = stats.string_references_unresolved.saturating_add(1);
    }
    resolved
}

fn read_ascii_name(bytes: &[u8], start: usize) -> Result<String, String> {
    if start >= bytes.len() {
        return Err(String::from("varstore name is absent"));
    }
    let end = bytes[start..]
        .iter()
        .position(|byte| *byte == 0)
        .map(|relative| start + relative)
        .ok_or_else(|| String::from("varstore name is not NUL terminated"))?;
    let mut name = String::new();
    for byte in &bytes[start..end] {
        name.push(match *byte {
            0x20..=0x7e => *byte as char,
            _ => '\u{fffd}',
        });
    }
    if name.is_empty() {
        return Err(String::from("varstore name is empty"));
    }
    Ok(name)
}

fn default_store_label(
    stores: &[DefaultStore],
    formset_index: usize,
    default_id: u16,
) -> String {
    stores
        .iter()
        .find(|store| {
            store.id == default_id
                && match store.formset_index {
                    None => true,
                    Some(index) => index == formset_index,
                }
        })
        .and_then(|store| store.name.clone())
        .unwrap_or_else(|| default_id_fallback(default_id))
}

fn default_id_fallback(default_id: u16) -> String {
    match default_id {
        0 => String::from("standard"),
        1 => String::from("manufacturing"),
        2 => String::from("safe"),
        _ => alloc::format!("default-0x{:04X}", default_id),
    }
}

fn has_default(defaults: &[QuestionDefault], default_id: u16) -> bool {
    defaults
        .iter()
        .any(|default| default.default_id == default_id)
}

fn is_expression_opcode(opcode: u8) -> bool {
    matches!(
        opcode,
        0x12..=0x18
            | 0x2a..=0x5a
            | 0x5e..=0x60
            | 0x64
    )
}

fn read_guid(bytes: &[u8], offset: usize) -> Result<EfiGuid, String> {
    let end = checked_end(offset, 16, bytes.len(), "GUID")?;
    let raw: [u8; 16] = bytes[offset..end]
        .try_into()
        .map_err(|_| String::from("GUID is truncated"))?;
    Ok(EfiGuid::from_uefi_bytes(raw))
}

fn read_unsigned(bytes: &[u8], offset: usize, width: usize) -> Result<u64, String> {
    if width > 8 {
        return Err(String::from("integer width exceeds u64"));
    }
    let end = checked_end(offset, width, bytes.len(), "integer")?;
    let mut raw = [0u8; 8];
    raw[..width].copy_from_slice(&bytes[offset..end]);
    Ok(u64::from_le_bytes(raw))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let end = checked_end(offset, size_of::<u32>(), bytes.len(), "u32")?;
    let raw = &bytes[offset..end];
    Ok(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, String> {
    let end = checked_end(offset, size_of::<u16>(), bytes.len(), "u16")?;
    let raw = &bytes[offset..end];
    Ok(u16::from_le_bytes([raw[0], raw[1]]))
}

fn checked_end(
    start: usize,
    length: usize,
    bound: usize,
    label: &str,
) -> Result<usize, String> {
    let end = start
        .checked_add(length)
        .ok_or_else(|| alloc::format!("{} range overflow", label))?;
    if end > bound {
        return Err(alloc::format!("{} crosses opcode/package boundary", label));
    }
    Ok(end)
}
