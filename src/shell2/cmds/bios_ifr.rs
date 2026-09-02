use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use spin::Mutex;

use crate::efi::EfiGuid;

use super::bios_catalogue::{self, BiosCatalogue, FormPackage};

const MAX_FORMSETS: usize = 1_024;
const MAX_FORMS: usize = 8_192;
const MAX_QUESTIONS: usize = 65_535;
const MAX_VARSTORES_PER_FORMSET: usize = 1_024;
const MAX_DEFAULT_STORES_PER_FORMSET: usize = 256;
const MAX_OPTIONS_PER_QUESTION: usize = 1_024;
const MAX_DEFAULTS_PER_QUESTION: usize = 64;
const MAX_SCOPE_DEPTH: usize = 128;
const MAX_CONDITIONS_PER_QUESTION: usize = 32;
const MAX_OPAQUE_GLOBAL: usize = 65_536;
const MAX_OPAQUE_PER_OWNER: usize = 128;
const MAX_EXPRESSION_OPS: usize = 128;
const MAX_VARSTORE_NAME_BYTES: usize = 255;

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

const QUESTION_FLAG_READ_ONLY: u8 = 0x01;
const QUESTION_FLAG_CALLBACK: u8 = 0x04;
const QUESTION_FLAG_RESET_REQUIRED: u8 = 0x10;
const QUESTION_FLAG_RECONNECT_REQUIRED: u8 = 0x40;

const CHECKBOX_DEFAULT: u8 = 0x01;
const CHECKBOX_DEFAULT_MFG: u8 = 0x02;
const OPTION_DEFAULT: u8 = 0x10;
const OPTION_DEFAULT_MFG: u8 = 0x20;

const DEFAULT_STANDARD: u16 = 0x0000;
const DEFAULT_MANUFACTURING: u16 = 0x0001;

static SCHEMA_CACHE: Mutex<Option<Result<BiosSchema, String>>> = Mutex::new(None);

pub(crate) struct BiosSchema {
    pub source: &'static str,
    pub catalogue_malformed_packages: u32,
    pub formsets: Vec<FormSet>,
    pub stats: SchemaStats,
    pub unknown_opcodes: BTreeMap<u8, u32>,
    pub opaque_opcodes: Vec<OpaqueOpcode>,
}

#[derive(Default)]
pub(crate) struct SchemaStats {
    pub form_packages: u32,
    pub parsed_form_packages: u32,
    pub malformed_form_packages: u32,
    pub malformed_opcodes: u32,
    pub forms: u32,
    pub questions: u32,
    pub resolved_strings: u32,
    pub unknown_opcode_instances: u32,
    pub orphan_forms: u32,
    pub orphan_questions: u32,
    pub orphan_options: u32,
    pub orphan_defaults: u32,
    pub orphan_varstores: u32,
    pub duplicate_varstores: u32,
    pub truncated_metadata: u32,
}

pub(crate) struct FormSet {
    pub package_list_index: usize,
    pub package_index: usize,
    pub package_offset: u32,
    pub guid: EfiGuid,
    pub title_id: u16,
    pub title: Option<String>,
    pub help_id: u16,
    pub help: Option<String>,
    pub class_guids: Vec<EfiGuid>,
    pub forms: Vec<Form>,
    pub varstores: Vec<VarStore>,
    pub default_stores: Vec<DefaultStore>,
    pub opaque: Vec<OpaqueOpcode>,
}

pub(crate) struct Form {
    pub form_id: u16,
    pub title_id: u16,
    pub title: Option<String>,
    pub package_offset: u32,
    pub questions: Vec<Question>,
    pub opaque: Vec<OpaqueOpcode>,
}

pub(crate) struct Question {
    pub package_offset: u32,
    pub prompt_id: u16,
    pub prompt: Option<String>,
    pub help_id: u16,
    pub help: Option<String>,
    pub question_id: u16,
    pub kind: QuestionKind,
    pub varstore_id: u16,
    pub varstore_info: u16,
    pub width: Option<u16>,
    pub minimum: Option<u64>,
    pub maximum: Option<u64>,
    pub step: Option<u64>,
    pub min_chars: Option<u16>,
    pub max_chars: Option<u16>,
    pub options: Vec<QuestionOption>,
    pub defaults: Vec<QuestionDefault>,
    pub policy: QuestionPolicy,
    pub storage: QuestionStorage,
    pub conditions: Vec<VisibilityCondition>,
    pub opaque: Vec<OpaqueOpcode>,
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
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::OneOf => "one-of",
            Self::Checkbox => "checkbox",
            Self::Numeric => "numeric",
            Self::String => "string",
            Self::Action => "action",
        }
    }
}

pub(crate) struct QuestionPolicy {
    pub read_only: bool,
    pub callback: bool,
    pub reset_required: bool,
    pub reconnect_required: bool,
}

#[derive(Clone)]
pub(crate) struct QuestionOption {
    pub label_id: u16,
    pub label: Option<String>,
    pub flags: u8,
    pub value: IfrValue,
}

#[derive(Clone)]
pub(crate) struct QuestionDefault {
    pub store_id: u16,
    pub value: Option<IfrValue>,
    pub expression: bool,
    pub source: DefaultSource,
}

#[derive(Clone, Copy)]
pub(crate) enum DefaultSource {
    Opcode,
    OptionFlag,
    CheckboxFlag,
}

