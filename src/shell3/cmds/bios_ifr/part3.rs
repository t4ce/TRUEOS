fn parse_formset(
    schema: &mut BiosSchema,
    catalogue: &HiiCatalogue,
    package: &FormPackageRecord,
    stack: &mut Vec<ScopeFrame>,
    bytes: &[u8],
    source_offset: u32,
    scope: bool,
) -> Result<(), String> {
    if bytes.len() < 23 {
        return Err(String::from("FORM_SET opcode is truncated"));
    }
    if schema.formsets.len() >= MAX_FORMSETS {
        return Err(String::from("formset count exceeds bound"));
    }
    let guid = read_guid(bytes, 2)?;
    let title_id = read_u16(bytes, 18)?;
    let help_id = read_u16(bytes, 20)?;
    let title = resolve_string(catalogue, &mut schema.stats, package.list_index, title_id);
    let help = resolve_string(catalogue, &mut schema.stats, package.list_index, help_id);
    let formset_index = schema.formsets.len();
    schema.formsets.push(FormSet {
        list_index: package.list_index,
        package_index: package.package_index,
        guid,
        title_id,
        title,
        help_id,
        help,
        flags: bytes[22],
        forms: Vec::new(),
    });
    if scope {
        stack.push(ScopeFrame::FormSet(formset_index));
    } else {
        preserve_recognized_unscoped(
            schema,
            package,
            IFR_FORM_SET,
            source_offset,
            bytes,
            stack,
        )?;
    }
    Ok(())
}

fn parse_form(
    schema: &mut BiosSchema,
    catalogue: &HiiCatalogue,
    package: &FormPackageRecord,
    stack: &mut Vec<ScopeFrame>,
    bytes: &[u8],
    source_offset: u32,
    scope: bool,
) -> Result<(), String> {
    if bytes.len() < 6 {
        return Err(String::from("FORM opcode is truncated"));
    }
    if schema.stats.forms >= MAX_FORMS {
        return Err(String::from("form count exceeds bound"));
    }
    let formset_index = current_formset(stack)
        .ok_or_else(|| String::from("FORM opcode is outside FORM_SET scope"))?;
    let id = read_u16(bytes, 2)?;
    let title_id = read_u16(bytes, 4)?;
    let title = resolve_string(catalogue, &mut schema.stats, package.list_index, title_id);
    let form_index = schema.formsets[formset_index].forms.len();
    schema.formsets[formset_index].forms.push(Form {
        id,
        title_id,
        title,
        source_offset,
        questions: Vec::new(),
    });
    schema.stats.forms = schema.stats.forms.saturating_add(1);
    if scope {
        stack.push(ScopeFrame::Form {
            formset_index,
            form_index,
        });
    }
    Ok(())
}

fn parse_varstore(
    schema: &mut BiosSchema,
    package: &FormPackageRecord,
    stack: &[ScopeFrame],
    bytes: &[u8],
    source_offset: u32,
    backend: VarStoreBackend,
) -> Result<(), String> {
    if schema.varstores.len() >= MAX_VARSTORES {
        return Err(String::from("varstore count exceeds bound"));
    }
    let formset_index = current_formset(stack)
        .ok_or_else(|| String::from("VARSTORE opcode is outside FORM_SET scope"))?;
    let (id, guid, name, size, attributes) = match backend {
        VarStoreBackend::Buffer => {
            if bytes.len() < 23 {
                return Err(String::from("VARSTORE opcode is truncated"));
            }
            (
                read_u16(bytes, 18)?,
                read_guid(bytes, 2)?,
                Some(read_ascii_name(bytes, 22)?),
                Some(read_u16(bytes, 20)?),
                None,
            )
        }
        VarStoreBackend::Efi => {
            if bytes.len() < 27 {
                return Err(String::from("VARSTORE_EFI opcode is truncated"));
            }
            (
                read_u16(bytes, 2)?,
                read_guid(bytes, 4)?,
                Some(read_ascii_name(bytes, 26)?),
                Some(read_u16(bytes, 24)?),
                Some(read_u32(bytes, 20)?),
            )
        }
        VarStoreBackend::NameValue => {
            if bytes.len() < 20 {
                return Err(String::from("VARSTORE_NAME_VALUE opcode is truncated"));
            }
            (
                read_u16(bytes, 2)?,
                read_guid(bytes, 4)?,
                None,
                None,
                None,
            )
        }
        _ => return Err(String::from("unsupported varstore backend")),
    };
    schema.varstores.push(VarStore {
        formset_index,
        list_index: package.list_index,
        package_index: package.package_index,
        id,
        backend,
        guid,
        name,
        size,
        attributes,
        source_offset,
    });
    Ok(())
}

