use alloc::string::String;
use core::fmt::Write;

use crate::shell2::shell2_cmd::ParseOutcome;

use super::super::{ShellBackend2, print_shell_line};
use super::bios_catalogue::{self, BiosCatalogue};

pub(crate) fn try_parse(
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> Option<ParseOutcome> {
    let mut args = rest.split_whitespace();
    let command = args.next()?;

    if command.eq_ignore_ascii_case("packages") {
        if args.next().is_some() {
            usage(io);
        } else {
            emit_catalogue(io, append_packages);
        }
        return Some(ParseOutcome::Handled);
    }
    if command.eq_ignore_ascii_case("languages") {
        if args.next().is_some() {
            usage(io);
        } else {
            emit_catalogue(io, append_languages);
        }
        return Some(ParseOutcome::Handled);
    }
    if command.eq_ignore_ascii_case("strings") {
        match (args.next(), args.next()) {
            (Some(status), None) if status.eq_ignore_ascii_case("status") => {
                emit_catalogue(io, append_string_status)
            }
            _ => usage(io),
        }
        return Some(ParseOutcome::Handled);
    }
    if matches!(command, "help" | "-h" | "--help") {
        usage(io);
        return Some(ParseOutcome::Handled);
    }
    None
}

fn emit_catalogue(io: &'static dyn ShellBackend2, render: fn(&BiosCatalogue, &mut String)) {
    let text = match bios_catalogue::with_catalogue(|catalogue| {
        let mut out = String::new();
        render(catalogue, &mut out);
        out
    }) {
        Ok(text) => text,
        Err(error) => {
            let mut out = String::new();
            writeln!(out, "state=unavailable detail=\"{}\"", escaped(&error)).unwrap();
            writeln!(out, "active_write_path=none").unwrap();
            out
        }
    };
    emit(io, &text);
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

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(
        io,
        "bios: read-only catalogue `bios packages` | `bios languages` | `bios strings status`",
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