impl DefaultSource {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Opcode => "default-opcode",
            Self::OptionFlag => "option-flag",
            Self::CheckboxFlag => "checkbox-flag",
        }
    }
}

#[derive(Clone)]
pub(crate) struct IfrValue {
    pub type_code: u8,
    pub unsigned: Option<u64>,
    pub string_id: Option<u16>,
    pub encoded_width: u8,
}

pub(crate) struct DefaultStore {
    pub id: u16,
    pub name_id: u16,
    pub name: Option<String>,
}

#[derive(Clone)]
pub(crate) struct VarStore {
    pub id: u16,
    pub backend: StorageBackend,
    pub guid: EfiGuid,
    pub name: Option<String>,
    pub size: Option<u16>,
    pub attributes: Option<u32>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum StorageBackend {
    None,
    Buffer,
    Efi,
    NameValue,
    Unknown,
}

impl StorageBackend {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Buffer => "buffer-varstore",
            Self::Efi => "efi-varstore",
            Self::NameValue => "name-value-varstore",
            Self::Unknown => "unknown",
        }
    }
}

pub(crate) struct QuestionStorage {
    pub backend: StorageBackend,
    pub varstore_id: u16,
    pub variable: Option<String>,
    pub variable_guid: Option<EfiGuid>,
    pub offset: Option<u16>,
    pub width: Option<u16>,
    pub attributes: Option<u32>,
    pub valid: bool,
    pub reason: Option<&'static str>,
}

#[derive(Clone)]
pub(crate) struct VisibilityCondition {
    pub kind: ConditionKind,
    pub package_offset: u32,
    pub expression: Vec<OpaqueOpcode>,
    pub expression_truncated: bool,
}

#[derive(Clone, Copy)]
pub(crate) enum ConditionKind {
    Suppress,
    GrayOut,
    Disable,
}

impl ConditionKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Suppress => "suppress-if",
            Self::GrayOut => "gray-out-if",
            Self::Disable => "disable-if",
        }
    }
}

#[derive(Clone)]
pub(crate) struct OpaqueOpcode {
    pub package_list_index: usize,
    pub package_index: usize,
    pub package_offset: u32,
    pub opcode: u8,
    pub length: u8,
    pub scope: bool,
    pub malformed: bool,
}

#[derive(Clone)]
struct ConditionFrame {
    condition: VisibilityCondition,
    expression_open: bool,
}

enum ScopeFrame {
    FormSet(usize),
    Form {
        formset: usize,
        form: usize,
    },
    Question {
        formset: usize,
        form: usize,
        question: usize,
    },
    Condition(ConditionFrame),
    Other,
}

#[derive(Clone, Copy)]
enum Owner {
    Global,
    FormSet(usize),
    Form(usize, usize),
    Question(usize, usize, usize),
}

pub(crate) fn with_schema<R>(f: impl FnOnce(&BiosSchema) -> R) -> Result<R, String> {
    let mut cache = SCHEMA_CACHE.lock();
    if cache.is_none() {
        let parsed = match bios_catalogue::with_catalogue(parse_schema) {
            Ok(result) => result,
            Err(error) => Err(error),
        };
        *cache = Some(parsed);
    }
    match cache.as_ref().expect("schema cache initialized") {
        Ok(schema) => Ok(f(schema)),
        Err(error) => Err(error.clone()),
    }
}

impl BiosSchema {
    pub(crate) fn total_malformed_packages(&self) -> u32 {
        self.catalogue_malformed_packages
            .saturating_add(self.stats.malformed_form_packages)
    }

    pub(crate) fn default_store_name(&self, formset_index: usize, id: u16) -> Option<&str> {
        self.formsets
            .get(formset_index)?
            .default_stores
            .iter()
            .find(|store| store.id == id)?
            .name
            .as_deref()
    }
}

fn parse_schema(catalogue: &BiosCatalogue) -> Result<BiosSchema, String> {
    let mut schema = BiosSchema {
        source: catalogue.source,
        catalogue_malformed_packages: catalogue.malformed_packages,
        formsets: Vec::new(),
        stats: SchemaStats::default(),
        unknown_opcodes: BTreeMap::new(),
        opaque_opcodes: Vec::new(),
    };

    for list in &catalogue.package_lists {
        for package in &list.forms {
            schema.stats.form_packages = schema.stats.form_packages.saturating_add(1);
            parse_form_package(&mut schema, catalogue, package);
        }
    }
    finalize_storage(&mut schema, catalogue);
    Ok(schema)
}

