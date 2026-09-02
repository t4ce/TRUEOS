use alloc::string::String;
use alloc::vec::Vec;
use core::mem::size_of;

use spin::Mutex;

use crate::efi::EfiGuid;

use super::bios_hii::{CaptureMetadata, FormPackageRecord, HiiCatalogue};

const IFR_FORM: u8 = 0x01;
const IFR_ONE_OF: u8 = 0x05;
const IFR_CHECKBOX: u8 = 0x06;
const IFR_NUMERIC: u8 = 0x07;
const IFR_ONE_OF_OPTION: u8 = 0x09;
const IFR_SUPPRESS_IF: u8 = 0x0a;
const IFR_ACTION: u8 = 0x0c;
const IFR_FORM_SET: u8 = 0x0e;
const IFR_GRAY_OUT_IF: u8 = 0x19;
const IFR_STRING: u8 = 0x1c;
const IFR_DISABLE_IF: u8 = 0x1e;
const IFR_VARSTORE: u8 = 0x24;
const IFR_VARSTORE_NAME_VALUE: u8 = 0x25;
const IFR_VARSTORE_EFI: u8 = 0x26;
const IFR_END: u8 = 0x29;
const IFR_DEFAULT: u8 = 0x5b;
const IFR_DEFAULTSTORE: u8 = 0x5c;

const QUESTION_HEADER_BYTES: usize = 13;
const MAX_FORMSETS: usize = 4096;
const MAX_FORMS: usize = 65_536;
const MAX_QUESTIONS: usize = 262_144;
const MAX_VARSTORES: usize = 65_536;
const MAX_UNKNOWN_OPCODES: usize = 1_000_000;
const MAX_SCOPE_DEPTH: usize = 256;

const QUESTION_FLAG_READ_ONLY: u8 = 0x01;
const QUESTION_FLAG_CALLBACK: u8 = 0x04;
const QUESTION_FLAG_RESET_REQUIRED: u8 = 0x10;
const CHECKBOX_DEFAULT: u8 = 0x01;
const CHECKBOX_DEFAULT_MFG: u8 = 0x02;
const OPTION_DEFAULT: u8 = 0x10;
const OPTION_DEFAULT_MFG: u8 = 0x20;

static SCHEMA_CACHE: Mutex<Option<Result<BiosSchema, String>>> = Mutex::new(None);

pub(crate) struct BiosSchema {
    pub capture: CaptureMetadata,
    pub package_lists: usize,
    pub packages: usize,
    pub catalogue_strings_resolved: u32,
    pub formsets: Vec<FormSet>,
    pub varstores: Vec<VarStore>,
    pub default_stores: Vec<DefaultStore>,
    pub unknown_opcodes: Vec<OpaqueOpcode>,
    pub stats: SchemaStats,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct SchemaStats {
    pub malformed_packages: u32,
    pub forms: usize,
    pub questions: usize,
    pub string_references_resolved: u32,
    pub string_references_unresolved: u32,
}

pub(crate) struct FormSet {
    pub list_index: usize,
    pub package_index: usize,
    pub guid: EfiGuid,
    pub title_id: u16,
    pub title: Option<String>,
    pub help_id: u16,
    pub help: Option<String>,
    pub flags: u8,
    pub forms: Vec<Form>,
}

pub(crate) struct Form {
    pub id: u16,
    pub title_id: u16,
    pub title: Option<String>,
    pub source_offset: u32,
    pub questions: Vec<Question>,
}

pub(crate) struct Question {
    pub prompt_id: u16,
    pub prompt: Option<String>,
    pub help_id: u16,
    pub help: Option<String>,
    pub id: u16,
    pub kind: QuestionKind,
    pub varstore_id: u16,
    pub varstore_info: u16,
    pub width: Option<u16>,
    pub question_flags: u8,
    pub kind_flags: u8,
    pub numeric: Option<NumericBounds>,
    pub string_limits: Option<StringLimits>,
    pub options: Vec<QuestionOption>,
    pub defaults: Vec<QuestionDefault>,
    pub conditions: Vec<VisibilityCondition>,
    pub storage: StorageBinding,
    pub source_offset: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuestionKind {
    OneOf,
    Checkbox,
    Numeric,
    String,
    Action,
}

impl QuestionKind {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::OneOf => "one-of",
            Self::Checkbox => "checkbox",
            Self::Numeric => "numeric",
            Self::String => "string",
            Self::Action => "action",
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct NumericBounds {
    pub minimum: u64,
    pub maximum: u64,
    pub step: u64,
}

#[derive(Clone, Copy)]
pub(crate) struct StringLimits {
    pub minimum_chars: u8,
    pub maximum_chars: u8,
    pub multiline: bool,
}

pub(crate) struct QuestionOption {
    pub text_id: u16,
    pub text: Option<String>,
    pub flags: u8,
    pub value: IfrValue,
    pub source_offset: u32,
}

pub(crate) struct QuestionDefault {
    pub default_id: u16,
    pub label: String,
    pub value: Option<IfrValue>,
    pub source: &'static str,
    pub source_offset: u32,
}

#[derive(Clone)]
pub(crate) struct IfrValue {
    pub type_code: u8,
    pub raw: Vec<u8>,
    pub unsigned: Option<u64>,
    pub boolean: Option<bool>,
    pub string_id: Option<u16>,
}

pub(crate) struct VarStore {
    pub formset_index: usize,
    pub list_index: usize,
    pub package_index: usize,
    pub id: u16,
    pub backend: VarStoreBackend,
    pub guid: EfiGuid,
    pub name: Option<String>,
    pub size: Option<u16>,
    pub attributes: Option<u32>,
    pub source_offset: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum VarStoreBackend {
    Buffer,
    Efi,
    NameValue,
    None,
    Missing,
}

impl VarStoreBackend {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Buffer => "buffer-varstore",
            Self::Efi => "efi-varstore",
            Self::NameValue => "name-value-varstore",
            Self::None => "none",
            Self::Missing => "missing",
        }
    }
}

pub(crate) struct StorageBinding {
    pub backend: VarStoreBackend,
    pub varstore_id: u16,
    pub variable: Option<String>,
    pub variable_guid: Option<EfiGuid>,
    pub offset: Option<u16>,
    pub width: Option<u16>,
    pub attributes: Option<u32>,
    pub valid: bool,
    pub detail: &'static str,
}

pub(crate) struct DefaultStore {
    pub formset_index: Option<usize>,
    pub list_index: usize,
    pub package_index: usize,
    pub name_id: u16,
    pub name: Option<String>,
    pub id: u16,
    pub source_offset: u32,
}

#[derive(Clone)]
pub(crate) struct VisibilityCondition {
    pub kind: ConditionKind,
    pub source_offset: u32,
    pub expression: Vec<OpaqueOpcode>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConditionKind {
    Suppress,
    GrayOut,
    Disable,
}

impl ConditionKind {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Suppress => "suppress-if",
            Self::GrayOut => "gray-out-if",
            Self::Disable => "disable-if",
        }
    }
}

#[derive(Clone)]
pub(crate) struct OpaqueOpcode {
    pub list_index: usize,
    pub package_index: usize,
    pub source_offset: u32,
    pub opcode: u8,
    pub length: u8,
    pub scope: bool,
    pub raw: Vec<u8>,
}

enum ScopeFrame {
    FormSet(usize),
    Form {
        formset_index: usize,
        form_index: usize,
    },
    Question {
        formset_index: usize,
        form_index: usize,
        question_index: usize,
    },
    Condition(VisibilityCondition),
    Opaque,
}

struct CommonQuestion {
    prompt_id: u16,
    help_id: u16,
    id: u16,
    varstore_id: u16,
    varstore_info: u16,
    flags: u8,
}

pub(crate) fn with_schema<R>(f: impl FnOnce(&BiosSchema) -> R) -> Result<R, String> {
    let mut cache = SCHEMA_CACHE.lock();
    if cache.is_none() {
        *cache = Some(build_schema());
    }
    match cache.as_ref().expect("schema cache initialized") {
        Ok(schema) => Ok(f(schema)),
        Err(error) => Err(error.clone()),
    }
}

impl BiosSchema {
    pub(crate) const fn form_count(&self) -> usize {
        self.stats.forms
    }

