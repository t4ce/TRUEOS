fn append_storage(out: &mut String, schema: &BiosSchema, question: &Question) {
    let storage = &question.storage;
    writeln!(out, "Storage").unwrap();
    writeln!(out, "  backend         {}", storage.backend.name()).unwrap();
    writeln!(out, "  varstore_id     0x{:04X}", storage.varstore_id).unwrap();
    writeln!(
        out,
        "  variable        {}",
        storage
            .variable
            .as_deref()
            .map(|text| single_line(text, 160))
            .unwrap_or_else(|| String::from("-"))
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
    writeln!(out, "  offset          {}", format_optional_hex(storage.offset)).unwrap();
    writeln!(
        out,
        "  width           {}",
        format_optional_decimal(storage.width)
    )
    .unwrap();
    writeln!(
        out,
        "  attributes      {}",
        storage
            .attributes
            .map(|attributes| alloc::format!("0x{:08X}", attributes))
            .unwrap_or_else(|| String::from("-"))
    )
    .unwrap();
    writeln!(out, "  validated       {}", yes_no(storage.valid)).unwrap();
    writeln!(out, "  detail          {}", storage.detail).unwrap();
    writeln!(
        out,
        "  config_content  {}",
        if schema.capture.config_captured {
            "captured-redacted"
        } else {
            "not-captured"
        }
    )
    .unwrap();
}

fn append_options(out: &mut String, question: &Question) {
    writeln!(out, "Options").unwrap();
    writeln!(out, "  count           {}", question.options.len()).unwrap();
    for option in &question.options {
        append_option(out, option);
    }
    writeln!(out, "Default").unwrap();
    if question.defaults.is_empty() {
        writeln!(out, "  value           none").unwrap();
    } else {
        for default in &question.defaults {
            append_default(out, default);
        }
    }
    writeln!(out, "  trueos_write    locked").unwrap();
}

fn append_option(out: &mut String, option: &QuestionOption) {
    writeln!(
        out,
        "  value={} label=\"{}\" string_id=0x{:04X} flags=0x{:02X}",
        format_value(&option.value),
        optional_text(&option.text, option.text_id),
        option.text_id,
        option.flags
    )
    .unwrap();
}

fn append_default(out: &mut String, default: &QuestionDefault) {
    writeln!(
        out,
        "  value={} default_id=0x{:04X} label=\"{}\" source={}",
        default
            .value
            .as_ref()
            .map(format_value)
            .unwrap_or_else(|| String::from("expression-or-unavailable")),
        default.default_id,
        single_line(&default.label, 120),
        default.source
    )
    .unwrap();
}

fn question_matches(
    formset: &FormSet,
    form: &Form,
    question: &Question,
    folded_query: &str,
) -> bool {
    contains_folded(formset.title.as_deref(), folded_query)
        || contains_folded(formset.help.as_deref(), folded_query)
        || contains_folded(form.title.as_deref(), folded_query)
        || contains_folded(question.prompt.as_deref(), folded_query)
        || contains_folded(question.help.as_deref(), folded_query)
        || question
            .options
            .iter()
            .any(|option| contains_folded(option.text.as_deref(), folded_query))
        || question
            .defaults
            .iter()
            .any(|default| default.label.to_ascii_lowercase().contains(folded_query))
}

fn contains_folded(text: Option<&str>, folded_query: &str) -> bool {
    text.is_some_and(|text| text.to_ascii_lowercase().contains(folded_query))
}

fn parse_single_question_id(text: &str) -> Option<u16> {
    if text.is_empty() || text.split_whitespace().count() != 1 {
        return None;
    }
    let value = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"));
    match value {
        Some(hex) if !hex.is_empty() => u16::from_str_radix(hex, 16).ok(),
        Some(_) => None,
        None => text.parse::<u16>().ok(),
    }
}

fn optional_text(text: &Option<String>, string_id: u16) -> String {
    text.as_deref()
        .map(|text| single_line(text, 240))
        .unwrap_or_else(|| {
            if string_id == 0 {
                String::from("-")
            } else {
                alloc::format!("<string 0x{:04X} unresolved>", string_id)
            }
        })
}

fn format_value(value: &IfrValue) -> String {
    if let Some(boolean) = value.boolean {
        return String::from(if boolean { "1 (true)" } else { "0 (false)" });
    }
    if let Some(string_id) = value.string_id {
        return alloc::format!("string-id:0x{:04X}", string_id);
    }
    if let Some(unsigned) = value.unsigned {
        return alloc::format!("{} (0x{:X})", unsigned, unsigned);
    }
    if value.raw.is_empty() {
        return alloc::format!("type:0x{:02X}", value.type_code);
    }
    let mut out = alloc::format!("type:0x{:02X} raw:", value.type_code);
    for byte in value.raw.iter().take(16) {
        write!(out, "{:02X}", byte).unwrap();
    }
    if value.raw.len() > 16 {
        out.push('…');
    }
    out
}

fn format_optional_hex(value: Option<u16>) -> String {
    value
        .map(|value| alloc::format!("0x{:04X}", value))
        .unwrap_or_else(|| String::from("-"))
}

fn format_optional_decimal(value: Option<u16>) -> String {
    value
        .map(|value| alloc::format!("{}", value))
        .unwrap_or_else(|| String::from("-"))
}

const fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