fn parse_question(
    schema: &mut BiosSchema,
    catalogue: &HiiCatalogue,
    package: &FormPackageRecord,
    stack: &mut Vec<ScopeFrame>,
    opcode: u8,
    bytes: &[u8],
    source_offset: u32,
    scope: bool,
) -> Result<(), String> {
    if bytes.len() < QUESTION_HEADER_BYTES {
        return Err(String::from("question opcode is truncated"));
    }
    if schema.stats.questions >= MAX_QUESTIONS {
        return Err(String::from("question count exceeds bound"));
    }
    let (formset_index, form_index) = current_form(stack)
        .ok_or_else(|| String::from("question opcode is outside FORM scope"))?;
    let common = parse_common_question(bytes)?;
    let kind = match opcode {
        IFR_ONE_OF => QuestionKind::OneOf,
        IFR_CHECKBOX => QuestionKind::Checkbox,
        IFR_NUMERIC => QuestionKind::Numeric,
        IFR_STRING => QuestionKind::String,
        IFR_ACTION => QuestionKind::Action,
        _ => return Err(String::from("unsupported question opcode")),
    };
    let mut kind_flags = 0u8;
    let mut numeric = None;
    let mut string_limits = None;
    let width = match kind {
        QuestionKind::OneOf | QuestionKind::Numeric => {
            if bytes.len() < QUESTION_HEADER_BYTES + 1 {
                return Err(String::from("numeric question flags are truncated"));
            }
            kind_flags = bytes[QUESTION_HEADER_BYTES];
            let value_width = numeric_width(kind_flags);
            let data_start = QUESTION_HEADER_BYTES + 1;
            let data_bytes = value_width
                .checked_mul(3)
                .ok_or_else(|| String::from("numeric bounds size overflow"))?;
            checked_end(data_start, data_bytes, bytes.len(), "numeric bounds")?;
            numeric = Some(NumericBounds {
                minimum: read_unsigned(bytes, data_start, value_width)?,
                maximum: read_unsigned(bytes, data_start + value_width, value_width)?,
                step: read_unsigned(bytes, data_start + value_width * 2, value_width)?,
            });
            Some(value_width as u16)
        }
        QuestionKind::Checkbox => {
            if bytes.len() < QUESTION_HEADER_BYTES + 1 {
                return Err(String::from("CHECKBOX flags are truncated"));
            }
            kind_flags = bytes[QUESTION_HEADER_BYTES];
            Some(1)
        }
        QuestionKind::String => {
            if bytes.len() < QUESTION_HEADER_BYTES + 3 {
                return Err(String::from("STRING question is truncated"));
            }
            let minimum_chars = bytes[QUESTION_HEADER_BYTES];
            let maximum_chars = bytes[QUESTION_HEADER_BYTES + 1];
            kind_flags = bytes[QUESTION_HEADER_BYTES + 2];
            if minimum_chars > maximum_chars {
                return Err(String::from("STRING minimum exceeds maximum"));
            }
            string_limits = Some(StringLimits {
                minimum_chars,
                maximum_chars,
                multiline: kind_flags & 0x01 != 0,
            });
            Some(u16::from(maximum_chars).saturating_mul(2))
        }
        QuestionKind::Action => None,
    };
    let prompt = resolve_string(
        catalogue,
        &mut schema.stats,
        package.list_index,
        common.prompt_id,
    );
    let help = resolve_string(
        catalogue,
        &mut schema.stats,
        package.list_index,
        common.help_id,
    );
    let conditions = active_conditions(stack);
    let question_index = schema.formsets[formset_index].forms[form_index]
        .questions
        .len();
    schema.formsets[formset_index].forms[form_index]
        .questions
        .push(Question {
            prompt_id: common.prompt_id,
            prompt,
            help_id: common.help_id,
            help,
            id: common.id,
            kind,
            varstore_id: common.varstore_id,
            varstore_info: common.varstore_info,
            width,
            question_flags: common.flags,
            kind_flags,
            numeric,
            string_limits,
            options: Vec::new(),
            defaults: Vec::new(),
            conditions,
            storage: unresolved_storage(common.varstore_id, width),
            source_offset,
        });
    schema.stats.questions = schema.stats.questions.saturating_add(1);
    if scope {
        stack.push(ScopeFrame::Question {
            formset_index,
            form_index,
            question_index,
        });
    }
    Ok(())
}

