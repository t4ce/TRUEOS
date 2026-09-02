use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;
use core::mem::size_of;

use spin::Mutex;

use crate::efi::EfiGuid;
use crate::shell2::shell2_cmd::ParseOutcome;

use super::super::{ShellBackend2, print_shell_line};

const MAX_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;
const MAX_HII_BYTES: usize = 12 * 1024 * 1024;
const MAX_SECTIONS: usize = 8;
const MAX_PACKAGE_LISTS: usize = 4096;
const MAX_PACKAGES: usize = 65_536;
const MAX_STRING_PACKAGES: usize = 4096;
const MAX_FORM_PACKAGES: usize = 4096;
const MAX_STORED_STRING_CHARS: usize = 2048;

const CATALOG_MAGIC: [u8; 8] = *b"TRBIOS1\0";
const PAYLOAD_MAGIC: [u8; 8] = *b"TRPAY1\0\0";
const VERSION: u16 = 1;

const SEC_HII: u32 = 2;
const SEC_CONFIG: u32 = 3;
const HII_FORMS: u8 = 0x02;
const HII_STRINGS: u8 = 0x04;
const HII_END: u8 = 0xdf;

const SIBT_END: u8 = 0x00;
const SIBT_STRING_SCSU: u8 = 0x10;
const SIBT_STRING_SCSU_FONT: u8 = 0x11;
const SIBT_STRINGS_SCSU: u8 = 0x12;
const SIBT_STRINGS_SCSU_FONT: u8 = 0x13;
const SIBT_STRING_UCS2: u8 = 0x14;
const SIBT_STRING_UCS2_FONT: u8 = 0x15;
const SIBT_STRINGS_UCS2: u8 = 0x16;
const SIBT_STRINGS_UCS2_FONT: u8 = 0x17;
const SIBT_DUPLICATE: u8 = 0x20;
const SIBT_SKIP2: u8 = 0x21;
const SIBT_SKIP1: u8 = 0x22;
const SIBT_EXT1: u8 = 0x30;
const SIBT_EXT2: u8 = 0x31;
const SIBT_EXT4: u8 = 0x32;

const TRUEOS_BIOS_CATALOG_GUID: EfiGuid = EfiGuid {
    data1: 0x184d_a5de,
    data2: 0xfa77,
    data3: 0x4a1f,
    data4: [0xb4, 0x27, 0xd4, 0xdb, 0xfc, 0xe6, 0xd7, 0xf7],
};

static CATALOGUE_CACHE: Mutex<Option<Result<HiiCatalogue, String>>> = Mutex::new(None);