fn parse_form_package(
    schema: &mut BiosSchema,
    catalogue: &BiosCatalogue,
    package: &FormPackage,
) {
    let mut stack = Vec::new();
    let mut offset = 0usize;
    let malformed_before = schema.stats.malformed_form_packages;

    while offset < package.bytes.len() {
        if package.bytes.len() - offset < 2 {
            schema.stats.malformed_form_packages =
                schema.stats.malformed_form_packages.saturating_add(1);
            break;
        }
        let opcode = package.bytes[offset];
        let length = package.bytes[offset + 1] & 0x7f;
        let scope = package.bytes[offset + 1] & 0x80 != 0;
        let length_usize = length as usize;
        let end = match offset.checked_add(length_usize) {
            Some(end) if length_usize >= 2 && end <= package.bytes.len() => end,
            _ => {
                schema.stats.malformed_form_packages =
                    schema.stats.malformed_form_packages.saturating_add(1);
                break;
            }
        };
        let operation = &package.bytes[offset..end];
        let metadata = OpaqueOpcode {
            package_list_index: package.package_list_index,
            package_index: package.package_index,
            package_offset: package.package_offset.saturating_add(4 + offset as u32),
            opcode,
            length,
            scope,
            malformed: false,
        };

        if is_expression_opcode(opcode) {
            append_condition_expression(&mut stack, &metadata);
        } else if opcode != IFR_END && !is_condition_opcode(opcode) {
            close_condition_expressions(&mut stack);
        }

        let mut handled_scope = false;
        let parse_result = match opcode {
            IFR_FORM_SET => {
                handled_scope = true;
                parse_formset(schema, catalogue, package, operation, &metadata, scope, &mut stack)
            }
            IFR_FORM => {
                handled_scope = true;
                parse_form(schema, catalogue, package, operation, &metadata, scope, &mut stack)
            }
            IFR_VARSTORE | IFR_VARSTORE_EFI | IFR_VARSTORE_NAME_VALUE => {
                parse_varstore(schema, operation, opcode, &stack)
            }
            IFR_DEFAULTSTORE => parse_default_store(schema, catalogue, package, operation, &stack),
            IFR_ONE_OF | IFR_CHECKBOX | IFR_NUMERIC | IFR_STRING | IFR_ACTION => {
                handled_scope = true;
                parse_question(
                    schema,
                    catalogue,
                    package,
                    operation,
                    opcode,
                    &metadata,
                    scope,
                    &mut stack,
                )
            }
            IFR_ONE_OF_OPTION => {
                parse_option(schema, catalogue, package, operation, &stack)
            }
            IFR_DEFAULT => parse_default(schema, operation, &stack),
            IFR_SUPPRESS_IF | IFR_GRAY_OUT_IF | IFR_DISABLE_IF => {
                handled_scope = true;
                parse_condition(opcode, &metadata, scope, &mut stack)
            }
            IFR_END => {
                handled_scope = true;
                if operation.len() != 2 || stack.pop().is_none() {
                    Err("unbalanced END opcode")
                } else {
                    Ok(())
                }
            }
            _ => {
                record_unknown(schema, &stack, metadata.clone());
                Ok(())
            }
        };

        if parse_result.is_err() {
            schema.stats.malformed_opcodes = schema.stats.malformed_opcodes.saturating_add(1);
            let mut malformed = metadata.clone();
            malformed.malformed = true;
            record_opaque(schema, owner_from_stack(&stack), malformed);
        }

        if scope && !handled_scope && opcode != IFR_END {
            if stack.len() >= MAX_SCOPE_DEPTH {
                schema.stats.malformed_form_packages =
                    schema.stats.malformed_form_packages.saturating_add(1);
                break;
            }
            stack.push(ScopeFrame::Other);
        }
        offset = end;
    }

    if !stack.is_empty() {
        schema.stats.malformed_form_packages =
            schema.stats.malformed_form_packages.saturating_add(1);
    }
    if schema.stats.malformed_form_packages == malformed_before {
        schema.stats.parsed_form_packages = schema.stats.parsed_form_packages.saturating_add(1);
    }
}

fn parse_formset(
    schema: &mut BiosSchema,
    catalogue: &BiosCatalogue,
    package: &FormPackage,
    operation: &[u8],
    metadata: &OpaqueOpcode,
    scope: bool,
    stack: &mut Vec<ScopeFrame>,
) -> Result<(), &'static str> {
    if operation.len() < 23 || schema.formsets.len() >= MAX_FORMSETS {
        return Err("FORM_SET is short or formset limit reached");
    }
    let flags = operation[22];
    let class_count = usize::from(flags & 0x03);
    let required = 23usize
        .checked_add(class_count.checked_mul(16).ok_or("FORM_SET class overflow")?)
        .ok_or("FORM_SET class overflow")?;
    if required > operation.len() {
        return Err("FORM_SET class GUIDs are truncated");
    }
    let mut guid_bytes = [0u8; 16];
    guid_bytes.copy_from_slice(&operation[2..18]);
    let title_id = read_u16(operation, 18).map_err(|_| "FORM_SET title is truncated")?;
    let help_id = read_u16(operation, 20).map_err(|_| "FORM_SET help is truncated")?;
    let mut class_guids = Vec::with_capacity(class_count);
    for index in 0..class_count {
        let start = 23 + index * 16;
        let mut class_bytes = [0u8; 16];
        class_bytes.copy_from_slice(&operation[start..start + 16]);
        class_guids.push(EfiGuid::from_uefi_bytes(class_bytes));
    }
    let title = resolve(catalogue, package.package_list_index, title_id, &mut schema.stats);
    let help = resolve(catalogue, package.package_list_index, help_id, &mut schema.stats);
    let formset_index = schema.formsets.len();
    schema.formsets.push(FormSet {
        package_list_index: package.package_list_index,
        package_index: package.package_index,
        package_offset: metadata.package_offset,
        guid: EfiGuid::from_uefi_bytes(guid_bytes),
        title_id,
        title,
        help_id,
        help,
        class_guids,
        forms: Vec::new(),
        varstores: Vec::new(),
        default_stores: Vec::new(),
        opaque: Vec::new(),
    });
    if !scope || stack.len() >= MAX_SCOPE_DEPTH {
        return Err("FORM_SET has no usable scope");
    }
    stack.push(ScopeFrame::FormSet(formset_index));
    Ok(())
}

