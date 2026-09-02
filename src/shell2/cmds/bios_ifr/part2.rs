fn parse_form_package(
    schema: &mut BiosSchema,
    catalogue: &HiiCatalogue,
    package: &FormPackageRecord,
) -> Result<(), String> {
    if package.bytes.len() < 4 {
        return Err(String::from("form package header is truncated"));
    }
    let raw_header = read_u32(&package.bytes, 0)?;
    let package_len = (raw_header & 0x00ff_ffff) as usize;
    let package_type = (raw_header >> 24) as u8;
    if package_type != 0x02 || package_len != package.bytes.len() {
        return Err(String::from("form package length/type mismatch"));
    }

    let mut stack = Vec::<ScopeFrame>::new();
    let mut cursor = 4usize;
    while cursor < package.bytes.len() {
        if stack.len() > MAX_SCOPE_DEPTH {
            return Err(String::from("IFR scope depth exceeds bound"));
        }
        let header_end = checked_end(cursor, 2, package.bytes.len(), "IFR opcode header")?;
        let opcode = package.bytes[cursor];
        let length = package.bytes[cursor + 1] & 0x7f;
        let scope = package.bytes[cursor + 1] & 0x80 != 0;
        if length < 2 {
            return Err(alloc::format!(
                "IFR opcode 0x{:02X} has invalid length={}",
                opcode,
                length
            ));
        }
        let opcode_end = checked_end(
            cursor,
            length as usize,
            package.bytes.len(),
            "IFR opcode",
        )?;
        let bytes = &package.bytes[cursor..opcode_end];
        let source_offset = u32::try_from(cursor)
            .map_err(|_| String::from("IFR opcode offset exceeds u32"))?;
        let _ = header_end;

        match opcode {
            IFR_FORM_SET => parse_formset(
                schema,
                catalogue,
                package,
                &mut stack,
                bytes,
                source_offset,
                scope,
            )?,
            IFR_FORM => parse_form(
                schema,
                catalogue,
                package,
                &mut stack,
                bytes,
                source_offset,
                scope,
            )?,
            IFR_VARSTORE => parse_varstore(
                schema,
                package,
                &stack,
                bytes,
                source_offset,
                VarStoreBackend::Buffer,
            )?,
            IFR_VARSTORE_EFI => parse_varstore(
                schema,
                package,
                &stack,
                bytes,
                source_offset,
                VarStoreBackend::Efi,
            )?,
            IFR_VARSTORE_NAME_VALUE => parse_varstore(
                schema,
                package,
                &stack,
                bytes,
                source_offset,
                VarStoreBackend::NameValue,
            )?,
            IFR_ONE_OF | IFR_CHECKBOX | IFR_NUMERIC | IFR_STRING | IFR_ACTION => {
                parse_question(
                    schema,
                    catalogue,
                    package,
                    &mut stack,
                    opcode,
                    bytes,
                    source_offset,
                    scope,
                )?
            }
            IFR_ONE_OF_OPTION => parse_option(
                schema,
                catalogue,
                package,
                &stack,
                bytes,
                source_offset,
                scope,
            )?,
            IFR_DEFAULT => parse_default(
                schema,
                package,
                &mut stack,
                bytes,
                source_offset,
                scope,
            )?,
            IFR_DEFAULTSTORE => parse_default_store(
                schema,
                catalogue,
                package,
                &stack,
                bytes,
                source_offset,
            )?,
            IFR_SUPPRESS_IF | IFR_GRAY_OUT_IF | IFR_DISABLE_IF => {
                parse_condition(&mut stack, opcode, source_offset, scope)?
            }
            IFR_END => {
                if bytes.len() != 2 {
                    return Err(String::from("IFR END opcode length is not 2"));
                }
                stack
                    .pop()
                    .ok_or_else(|| String::from("IFR END has no enclosing scope"))?;
            }
            _ => preserve_unknown(
                schema,
                package,
                &mut stack,
                opcode,
                length,
                scope,
                source_offset,
                bytes,
            )?,
        }
        cursor = opcode_end;
    }
    if cursor != package.bytes.len() {
        return Err(String::from("form package ended off an IFR opcode boundary"));
    }
    if !stack.is_empty() {
        return Err(String::from("form package ended with unterminated IFR scopes"));
    }
    Ok(())
}