#[repr(C)]
#[derive(Clone, Copy)]
struct CatalogHeader {
    magic: [u8; 8],
    version: u16,
    header_bytes: u16,
    flags: u32,
    package_list_count: u32,
    formset_count: u32,
    question_count: u32,
    payload_bytes: u32,
    payload_crc32: u32,
    reserved: u32,
    payload_phys: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct PayloadHeader {
    magic: [u8; 8],
    version: u16,
    header_bytes: u16,
    section_entry_bytes: u16,
    reserved0: u16,
    section_count: u32,
    total_bytes: u32,
    capture_flags: u32,
    reserved1: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct SectionEntry {
    kind: u32,
    flags: u32,
    offset: u32,
    length: u32,
    crc32: u32,
    reserved: u32,
    status: u64,
}

#[derive(Clone, Copy)]
pub(crate) struct CaptureMetadata {
    pub source: &'static str,
    pub hii_bytes: usize,
    pub config_captured: bool,
}

pub(crate) struct HiiCatalogue {
    pub capture: CaptureMetadata,
    pub lists: Vec<PackageListRecord>,
    pub packages: Vec<PackageRecord>,
    pub string_packages: Vec<StringPackageRecord>,
    pub form_packages: Vec<FormPackageRecord>,
    pub stats: CatalogueStats,
}

pub(crate) struct PackageListRecord {
    pub guid: EfiGuid,
    pub first_package: usize,
    pub package_count: usize,
}

pub(crate) struct PackageRecord {
    pub list_index: usize,
    pub package_index: usize,
    pub package_type: u8,
    pub offset: u32,
    pub length: u32,
}

pub(crate) struct StringPackageRecord {
    pub list_index: usize,
    pub package_index: usize,
    pub language: String,
    pub language_name_id: u16,
    pub strings: BTreeMap<u16, ResolvedString>,
    pub max_string_id: u16,
    pub duplicate_blocks: u32,
    pub unresolved_duplicates: u32,
    pub skipped_ids: u32,
    pub extension_blocks: u32,
    pub truncated_strings: u32,
}

#[derive(Clone)]
pub(crate) struct ResolvedString {
    pub text: String,
    pub source: StringSource,
    pub truncated: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum StringSource {
    Scsu,
    Ucs2,
    Duplicate,
}

pub(crate) struct FormPackageRecord {
    pub list_index: usize,
    pub package_index: usize,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct CatalogueStats {
    pub malformed_packages: u32,
    pub decoded_strings: u32,
    pub duplicate_strings: u32,
    pub unresolved_duplicates: u32,
    pub skipped_string_ids: u32,
    pub extension_blocks: u32,
    pub truncated_strings: u32,
}

struct CapturedSections {
    source: &'static str,
    hii: &'static [u8],
    config_captured: bool,
}

struct DecodedText {
    text: String,
    consumed: usize,
    truncated: bool,
}

pub(crate) fn try_parse(
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> Option<ParseOutcome> {
    let mut args = rest.split_whitespace();
    let command = args.next()?;
    match command {
        "packages" if args.next().is_none() => emit_catalogue(io, append_packages),
        "languages" if args.next().is_none() => emit_catalogue(io, append_languages),
        "strings" => match (args.next(), args.next()) {
            (Some("status"), None) => emit_catalogue(io, append_string_status),
            _ => {
                print_shell_line(io, "bios: usage `bios strings status`");
                return Some(ParseOutcome::Handled);
            }
        },
        _ => return None,
    }
    Some(ParseOutcome::Handled)
}

pub(crate) fn with_catalogue<R>(
    f: impl FnOnce(&HiiCatalogue) -> R,
) -> Result<R, String> {
    let mut cache = CATALOGUE_CACHE.lock();
    if cache.is_none() {
        *cache = Some(build_catalogue());
    }
    match cache.as_ref().expect("catalogue cache initialized") {
        Ok(catalogue) => Ok(f(catalogue)),
        Err(error) => Err(error.clone()),
    }
}

impl HiiCatalogue {
    pub(crate) fn resolve_string(&self, list_index: usize, string_id: u16) -> Option<&str> {
        if string_id == 0 {
            return None;
        }
        let mut best: Option<(&str, u8, usize)> = None;
        for (index, package) in self.string_packages.iter().enumerate() {
            if package.list_index != list_index {
                continue;
            }
            let Some(resolved) = package.strings.get(&string_id) else {
                continue;
            };
            let priority = language_priority(&package.language);
            let replace = match best.as_ref() {
                None => true,
                Some((_, best_priority, best_index)) => {
                    priority < *best_priority
                        || (priority == *best_priority && index < *best_index)
                }
            };
            if replace {
                best = Some((&resolved.text, priority, index));
            }
        }
        best.map(|(text, _, _)| text)
    }

    pub(crate) fn resolve_string_owned(
        &self,
        list_index: usize,
        string_id: u16,
    ) -> Option<String> {
        self.resolve_string(list_index, string_id).map(String::from)
    }
}

fn emit_catalogue(
    io: &'static dyn ShellBackend2,
    append: fn(&mut String, &HiiCatalogue),
) {
    let mut out = String::new();
    match with_catalogue(|catalogue| append(&mut out, catalogue)) {
        Ok(()) => {}
        Err(error) => {
            writeln!(out, "state=unavailable").unwrap();
            writeln!(out, "detail=\"{}\"", single_line(&error, 240)).unwrap();
            writeln!(out, "active_write_path=none").unwrap();
        }
    }
    for line in out.lines() {
        print_shell_line(io, line.trim_end_matches('\r'));
    }
}

fn append_packages(out: &mut String, catalogue: &HiiCatalogue) {
    writeln!(out, "=== Captured HII package catalogue ===").unwrap();
    writeln!(
        out,
        "state=ready source={} package_lists={} packages={} malformed_packages={} cache=parsed-once",
        catalogue.capture.source,
        catalogue.lists.len(),
        catalogue.packages.len(),
        catalogue.stats.malformed_packages
    )
    .unwrap();
    for (list_index, list) in catalogue.lists.iter().enumerate() {
        let mut types = BTreeMap::<u8, u32>::new();
        for package in catalogue
            .packages
            .iter()
            .filter(|package| package.list_index == list_index)
        {
            *types.entry(package.package_type).or_default() += 1;
        }
        write!(
            out,
            "list={} guid={} packages={} types=",
            list_index,
            list.guid.fmt_canonical(),
            list.package_count
        )
        .unwrap();
        if types.is_empty() {
            write!(out, "none").unwrap();
        } else {
            for (position, (package_type, count)) in types.iter().enumerate() {
                if position != 0 {
                    write!(out, ",").unwrap();
                }
                write!(
                    out,
                    "0x{:02X}({}):{}",
                    package_type,
                    package_type_name(*package_type),
                    count
                )
                .unwrap();
            }
        }
        writeln!(out).unwrap();
    }
    writeln!(out, "raw_package_bytes=hidden").unwrap();
    writeln!(out, "active_write_path=none").unwrap();
}

fn append_languages(out: &mut String, catalogue: &HiiCatalogue) {
    writeln!(out, "=== Captured HII languages ===").unwrap();
    writeln!(
        out,
        "state=ready string_packages={} strings_resolved={} malformed_packages={}",
        catalogue.string_packages.len(),
        catalogue.stats.decoded_strings,
        catalogue.stats.malformed_packages
    )
    .unwrap();
    if catalogue.string_packages.is_empty() {
        writeln!(out, "language=none").unwrap();
    }
    for package in &catalogue.string_packages {
        let language_name = package
            .strings
            .get(&package.language_name_id)
            .map(|entry| single_line(&entry.text, 120))
            .unwrap_or_else(|| String::from("-"));
        writeln!(
            out,
            "list={} package={} language=\"{}\" language_name_id=0x{:04X} language_name=\"{}\" max_string_id=0x{:04X} resolved={} duplicates={} unresolved_duplicates={} skipped_ids={} extensions={} truncated={}",
            package.list_index,
            package.package_index,
            single_line(&package.language, 80),
            package.language_name_id,
            language_name,
            package.max_string_id,
            package.strings.len(),
            package.duplicate_blocks,
            package.unresolved_duplicates,
            package.skipped_ids,
            package.extension_blocks,
            package.truncated_strings
        )
        .unwrap();
    }
    writeln!(out, "bulk_strings=hidden").unwrap();
    writeln!(out, "active_write_path=none").unwrap();
}

fn append_string_status(out: &mut String, catalogue: &HiiCatalogue) {
    writeln!(out, "=== HII string decoder status ===").unwrap();
    writeln!(out, "state=ready cache=parsed-once").unwrap();
    writeln!(out, "string_packages={}", catalogue.string_packages.len()).unwrap();
    writeln!(out, "strings_resolved={}", catalogue.stats.decoded_strings).unwrap();
    writeln!(out, "duplicate_strings={}", catalogue.stats.duplicate_strings).unwrap();
    writeln!(
        out,
        "unresolved_duplicates={}",
        catalogue.stats.unresolved_duplicates
    )
    .unwrap();
    writeln!(out, "skipped_string_ids={}", catalogue.stats.skipped_string_ids).unwrap();
    writeln!(out, "extension_blocks={}", catalogue.stats.extension_blocks).unwrap();
    writeln!(out, "truncated_strings={}", catalogue.stats.truncated_strings).unwrap();
    writeln!(out, "malformed_packages={}", catalogue.stats.malformed_packages).unwrap();
    writeln!(out, "bulk_strings=hidden").unwrap();
    writeln!(
        out,
        "current_configuration={}",
        if catalogue.capture.config_captured {
            "captured-redacted"
        } else {
            "not-captured"
        }
    )
    .unwrap();
    writeln!(out, "active_write_path=none").unwrap();
}