fn parse_form(
    schema: &mut BiosSchema,
    catalogue: &BiosCatalogue,
    package: &FormPackage,
    operation: &[u8],
    metadata: &OpaqueOpcode,
    scope: bool,
    stack: &mut Vec<ScopeFrame>,
) -> Result<(), &'static str> {
    if operation.len() < 6 {
        return Err("FORM is truncated");
    }
    let Some(formset_index) = current_formset(stack) else {
        schema.stats.orphan_forms = schema.stats.orphan_forms.saturating_add(1);
        return Err("FORM has no enclosing FORM_SET");
    };
    if schema.stats.forms as usize >= MAX_FORMS {
        return Err("form limit reached");
    }
    let form_id = read_u16(operation, 2).map_err(|_| "FORM id is truncated")?;
    let title_id = read_u16(operation, 4).map_err(|_| "FORM title is truncated")?;
    let title = resolve(catalogue, package.package_list_index, title_id, &mut schema.stats);
    let formset = schema
        .formsets
        .get_mut(formset_index)
        .ok_or("FORM_SET index is invalid")?;
    let form_index = formset.forms.len();
    formset.forms.push(Form {
        form_id,
        title_id,
        title,
        package_offset: metadata.package_offset,
        questions: Vec::new(),
        opaque: Vec::new(),
    });
    schema.stats.forms = schema.stats.forms.saturating_add(1);
    if !scope || stack.len() >= MAX_SCOPE_DEPTH {
        return Err("FORM has no usable scope");
    }
    stack.push(ScopeFrame::Form {
        formset: formset_index,
        form: form_index,
    });
    Ok(())
}

fn parse_varstore(
    schema: &mut BiosSchema,
    operation: &[u8],
    opcode: u8,
    stack: &[ScopeFrame],
) -> Result<(), &'static str> {
    let Some(formset_index) = current_formset(stack) else {
        schema.stats.orphan_varstores = schema.stats.orphan_varstores.saturating_add(1);
        return Err("VARSTORE has no enclosing FORM_SET");
    };
    let store = match opcode {
        IFR_VARSTORE => {
            if operation.len() < 23 {
                return Err("VARSTORE is truncated");
            }
            let mut guid_bytes = [0u8; 16];
            guid_bytes.copy_from_slice(&operation[2..18]);
            VarStore {
                id: read_u16(operation, 18).map_err(|_| "VARSTORE id is truncated")?,
                backend: StorageBackend::Buffer,
                guid: EfiGuid::from_uefi_bytes(guid_bytes),
                name: Some(read_ascii_name(operation, 22)?),
                size: Some(read_u16(operation, 20).map_err(|_| "VARSTORE size is truncated")?),
                attributes: None,
            }
        }
        IFR_VARSTORE_EFI => {
            if operation.len() < 27 {
                return Err("VARSTORE_EFI is truncated");
            }
            let mut guid_bytes = [0u8; 16];
            guid_bytes.copy_from_slice(&operation[4..20]);
            VarStore {
                id: read_u16(operation, 2).map_err(|_| "VARSTORE_EFI id is truncated")?,
                backend: StorageBackend::Efi,
                guid: EfiGuid::from_uefi_bytes(guid_bytes),
                name: Some(read_ascii_name(operation, 26)?),
                size: Some(
                    read_u16(operation, 24).map_err(|_| "VARSTORE_EFI size is truncated")?,
                ),
                attributes: Some(
                    read_u32(operation, 20)
                        .map_err(|_| "VARSTORE_EFI attributes are truncated")?,
                ),
            }
        }
        IFR_VARSTORE_NAME_VALUE => {
            if operation.len() < 20 {
                return Err("VARSTORE_NAME_VALUE is truncated");
            }
            let mut guid_bytes = [0u8; 16];
            guid_bytes.copy_from_slice(&operation[4..20]);
            VarStore {
                id: read_u16(operation, 2)
                    .map_err(|_| "VARSTORE_NAME_VALUE id is truncated")?,
                backend: StorageBackend::NameValue,
                guid: EfiGuid::from_uefi_bytes(guid_bytes),
                name: None,
                size: None,
                attributes: None,
            }
        }
        _ => return Err("not a supported VARSTORE opcode"),
    };
    let duplicate = {
        let formset = schema
            .formsets
            .get_mut(formset_index)
            .ok_or("VARSTORE FORM_SET index is invalid")?;
        if formset.varstores.len() >= MAX_VARSTORES_PER_FORMSET {
            return Err("varstore limit reached");
        }
        let duplicate = formset
            .varstores
            .iter()
            .any(|existing| existing.id == store.id);
        formset.varstores.push(store);
        duplicate
    };
    if duplicate {
        schema.stats.duplicate_varstores = schema.stats.duplicate_varstores.saturating_add(1);
    }
    Ok(())
}

fn parse_default_store(
    schema: &mut BiosSchema,
    catalogue: &BiosCatalogue,
    package: &FormPackage,
    operation: &[u8],
    stack: &[ScopeFrame],
) -> Result<(), &'static str> {
    if operation.len() < 6 {
        return Err("DEFAULTSTORE is truncated");
    }
    let Some(formset_index) = current_formset(stack) else {
        return Err("DEFAULTSTORE has no enclosing FORM_SET");
    };
    let name_id = read_u16(operation, 2).map_err(|_| "DEFAULTSTORE name is truncated")?;
    let id = read_u16(operation, 4).map_err(|_| "DEFAULTSTORE id is truncated")?;
    let name = resolve(catalogue, package.package_list_index, name_id, &mut schema.stats);
    let formset = schema
        .formsets
        .get_mut(formset_index)
        .ok_or("DEFAULTSTORE FORM_SET index is invalid")?;
    if formset.default_stores.len() >= MAX_DEFAULT_STORES_PER_FORMSET {
        return Err("default-store limit reached");
    }
    formset.default_stores.push(DefaultStore { id, name_id, name });
    Ok(())
}

