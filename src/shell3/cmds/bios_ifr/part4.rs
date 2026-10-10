fn parse_option(
    schema: &mut BiosSchema,
    catalogue: &HiiCatalogue,
    package: &FormPackageRecord,
    stack: &[ScopeFrame],
    bytes: &[u8],
    source_offset: u32,
    scope: bool,
) -> Result<(), String> {
    if bytes.len() < 6 {
        return Err(String::from("ONE_OF_OPTION opcode is truncated"));
    }
    let (formset_index, form_index, question_index) = current_question(stack)
        .ok_or_else(|| String::from("ONE_OF_OPTION is outside question scope"))?;
    if schema.formsets[formset_index].forms[form_index].questions[question_index].kind
        != QuestionKind::OneOf
    {
        return Err(String::from("ONE_OF_OPTION is not enclosed by ONE_OF"));
    }
    let text_id = read_u16(bytes, 2)?;
    let flags = bytes[4];
    let type_code = bytes[5];
    let value = parse_value(bytes, type_code, 6)?;
    let text = resolve_string(catalogue, &mut schema.stats, package.list_index, text_id);
    schema.formsets[formset_index].forms[form_index].questions[question_index]
        .options
        .push(QuestionOption {
            text_id,
            text,
            flags,
            value,
            source_offset,
        });
    if scope {
        return Err(String::from("scoped ONE_OF_OPTION is outside conservative subset"));
    }
    Ok(())
}

fn parse_default(
    schema: &mut BiosSchema,
    package: &FormPackageRecord,
    stack: &mut Vec<ScopeFrame>,
    bytes: &[u8],
    source_offset: u32,
    scope: bool,
) -> Result<(), String> {
    if bytes.len() < 5 {
        return Err(String::from("DEFAULT opcode is truncated"));
    }
    let (formset_index, form_index, question_index) = current_question(stack)
        .ok_or_else(|| String::from("DEFAULT is outside question scope"))?;
    let default_id = read_u16(bytes, 2)?;
    let type_code = bytes[4];
    let value = if bytes.len() > 5 {
        Some(parse_value(bytes, type_code, 5)?)
    } else {
        None
    };
    schema.formsets[formset_index].forms[form_index].questions[question_index]
        .defaults
        .push(QuestionDefault {
            default_id,
            label: default_id_fallback(default_id),
            value,
            source: if scope {
                "ifr-default-expression"
            } else {
                "ifr-default"
            },
            source_offset,
        });
    if scope {
        stack.push(ScopeFrame::Opaque);
    }
    let _ = package;
    Ok(())
}

fn parse_default_store(
    schema: &mut BiosSchema,
    catalogue: &HiiCatalogue,
    package: &FormPackageRecord,
    stack: &[ScopeFrame],
    bytes: &[u8],
    source_offset: u32,
) -> Result<(), String> {
    if bytes.len() < 6 {
        return Err(String::from("DEFAULTSTORE opcode is truncated"));
    }
    let name_id = read_u16(bytes, 2)?;
    let id = read_u16(bytes, 4)?;
    let name = resolve_string(catalogue, &mut schema.stats, package.list_index, name_id);
    schema.default_stores.push(DefaultStore {
        formset_index: current_formset(stack),
        list_index: package.list_index,
        package_index: package.package_index,
        name_id,
        name,
        id,
        source_offset,
    });
    Ok(())
}

fn parse_condition(
    stack: &mut Vec<ScopeFrame>,
    opcode: u8,
    source_offset: u32,
    scope: bool,
) -> Result<(), String> {
    if !scope {
        return Err(String::from("visibility condition is not scoped"));
    }
    let kind = match opcode {
        IFR_SUPPRESS_IF => ConditionKind::Suppress,
        IFR_GRAY_OUT_IF => ConditionKind::GrayOut,
        IFR_DISABLE_IF => ConditionKind::Disable,
        _ => return Err(String::from("unsupported visibility condition")),
    };
    stack.push(ScopeFrame::Condition(VisibilityCondition {
        kind,
        source_offset,
        expression: Vec::new(),
    }));
    Ok(())
}

fn preserve_unknown(
    schema: &mut BiosSchema,
    package: &FormPackageRecord,
    stack: &mut Vec<ScopeFrame>,
    opcode: u8,
    length: u8,
    scope: bool,
    source_offset: u32,
    bytes: &[u8],
) -> Result<(), String> {
    if schema.unknown_opcodes.len() >= MAX_UNKNOWN_OPCODES {
        return Err(String::from("unknown IFR opcode count exceeds bound"));
    }
    let opaque = OpaqueOpcode {
        list_index: package.list_index,
        package_index: package.package_index,
        source_offset,
        opcode,
        length,
        scope,
        raw: bytes.to_vec(),
    };
    if is_expression_opcode(opcode) {
        for frame in stack.iter_mut() {
            if let ScopeFrame::Condition(condition) = frame {
                condition.expression.push(opaque.clone());
            }
        }
    }
    schema.unknown_opcodes.push(opaque);
    if scope {
        stack.push(ScopeFrame::Opaque);
    }
    Ok(())
}

fn preserve_recognized_unscoped(
    schema: &mut BiosSchema,
    package: &FormPackageRecord,
    opcode: u8,
    source_offset: u32,
    bytes: &[u8],
    stack: &mut Vec<ScopeFrame>,
) -> Result<(), String> {
    preserve_unknown(
        schema,
        package,
        stack,
        opcode,
        bytes.len() as u8,
        false,
        source_offset,
        bytes,
    )
}