    pub(crate) const fn question_count(&self) -> usize {
        self.stats.questions
    }

    pub(crate) fn state(&self) -> &'static str {
        if self.formsets.is_empty() || self.form_count() == 0 || self.question_count() == 0 {
            "unavailable"
        } else if self.stats.malformed_packages != 0 {
            "degraded"
        } else {
            "ready"
        }
    }
}

impl Question {
    pub(crate) const fn read_only(&self) -> bool {
        self.question_flags & QUESTION_FLAG_READ_ONLY != 0
    }

    pub(crate) const fn callback(&self) -> bool {
        self.question_flags & QUESTION_FLAG_CALLBACK != 0
    }

    pub(crate) const fn requires_reset(&self) -> bool {
        self.question_flags & QUESTION_FLAG_RESET_REQUIRED != 0
    }
}

fn build_schema() -> Result<BiosSchema, String> {
    super::bios_hii::with_catalogue(parse_catalogue_schema)?
}

fn parse_catalogue_schema(catalogue: &HiiCatalogue) -> Result<BiosSchema, String> {
    let mut schema = BiosSchema {
        capture: catalogue.capture,
        package_lists: catalogue.lists.len(),
        packages: catalogue.packages.len(),
        catalogue_strings_resolved: catalogue.stats.decoded_strings,
        formsets: Vec::new(),
        varstores: Vec::new(),
        default_stores: Vec::new(),
        unknown_opcodes: Vec::new(),
        stats: SchemaStats {
            malformed_packages: catalogue.stats.malformed_packages,
            ..SchemaStats::default()
        },
    };

    for package in &catalogue.form_packages {
        let formset_checkpoint = schema.formsets.len();
        let varstore_checkpoint = schema.varstores.len();
        let default_checkpoint = schema.default_stores.len();
        let unknown_checkpoint = schema.unknown_opcodes.len();
        let stats_checkpoint = schema.stats;
        if parse_form_package(&mut schema, catalogue, package).is_err() {
            schema.formsets.truncate(formset_checkpoint);
            schema.varstores.truncate(varstore_checkpoint);
            schema.default_stores.truncate(default_checkpoint);
            schema.unknown_opcodes.truncate(unknown_checkpoint);
            schema.stats = stats_checkpoint;
            schema.stats.malformed_packages =
                schema.stats.malformed_packages.saturating_add(1);
        }
    }

    attach_storage_and_defaults(&mut schema, catalogue);
    Ok(schema)
}