fn parse_question(
    schema: &mut BiosSchema,
    catalogue: &BiosCatalogue,
    package: &FormPackage,
    operation: &[u8],
    opcode: u8,
    metadata: &OpaqueOpcode,
    scope: bool,
    stack: &mut Vec<ScopeFrame>,
) -> Result<(), &'static str> {
    if operation.len() < 13 {
        return Err("question header is truncated");
    }
    let Some((formset_index, form_index)) = current_form(stack) else {
        schema.stats.orphan_questions = schema.stats.orphan_questions.saturating_add(1);
        return Err("question has no enclosing FORM");
    };
    if schema.stats.questions as usize >= MAX_QUESTIONS {
        return Err("question limit reached");
    }

    let prompt_id = read_u16(operation, 2).map_err(|_| "question prompt is truncated")?;
    let help_id = read_u16(operation, 4).map_err(|_| "question help is truncated")?;
    let question_id = read_u16(operation, 6).map_err(|_| "question id is truncated")?;
    let varstore_id = read_u16(operation, 8).map_err(|_| "question varstore is truncated")?;
    let varstore_info = read_u16(operation, 10).map_err(|_| "question storage info is truncated")?;
    let question_flags = operation[12];

    let mut question = Question {
        package_offset: metadata.package_offset,
        prompt_id,
        prompt: resolve(catalogue, package.package_list_index, prompt_id, &mut schema.stats),
        help_id,
        help: resolve(catalogue, package.package_list_index, help_id, &mut schema.stats),
        question_id,
        kind: QuestionKind::Action,
        varstore_id,
        varstore_info,
        width: None,
        minimum: None,
        maximum: None,
        step: None,
        min_chars: None,
        max_chars: None,
        options: Vec::new(),
        defaults: Vec::new(),
        policy: QuestionPolicy {
            read_only: question_flags & QUESTION_FLAG_READ_ONLY != 0,
            callback: question_flags & QUESTION_FLAG_CALLBACK != 0,
            reset_required: question_flags & QUESTION_FLAG_RESET_REQUIRED != 0,
            reconnect_required: question_flags & QUESTION_FLAG_RECONNECT_REQUIRED != 0,
        },
        storage: QuestionStorage {
            backend: StorageBackend::Unknown,
            varstore_id,
            variable: None,
            variable_guid: None,
            offset: None,
            width: None,
            attributes: None,
            valid: false,
            reason: Some("storage-not-finalized"),
        },
        conditions: active_conditions(stack),
        opaque: Vec::new(),
    };

    match opcode {
        IFR_ONE_OF | IFR_NUMERIC => {
            if operation.len() < 14 {
                return Err("numeric question flags are truncated");
            }
            let numeric_flags = operation[13];
            let width = numeric_width(numeric_flags);
            let required = 14usize
                .checked_add(3usize.checked_mul(width).ok_or("numeric width overflow")?)
                .ok_or("numeric width overflow")?;
            if required > operation.len() {
                return Err("numeric min/max/step data are truncated");
            }
            question.kind = if opcode == IFR_ONE_OF {
                QuestionKind::OneOf
            } else {
                QuestionKind::Numeric
            };
            question.width = Some(width as u16);
            question.minimum = Some(read_uint(operation, 14, width)?);
            question.maximum = Some(read_uint(operation, 14 + width, width)?);
            question.step = Some(read_uint(operation, 14 + width * 2, width)?);
        }
        IFR_CHECKBOX => {
            if operation.len() < 14 {
                return Err("CHECKBOX flags are truncated");
            }
            question.kind = QuestionKind::Checkbox;
            question.width = Some(1);
            let checkbox_flags = operation[13];
            if checkbox_flags & CHECKBOX_DEFAULT != 0 {
                push_default(
                    &mut question,
                    QuestionDefault {
                        store_id: DEFAULT_STANDARD,
                        value: Some(boolean_value(true)),
                        expression: false,
                        source: DefaultSource::CheckboxFlag,
                    },
                )?;
            }
            if checkbox_flags & CHECKBOX_DEFAULT_MFG != 0 {
                push_default(
                    &mut question,
                    QuestionDefault {
                        store_id: DEFAULT_MANUFACTURING,
                        value: Some(boolean_value(true)),
                        expression: false,
                        source: DefaultSource::CheckboxFlag,
                    },
                )?;
            }
        }
        IFR_STRING => {
            if operation.len() < 16 {
                return Err("STRING limits are truncated");
            }
            question.kind = QuestionKind::String;
            question.min_chars = Some(operation[13] as u16);
            question.max_chars = Some(operation[14] as u16);
            question.width = (operation[14] as u16).checked_mul(2);
            if question.width.is_none() {
                return Err("STRING width overflow");
            }
        }
        IFR_ACTION => {
            question.kind = QuestionKind::Action;
        }
        _ => return Err("unsupported question opcode"),
    }

    let formset = schema
        .formsets
        .get_mut(formset_index)
        .ok_or("question FORM_SET index is invalid")?;
    let form = formset
        .forms
        .get_mut(form_index)
        .ok_or("question FORM index is invalid")?;
    let question_index = form.questions.len();
    form.questions.push(question);
    schema.stats.questions = schema.stats.questions.saturating_add(1);

    if scope {
        if stack.len() >= MAX_SCOPE_DEPTH {
            return Err("question scope depth exceeds bound");
        }
        stack.push(ScopeFrame::Question {
            formset: formset_index,
            form: form_index,
            question: question_index,
        });
    }
    Ok(())
}

fn parse_option(
    schema: &mut BiosSchema,
    catalogue: &BiosCatalogue,
    package: &FormPackage,
    operation: &[u8],
    stack: &[ScopeFrame],
) -> Result<(), &'static str> {
    if operation.len() < 6 {
        return Err("ONE_OF_OPTION is truncated");
    }
    let Some((formset_index, form_index, question_index)) = current_question(stack) else {
        schema.stats.orphan_options = schema.stats.orphan_options.saturating_add(1);
        return Err("ONE_OF_OPTION has no enclosing question");
    };
    let label_id = read_u16(operation, 2).map_err(|_| "option label is truncated")?;
    let flags = operation[4];
    let value = parse_value(operation, 5)?;
    let label = resolve(catalogue, package.package_list_index, label_id, &mut schema.stats);
    let question = schema
        .formsets
        .get_mut(formset_index)
        .and_then(|formset| formset.forms.get_mut(form_index))
        .and_then(|form| form.questions.get_mut(question_index))
        .ok_or("option question index is invalid")?;
    if question.kind != QuestionKind::OneOf {
        return Err("ONE_OF_OPTION parent is not ONE_OF");
    }
    if question.options.len() >= MAX_OPTIONS_PER_QUESTION {
        return Err("option limit reached");
    }
    question.options.push(QuestionOption {
        label_id,
        label,
        flags,
        value: value.clone(),
    });
    if flags & OPTION_DEFAULT != 0 {
        push_default(
            question,
            QuestionDefault {
                store_id: DEFAULT_STANDARD,
                value: Some(value.clone()),
                expression: false,
                source: DefaultSource::OptionFlag,
            },
        )?;
    }
    if flags & OPTION_DEFAULT_MFG != 0 {
        push_default(
            question,
            QuestionDefault {
                store_id: DEFAULT_MANUFACTURING,
                value: Some(value),
                expression: false,
                source: DefaultSource::OptionFlag,
            },
        )?;
    }
    Ok(())
}

fn parse_default(
    schema: &mut BiosSchema,
    operation: &[u8],
    stack: &[ScopeFrame],
) -> Result<(), &'static str> {
    if operation.len() < 5 {
        return Err("DEFAULT is truncated");
    }
    let Some((formset_index, form_index, question_index)) = current_question(stack) else {
        schema.stats.orphan_defaults = schema.stats.orphan_defaults.saturating_add(1);
        return Err("DEFAULT has no enclosing question");
    };
    let store_id = read_u16(operation, 2).map_err(|_| "DEFAULT id is truncated")?;
    let parsed = parse_value(operation, 4);
    let (value, expression) = match parsed {
        Ok(value) => (Some(value), false),
        Err(_) => (None, true),
    };
    let question = schema
        .formsets
        .get_mut(formset_index)
        .and_then(|formset| formset.forms.get_mut(form_index))
        .and_then(|form| form.questions.get_mut(question_index))
        .ok_or("default question index is invalid")?;
    push_default(
        question,
        QuestionDefault {
            store_id,
            value,
            expression,
            source: DefaultSource::Opcode,
        },
    )
}

fn parse_condition(
    opcode: u8,
    metadata: &OpaqueOpcode,
    scope: bool,
    stack: &mut Vec<ScopeFrame>,
) -> Result<(), &'static str> {
    if !scope || stack.len() >= MAX_SCOPE_DEPTH {
        return Err("condition has no usable scope");
    }
    let kind = match opcode {
        IFR_SUPPRESS_IF => ConditionKind::Suppress,
        IFR_GRAY_OUT_IF => ConditionKind::GrayOut,
        IFR_DISABLE_IF => ConditionKind::Disable,
        _ => return Err("unsupported condition opcode"),
    };
    stack.push(ScopeFrame::Condition(ConditionFrame {
        condition: VisibilityCondition {
            kind,
            package_offset: metadata.package_offset,
            expression: Vec::new(),
            expression_truncated: false,
        },
        expression_open: true,
    }));
    Ok(())
}

fn finalize_storage(schema: &mut BiosSchema, catalogue: &BiosCatalogue) {
    for formset_index in 0..schema.formsets.len() {
        let package_list_index = schema.formsets[formset_index].package_list_index;
        let varstores = schema.formsets[formset_index].varstores.clone();
        for form in &mut schema.formsets[formset_index].forms {
            for question in &mut form.questions {
                question.storage = resolve_storage(
                    question,
                    &varstores,
                    catalogue,
                    package_list_index,
                );
            }
        }
    }
}

fn resolve_storage(
    question: &Question,
    varstores: &[VarStore],
    catalogue: &BiosCatalogue,
    package_list_index: usize,
) -> QuestionStorage {
    let mut storage = QuestionStorage {
        backend: StorageBackend::None,
        varstore_id: question.varstore_id,
        variable: None,
        variable_guid: None,
        offset: None,
        width: question.width,
        attributes: None,
        valid: false,
        reason: None,
    };
    if question.varstore_id == 0 {
        storage.reason = Some("question-has-no-varstore");
        return storage;
    }
    let mut matches = varstores
        .iter()
        .filter(|varstore| varstore.id == question.varstore_id);
    let Some(varstore) = matches.next() else {
        storage.backend = StorageBackend::Unknown;
        storage.reason = Some("varstore-not-found");
        return storage;
    };
    if matches.next().is_some() {
        storage.backend = StorageBackend::Unknown;
        storage.reason = Some("varstore-id-is-ambiguous");
        return storage;
    }

    storage.backend = varstore.backend;
    storage.variable_guid = Some(varstore.guid);
    storage.attributes = varstore.attributes;
    match varstore.backend {
        StorageBackend::Buffer | StorageBackend::Efi => {
            storage.variable = varstore.name.clone();
            storage.offset = Some(question.varstore_info);
            let Some(width) = question.width else {
                storage.reason = Some("question-width-is-unknown");
                return storage;
            };
            let Some(size) = varstore.size else {
                storage.reason = Some("varstore-size-is-unknown");
                return storage;
            };
            let Some(end) = question.varstore_info.checked_add(width) else {
                storage.reason = Some("question-storage-range-overflow");
                return storage;
            };
            if end > size {
                storage.reason = Some("question-storage-range-exceeds-varstore");
                return storage;
            }
            if storage.variable.is_none() {
                storage.reason = Some("varstore-name-is-missing");
                return storage;
            }
            storage.valid = true;
        }
        StorageBackend::NameValue => {
            storage.variable = catalogue
                .resolve_string(package_list_index, question.varstore_info)
                .map(String::from);
            if storage.variable.is_none() {
                storage.reason = Some("name-value-key-is-unresolved");
                return storage;
            }
            storage.valid = true;
        }
        StorageBackend::None | StorageBackend::Unknown => {
            storage.reason = Some("unsupported-varstore-backend");
        }
    }
    storage
}

fn parse_value(operation: &[u8], type_offset: usize) -> Result<IfrValue, &'static str> {
    let type_code = *operation
        .get(type_offset)
        .ok_or("IFR value type is truncated")?;
    let value_offset = type_offset + 1;
    let Some(width) = value_width(type_code) else {
        return Ok(IfrValue {
            type_code,
            unsigned: None,
            string_id: None,
            encoded_width: operation.len().saturating_sub(value_offset).min(255) as u8,
        });
    };
    let end = value_offset
        .checked_add(width)
        .ok_or("IFR value range overflow")?;
    if end > operation.len() {
        return Err("IFR value is truncated");
    }
    let unsigned = if width <= 8 {
        Some(read_uint(operation, value_offset, width)?)
    } else {
        None
    };
    let string_id = if matches!(type_code, 0x07 | 0x0a) {
        Some(read_u16(operation, value_offset).map_err(|_| "string value is truncated")?)
    } else {
        None
    };
    Ok(IfrValue {
        type_code,
        unsigned,
        string_id,
        encoded_width: width.min(255) as u8,
    })
}

fn value_width(type_code: u8) -> Option<usize> {
    match type_code {
        0x00 => Some(1),
        0x01 => Some(2),
        0x02 => Some(4),
        0x03 => Some(8),
        0x04 => Some(1),
        0x05 => Some(3),
        0x06 => Some(4),
        0x07 => Some(2),
        0x0a => Some(2),
        0x0c => Some(22),
        _ => None,
    }
}

fn boolean_value(value: bool) -> IfrValue {
    IfrValue {
        type_code: 0x04,
        unsigned: Some(if value { 1 } else { 0 }),
        string_id: None,
        encoded_width: 1,
    }
}

fn push_default(
    question: &mut Question,
    value: QuestionDefault,
) -> Result<(), &'static str> {
    if question.defaults.len() >= MAX_DEFAULTS_PER_QUESTION {
        return Err("default limit reached");
    }
    question.defaults.push(value);
    Ok(())
}

fn numeric_width(flags: u8) -> usize {
    1usize << usize::from(flags & 0x03)
}

fn read_uint(bytes: &[u8], offset: usize, width: usize) -> Result<u64, &'static str> {
    if !matches!(width, 1 | 2 | 3 | 4 | 8) {
        return Err("unsupported integer width");
    }
    let end = offset.checked_add(width).ok_or("integer range overflow")?;
    let slice = bytes.get(offset..end).ok_or("integer is truncated")?;
    let mut value = 0u64;
    for (shift, byte) in slice.iter().enumerate() {
        value |= u64::from(*byte) << (shift * 8);
    }
    Ok(value)
}

fn resolve(
    catalogue: &BiosCatalogue,
    package_list_index: usize,
    id: u16,
    stats: &mut SchemaStats,
) -> Option<String> {
    let value = catalogue
        .resolve_string(package_list_index, id)
        .map(String::from);
    if value.is_some() {
        stats.resolved_strings = stats.resolved_strings.saturating_add(1);
    }
    value
}

fn active_conditions(stack: &[ScopeFrame]) -> Vec<VisibilityCondition> {
    let mut conditions = Vec::new();
    for frame in stack {
        if let ScopeFrame::Condition(condition) = frame {
            if conditions.len() >= MAX_CONDITIONS_PER_QUESTION {
                break;
            }
            conditions.push(condition.condition.clone());
        }
    }
    conditions
}

fn append_condition_expression(stack: &mut [ScopeFrame], opcode: &OpaqueOpcode) {
    for frame in stack {
        if let ScopeFrame::Condition(condition) = frame {
            if !condition.expression_open {
                continue;
            }
            if condition.condition.expression.len() >= MAX_EXPRESSION_OPS {
                condition.condition.expression_truncated = true;
            } else {
                condition.condition.expression.push(opcode.clone());
            }
        }
    }
}

fn close_condition_expressions(stack: &mut [ScopeFrame]) {
    for frame in stack {
        if let ScopeFrame::Condition(condition) = frame {
            condition.expression_open = false;
        }
    }
}

fn current_formset(stack: &[ScopeFrame]) -> Option<usize> {
    stack.iter().rev().find_map(|frame| match frame {
        ScopeFrame::FormSet(formset) => Some(*formset),
        ScopeFrame::Form { formset, .. } | ScopeFrame::Question { formset, .. } => Some(*formset),
        ScopeFrame::Condition(_) | ScopeFrame::Other => None,
    })
}

fn current_form(stack: &[ScopeFrame]) -> Option<(usize, usize)> {
    stack.iter().rev().find_map(|frame| match frame {
        ScopeFrame::Form { formset, form } | ScopeFrame::Question { formset, form, .. } => {
            Some((*formset, *form))
        }
        ScopeFrame::FormSet(_) | ScopeFrame::Condition(_) | ScopeFrame::Other => None,
    })
}

fn current_question(stack: &[ScopeFrame]) -> Option<(usize, usize, usize)> {
    stack.iter().rev().find_map(|frame| match frame {
        ScopeFrame::Question {
            formset,
            form,
            question,
        } => Some((*formset, *form, *question)),
        _ => None,
    })
}

fn owner_from_stack(stack: &[ScopeFrame]) -> Owner {
    for frame in stack.iter().rev() {
        match frame {
            ScopeFrame::Question {
                formset,
                form,
                question,
            } => return Owner::Question(*formset, *form, *question),
            ScopeFrame::Form { formset, form } => return Owner::Form(*formset, *form),
            ScopeFrame::FormSet(formset) => return Owner::FormSet(*formset),
            ScopeFrame::Condition(_) | ScopeFrame::Other => {}
        }
    }
    Owner::Global
}

fn record_unknown(schema: &mut BiosSchema, stack: &[ScopeFrame], opcode: OpaqueOpcode) {
    let count = schema.unknown_opcodes.entry(opcode.opcode).or_insert(0);
    *count = count.saturating_add(1);
    schema.stats.unknown_opcode_instances =
        schema.stats.unknown_opcode_instances.saturating_add(1);
    record_opaque(schema, owner_from_stack(stack), opcode);
}

fn record_opaque(schema: &mut BiosSchema, owner: Owner, opcode: OpaqueOpcode) {
    if schema.opaque_opcodes.len() < MAX_OPAQUE_GLOBAL {
        schema.opaque_opcodes.push(opcode.clone());
    } else {
        schema.stats.truncated_metadata = schema.stats.truncated_metadata.saturating_add(1);
    }
    let target = match owner {
        Owner::Global => return,
        Owner::FormSet(formset) => schema
            .formsets
            .get_mut(formset)
            .map(|formset| &mut formset.opaque),
        Owner::Form(formset, form) => schema
            .formsets
            .get_mut(formset)
            .and_then(|formset| formset.forms.get_mut(form))
            .map(|form| &mut form.opaque),
        Owner::Question(formset, form, question) => schema
            .formsets
            .get_mut(formset)
            .and_then(|formset| formset.forms.get_mut(form))
            .and_then(|form| form.questions.get_mut(question))
            .map(|question| &mut question.opaque),
    };
    let mut owner_truncated = false;
    if let Some(target) = target {
        if target.len() < MAX_OPAQUE_PER_OWNER {
            target.push(opcode);
        } else {
            owner_truncated = true;
        }
    }
    if owner_truncated {
        schema.stats.truncated_metadata = schema.stats.truncated_metadata.saturating_add(1);
    }
}

fn is_condition_opcode(opcode: u8) -> bool {
    matches!(opcode, IFR_SUPPRESS_IF | IFR_GRAY_OUT_IF | IFR_DISABLE_IF)
}

fn is_expression_opcode(opcode: u8) -> bool {
    matches!(
        opcode,
        0x12..=0x17
            | 0x20..=0x22
            | 0x28
            | 0x2a..=0x5a
            | 0x5e
            | 0x60
            | 0x64
    )
}

fn read_ascii_name(bytes: &[u8], start: usize) -> Result<String, &'static str> {
    let tail = bytes.get(start..).ok_or("varstore name starts outside opcode")?;
    let end = tail
        .iter()
        .position(|byte| *byte == 0)
        .ok_or("varstore name is not terminated")?;
    if end == 0 || end > MAX_VARSTORE_NAME_BYTES {
        return Err("varstore name is empty or too long");
    }
    let text = core::str::from_utf8(&tail[..end]).map_err(|_| "varstore name is not UTF-8")?;
    Ok(String::from(text))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, ()> {
    let slice = bytes.get(offset..offset.saturating_add(4)).ok_or(())?;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, ()> {
    let slice = bytes.get(offset..offset.saturating_add(2)).ok_or(())?;
    Ok(u16::from_le_bytes([slice[0], slice[1]]))
}
