use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use crate::efi::EfiGuid;

use super::bios_hii::{self, HiiIndex};

const MAX_IFR_OPS: usize = 65_536;
const MAX_SCOPE_DEPTH: usize = 96;
const MAX_FORMSETS: usize = 256;
const MAX_FORMS: usize = 4096;
const MAX_VARSTORES: usize = 2048;
const MAX_DEFAULT_STORES: usize = 256;
const MAX_QUESTIONS: usize = 16_384;
const MAX_OPTIONS: usize = 65_536;
const MAX_DEFAULTS: usize = 32_768;
const MAX_DIAGNOSTICS: usize = 64;

const OP_FORM: u8 = 0x01;
const OP_ONE_OF: u8 = 0x05;
const OP_CHECKBOX: u8 = 0x06;
const OP_NUMERIC: u8 = 0x07;
const OP_PASSWORD: u8 = 0x08;
const OP_ONE_OF_OPTION: u8 = 0x09;
const OP_SUPPRESS_IF: u8 = 0x0a;
const OP_LOCKED: u8 = 0x0b;
const OP_ACTION: u8 = 0x0c;
const OP_FORM_SET: u8 = 0x0e;
const OP_REF: u8 = 0x0f;
const OP_NO_SUBMIT_IF: u8 = 0x10;
const OP_INCONSISTENT_IF: u8 = 0x11;
const OP_GRAY_OUT_IF: u8 = 0x19;
const OP_DATE: u8 = 0x1a;
const OP_TIME: u8 = 0x1b;
const OP_STRING: u8 = 0x1c;
const OP_DISABLE_IF: u8 = 0x1e;
const OP_ORDERED_LIST: u8 = 0x23;
const OP_VARSTORE: u8 = 0x24;
const OP_VARSTORE_NAME_VALUE: u8 = 0x25;
const OP_VARSTORE_EFI: u8 = 0x26;
const OP_VARSTORE_DEVICE: u8 = 0x27;
const OP_END: u8 = 0x29;
const OP_DEFAULT: u8 = 0x5b;
const OP_DEFAULTSTORE: u8 = 0x5c;
const OP_WARNING_IF: u8 = 0x63;

pub(crate) const CONDITION_SUPPRESS: u8 = 1 << 0;
pub(crate) const CONDITION_GRAY_OUT: u8 = 1 << 1;
pub(crate) const CONDITION_DISABLE: u8 = 1 << 2;
pub(crate) const CONDITION_LOCKED: u8 = 1 << 3;

const QUESTION_READ_ONLY: u8 = 0x01;
const QUESTION_CALLBACK: u8 = 0x04;
const QUESTION_RESET_REQUIRED: u8 = 0x10;
const QUESTION_RECONNECT_REQUIRED: u8 = 0x40;
const OPTION_DEFAULT: u8 = 0x10;
const OPTION_DEFAULT_MFG: u8 = 0x20;
const CHECKBOX_DEFAULT: u8 = 0x01;
const CHECKBOX_DEFAULT_MFG: u8 = 0x02;

const TYPE_U8: u8 = 0x00;
const TYPE_U16: u8 = 0x01;
const TYPE_U32: u8 = 0x02;
const TYPE_U64: u8 = 0x03;
const TYPE_BOOLEAN: u8 = 0x04;
const TYPE_TIME: u8 = 0x05;
const TYPE_DATE: u8 = 0x06;
const TYPE_STRING: u8 = 0x07;
const TYPE_ACTION: u8 = 0x0a;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum QuestionKind {
    OneOf,
    Checkbox,
    Numeric,
    Password,
    Action,
    Reference,
    Date,
    Time,
    String,
    OrderedList,
}

impl QuestionKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::OneOf => "one-of",
            Self::Checkbox => "checkbox",
            Self::Numeric => "numeric",
            Self::Password => "password",
            Self::Action => "action",
            Self::Reference => "reference",
            Self::Date => "date",
            Self::Time => "time",
            Self::String => "string",
            Self::OrderedList => "ordered-list",
        }
    }
}

#[derive(Clone)]
pub(crate) enum TypedValue {
    Unsigned(u64),
    Boolean(bool),
    StringId(u16),
    Time { hour: u8, minute: u8, second: u8 },
    Date { year: u16, month: u8, day: u8 },
    Opaque { type_code: u8, bytes: Vec<u8> },
    Expression { type_code: u8 },
}

pub(crate) struct QuestionOption {
    pub text_id: u16,
    pub flags: u8,
    pub type_code: u8,
    pub value: TypedValue,
}

pub(crate) struct QuestionDefault {
    pub default_id: u16,
    pub source: &'static str,
    pub value: TypedValue,
}

pub(crate) struct FormSet {
    pub list_index: usize,
    pub package_index: u32,
    pub opcode_offset: u32,
    pub guid: EfiGuid,
    pub title_id: u16,
    pub help_id: u16,
    pub flags: u8,
    pub class_guids: Vec<EfiGuid>,
    pub forms: Vec<usize>,
    pub varstores: Vec<usize>,
}

pub(crate) struct Form {
    pub list_index: usize,
    pub formset_index: Option<usize>,
    pub opcode_offset: u32,
    pub id: u16,
    pub title_id: u16,
    pub conditions: u8,
    pub questions: Vec<usize>,
}

pub(crate) enum VarStoreKind {
    Buffer {
        guid: EfiGuid,
        size: u16,
        name: String,
    },
    EfiVariable {
        guid: EfiGuid,
        attributes: u32,
        size: u16,
        name: String,
    },
    NameValue {
        guid: EfiGuid,
    },
}

pub(crate) struct VarStore {
    pub list_index: usize,
    pub formset_index: Option<usize>,
    pub opcode_offset: u32,
    pub id: u16,
    pub kind: VarStoreKind,
    pub device_path_id: Option<u16>,
}

pub(crate) struct DefaultStore {
    pub list_index: usize,
    pub formset_index: Option<usize>,
    pub opcode_offset: u32,
    pub id: u16,
    pub name_id: u16,
}

pub(crate) struct NumericRange {
    pub width: u8,
    pub minimum: u64,
    pub maximum: u64,
    pub step: u64,
}

pub(crate) struct Question {
    pub list_index: usize,
    pub formset_index: Option<usize>,
    pub form_index: Option<usize>,
    pub opcode_offset: u32,
    pub id: u16,
    pub kind: QuestionKind,
    pub prompt_id: u16,
    pub help_id: u16,
    pub varstore_id: u16,
    pub varstore_info: u16,
    pub flags: u8,
    pub kind_flags: u8,
    pub conditions: u8,
    pub numeric: Option<NumericRange>,
    pub min_size: Option<u16>,
    pub max_size: Option<u16>,
    pub max_containers: Option<u8>,
    pub options: Vec<QuestionOption>,
    pub defaults: Vec<QuestionDefault>,
    pub validation_constraints: u16,
}

impl Question {
    pub fn read_only(&self) -> bool {
        self.flags & QUESTION_READ_ONLY != 0
    }

    pub fn callback(&self) -> bool {
        self.flags & QUESTION_CALLBACK != 0
    }

    pub fn reset_required(&self) -> bool {
        self.flags & QUESTION_RESET_REQUIRED != 0
    }

    pub fn reconnect_required(&self) -> bool {
        self.flags & QUESTION_RECONNECT_REQUIRED != 0
    }

    pub fn storage_width(&self) -> Option<u8> {
        if let Some(numeric) = self.numeric.as_ref() {
            return Some(numeric.width);
        }
        match self.kind {
            QuestionKind::Checkbox => Some(1),
            QuestionKind::Date => Some(4),
            QuestionKind::Time => Some(3),
            _ => None,
        }
    }
}

#[derive(Default)]
pub(crate) struct IfrDiagnostics {
    pub form_package_errors: u32,
    pub unknown_opcodes: BTreeMap<u8, u32>,
    pub scope_underflows: u32,
    pub unclosed_scopes: u32,
    pub orphan_options: u32,
    pub orphan_defaults: u32,
    pub messages: Vec<String>,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct IfrStats {
    pub opcodes: u32,
    pub formsets: u32,
    pub forms: u32,
    pub varstores: u32,
    pub default_stores: u32,
    pub questions: u32,
    pub options: u32,
    pub defaults: u32,
}

pub(crate) struct FirmwareSchema {
    pub hii: HiiIndex,
    pub formsets: Vec<FormSet>,
    pub forms: Vec<Form>,
    pub varstores: Vec<VarStore>,
    pub default_stores: Vec<DefaultStore>,
    pub questions: Vec<Question>,
    pub stats: IfrStats,
    pub diagnostics: IfrDiagnostics,
}

impl FirmwareSchema {
    pub fn load() -> Result<Self, String> {
        Self::parse(bios_hii::load_hii_index()?)
    }

    pub fn parse(hii: HiiIndex) -> Result<Self, String> {
        let mut schema = Self {
            hii,
            formsets: Vec::new(),
            forms: Vec::new(),
            varstores: Vec::new(),
            default_stores: Vec::new(),
            questions: Vec::new(),
            stats: IfrStats::default(),
            diagnostics: IfrDiagnostics::default(),
        };

        let packages: Vec<(usize, u32, Vec<u8>)> = schema
            .hii
            .form_packages
            .iter()
            .map(|package| (package.list_index, package.package_index, package.bytes.clone()))
            .collect();
        for (list_index, package_index, bytes) in packages {
            let checkpoint = SchemaCheckpoint::capture(&schema);
            if let Err(error) = parse_form_package(&mut schema, list_index, package_index, &bytes) {
                checkpoint.restore(&mut schema);
                schema.diagnostics.form_package_errors = schema
                    .diagnostics
                    .form_package_errors
                    .saturating_add(1);
                push_diagnostic(
                    &mut schema.diagnostics,
                    alloc::format!(
                        "list={} package={} IFR parse: {}",
                        list_index,
                        package_index,
                        error
                    ),
                );
            }
        }
        if schema.formsets.is_empty() && schema.questions.is_empty() {
            return Err(String::from("no bounded IFR formset or question decoded"));
        }
        Ok(schema)
    }

    pub fn text(&self, list_index: usize, string_id: u16) -> Option<&str> {
        self.hii.resolve_string(list_index, string_id)
    }

    pub fn formset_title(&self, formset_index: usize) -> Option<&str> {
        let formset = self.formsets.get(formset_index)?;
        self.text(formset.list_index, formset.title_id)
    }

    pub fn form_title(&self, form_index: usize) -> Option<&str> {
        let form = self.forms.get(form_index)?;
        self.text(form.list_index, form.title_id)
    }

    pub fn question_prompt(&self, question_index: usize) -> Option<&str> {
        let question = self.questions.get(question_index)?;
        self.text(question.list_index, question.prompt_id)
    }

    pub fn question_help(&self, question_index: usize) -> Option<&str> {
        let question = self.questions.get(question_index)?;
        self.text(question.list_index, question.help_id)
    }

    pub fn resolve_varstore(&self, question: &Question) -> Option<&VarStore> {
        if question.varstore_id == 0 {
            return None;
        }
        self.varstores
            .iter()
            .find(|store| {
                store.id == question.varstore_id
                    && store.formset_index == question.formset_index
                    && store.list_index == question.list_index
            })
            .or_else(|| {
                self.varstores.iter().find(|store| {
                    store.id == question.varstore_id && store.list_index == question.list_index
                })
            })
    }

    pub fn default_store_name(&self, question: &Question, default_id: u16) -> Option<&str> {
        self.default_stores
            .iter()
            .find(|store| {
                store.id == default_id
                    && store.formset_index == question.formset_index
                    && store.list_index == question.list_index
            })
            .and_then(|store| self.text(store.list_index, store.name_id))
    }
}

#[derive(Clone, Copy)]
struct SchemaCheckpoint {
    formsets: usize,
    forms: usize,
    varstores: usize,
    default_stores: usize,
    questions: usize,
    stats: IfrStats,
}

impl SchemaCheckpoint {
    fn capture(schema: &FirmwareSchema) -> Self {
        Self {
            formsets: schema.formsets.len(),
            forms: schema.forms.len(),
            varstores: schema.varstores.len(),
            default_stores: schema.default_stores.len(),
            questions: schema.questions.len(),
            stats: schema.stats,
        }
    }

    fn restore(self, schema: &mut FirmwareSchema) {
        schema.formsets.truncate(self.formsets);
        schema.forms.truncate(self.forms);
        schema.varstores.truncate(self.varstores);
        schema.default_stores.truncate(self.default_stores);
        schema.questions.truncate(self.questions);
        schema.stats = self.stats;
    }
}

#[derive(Clone, Copy, Default)]
struct ParseContext {
    formset: Option<usize>,
    form: Option<usize>,
    question: Option<usize>,
    conditions: u8,
}

struct ScopeFrame {
    before: ParseContext,
    opcode: u8,
}

fn parse_form_package(
    schema: &mut FirmwareSchema,
    list_index: usize,
    package_index: u32,
    package: &[u8],
) -> Result<(), String> {
    const PACKAGE_HEADER_BYTES: usize = 4;
    if package.len() < PACKAGE_HEADER_BYTES {
        return Err(String::from("form package is shorter than its header"));
    }
    let raw = read_u32(package, 0)?;
    let encoded_len = (raw & 0x00ff_ffff) as usize;
    let package_type = (raw >> 24) as u8;
    if encoded_len != package.len() || package_type != 0x02 {
        return Err(String::from("form package header does not match captured range"));
    }

    let mut context = ParseContext::default();
    let mut scopes: Vec<ScopeFrame> = Vec::new();
    let mut offset = PACKAGE_HEADER_BYTES;
    let mut opcode_count = 0usize;

    while offset < package.len() {
        if opcode_count >= MAX_IFR_OPS {
            return Err(String::from("IFR opcode count exceeds bound"));
        }
        let opcode = *package
            .get(offset)
            .ok_or_else(|| String::from("IFR opcode byte is missing"))?;
        let length_scope = *package
            .get(offset + 1)
            .ok_or_else(|| String::from("IFR length byte is missing"))?;
        let length = usize::from(length_scope & 0x7f);
        let scoped = length_scope & 0x80 != 0;
        if length < 2 {
            return Err(alloc::format!(
                "IFR opcode 0x{:02X} has invalid length {}",
                opcode,
                length
            ));
        }
        let end = checked_end(offset, length, package.len(), "IFR opcode")?;
        let operation = &package[offset..end];
        opcode_count += 1;
        schema.stats.opcodes = schema.stats.opcodes.saturating_add(1);

        if opcode == OP_END {
            let Some(frame) = scopes.pop() else {
                schema.diagnostics.scope_underflows = schema
                    .diagnostics
                    .scope_underflows
                    .saturating_add(1);
                return Err(alloc::format!("IFR END underflow at offset 0x{:X}", offset));
            };
            let _closed_opcode = frame.opcode;
            context = frame.before;
            offset = end;
            continue;
        }

        let before = context;
        match opcode {
            OP_FORM_SET => parse_formset(
                schema,
                list_index,
                package_index,
                offset,
                operation,
                &mut context,
            )?,
            OP_FORM => parse_form(schema, list_index, offset, operation, &mut context)?,
            OP_VARSTORE => parse_buffer_varstore(schema, list_index, offset, operation, context)?,
            OP_VARSTORE_EFI => parse_efi_varstore(schema, list_index, offset, operation, context)?,
            OP_VARSTORE_NAME_VALUE => {
                parse_name_value_varstore(schema, list_index, offset, operation, context)?
            }
            OP_VARSTORE_DEVICE => attach_varstore_device(schema, operation, context)?,
            OP_DEFAULTSTORE => parse_default_store(schema, list_index, offset, operation, context)?,
            OP_ONE_OF => parse_question(
                schema,
                list_index,
                offset,
                operation,
                QuestionKind::OneOf,
                &mut context,
            )?,
            OP_CHECKBOX => parse_question(
                schema,
                list_index,
                offset,
                operation,
                QuestionKind::Checkbox,
                &mut context,
            )?,
            OP_NUMERIC => parse_question(
                schema,
                list_index,
                offset,
                operation,
                QuestionKind::Numeric,
                &mut context,
            )?,
            OP_PASSWORD => parse_question(
                schema,
                list_index,
                offset,
                operation,
                QuestionKind::Password,
                &mut context,
            )?,
            OP_ACTION => parse_question(
                schema,
                list_index,
                offset,
                operation,
                QuestionKind::Action,
                &mut context,
            )?,
            OP_REF => parse_question(
                schema,
                list_index,
                offset,
                operation,
                QuestionKind::Reference,
                &mut context,
            )?,
            OP_DATE => parse_question(
                schema,
                list_index,
                offset,
                operation,
                QuestionKind::Date,
                &mut context,
            )?,
            OP_TIME => parse_question(
                schema,
                list_index,
                offset,
                operation,
                QuestionKind::Time,
                &mut context,
            )?,
            OP_STRING => parse_question(
                schema,
                list_index,
                offset,
                operation,
                QuestionKind::String,
                &mut context,
            )?,
            OP_ORDERED_LIST => parse_question(
                schema,
                list_index,
                offset,
                operation,
                QuestionKind::OrderedList,
                &mut context,
            )?,
            OP_ONE_OF_OPTION => parse_option(schema, operation, context)?,
            OP_DEFAULT => parse_default(schema, operation, context, scoped)?,
            OP_SUPPRESS_IF => context.conditions |= CONDITION_SUPPRESS,
            OP_GRAY_OUT_IF => context.conditions |= CONDITION_GRAY_OUT,
            OP_DISABLE_IF => context.conditions |= CONDITION_DISABLE,
            OP_LOCKED => {
                if let Some(question) = context.question.and_then(|index| schema.questions.get_mut(index)) {
                    question.conditions |= CONDITION_LOCKED;
                } else if scoped {
                    context.conditions |= CONDITION_LOCKED;
                }
            }
            OP_NO_SUBMIT_IF | OP_INCONSISTENT_IF | OP_WARNING_IF => {
                if let Some(question) = context.question.and_then(|index| schema.questions.get_mut(index)) {
                    question.validation_constraints = question.validation_constraints.saturating_add(1);
                }
            }
            _ => {
                *schema.diagnostics.unknown_opcodes.entry(opcode).or_insert(0) += 1;
            }
        }

        if scoped {
            if scopes.len() >= MAX_SCOPE_DEPTH {
                return Err(String::from("IFR scope depth exceeds bound"));
            }
            scopes.push(ScopeFrame { before, opcode });
        } else {
            context = before;
        }
        offset = end;
    }

    if !scopes.is_empty() {
        schema.diagnostics.unclosed_scopes = schema
            .diagnostics
            .unclosed_scopes
            .saturating_add(scopes.len() as u32);
        return Err(alloc::format!("{} IFR scopes were not closed", scopes.len()));
    }
    Ok(())
}

fn parse_formset(
    schema: &mut FirmwareSchema,
    list_index: usize,
    package_index: u32,
    opcode_offset: usize,
    bytes: &[u8],
    context: &mut ParseContext,
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
    let flags = bytes[22];
    let class_guid_count = usize::from(flags & 0x03);
    let expected_bytes = 23usize
        .checked_add(
            class_guid_count
                .checked_mul(16)
                .ok_or_else(|| String::from("FORM_SET class GUID count overflow"))?,
        )
        .ok_or_else(|| String::from("FORM_SET length overflow"))?;
    if bytes.len() != expected_bytes {
        return Err(alloc::format!(
            "FORM_SET length={} does not match class_guid_count={} expected={}",
            bytes.len(),
            class_guid_count,
            expected_bytes
        ));
    }
    let mut class_guids = Vec::with_capacity(class_guid_count);
    for index in 0..class_guid_count {
        class_guids.push(read_guid(bytes, 23 + index * 16)?);
    }
    let index = schema.formsets.len();
    schema.formsets.push(FormSet {
        list_index,
        package_index,
        opcode_offset: opcode_offset as u32,
        guid,
        title_id,
        help_id,
        flags,
        class_guids,
        forms: Vec::new(),
        varstores: Vec::new(),
    });
    schema.stats.formsets = schema.stats.formsets.saturating_add(1);
    context.formset = Some(index);
    context.form = None;
    context.question = None;
    Ok(())
}

fn parse_form(
    schema: &mut FirmwareSchema,
    list_index: usize,
    opcode_offset: usize,
    bytes: &[u8],
    context: &mut ParseContext,
) -> Result<(), String> {
    if bytes.len() < 6 {
        return Err(String::from("FORM opcode is truncated"));
    }
    if schema.forms.len() >= MAX_FORMS {
        return Err(String::from("form count exceeds bound"));
    }
    let index = schema.forms.len();
    schema.forms.push(Form {
        list_index,
        formset_index: context.formset,
        opcode_offset: opcode_offset as u32,
        id: read_u16(bytes, 2)?,
        title_id: read_u16(bytes, 4)?,
        conditions: context.conditions,
        questions: Vec::new(),
    });
    if let Some(formset) = context.formset.and_then(|index| schema.formsets.get_mut(index)) {
        formset.forms.push(index);
    }
    schema.stats.forms = schema.stats.forms.saturating_add(1);
    context.form = Some(index);
    context.question = None;
    Ok(())
}

fn parse_buffer_varstore(
    schema: &mut FirmwareSchema,
    list_index: usize,
    opcode_offset: usize,
    bytes: &[u8],
    context: ParseContext,
) -> Result<(), String> {
    if bytes.len() < 23 {
        return Err(String::from("VARSTORE opcode is truncated"));
    }
    let name = read_c_string(bytes, 22)?;
    add_varstore(
        schema,
        VarStore {
            list_index,
            formset_index: context.formset,
            opcode_offset: opcode_offset as u32,
            id: read_u16(bytes, 18)?,
            kind: VarStoreKind::Buffer {
                guid: read_guid(bytes, 2)?,
                size: read_u16(bytes, 20)?,
                name,
            },
            device_path_id: None,
        },
    )
}

fn parse_efi_varstore(
    schema: &mut FirmwareSchema,
    list_index: usize,
    opcode_offset: usize,
    bytes: &[u8],
    context: ParseContext,
) -> Result<(), String> {
    if bytes.len() < 27 {
        return Err(String::from("VARSTORE_EFI opcode is truncated"));
    }
    let name = read_c_string(bytes, 26)?;
    add_varstore(
        schema,
        VarStore {
            list_index,
            formset_index: context.formset,
            opcode_offset: opcode_offset as u32,
            id: read_u16(bytes, 2)?,
            kind: VarStoreKind::EfiVariable {
                guid: read_guid(bytes, 4)?,
                attributes: read_u32(bytes, 20)?,
                size: read_u16(bytes, 24)?,
                name,
            },
            device_path_id: None,
        },
    )
}

fn parse_name_value_varstore(
    schema: &mut FirmwareSchema,
    list_index: usize,
    opcode_offset: usize,
    bytes: &[u8],
    context: ParseContext,
) -> Result<(), String> {
    if bytes.len() < 20 {
        return Err(String::from("VARSTORE_NAME_VALUE opcode is truncated"));
    }
    add_varstore(
        schema,
        VarStore {
            list_index,
            formset_index: context.formset,
            opcode_offset: opcode_offset as u32,
            id: read_u16(bytes, 2)?,
            kind: VarStoreKind::NameValue {
                guid: read_guid(bytes, 4)?,
            },
            device_path_id: None,
        },
    )
}

fn add_varstore(schema: &mut FirmwareSchema, store: VarStore) -> Result<(), String> {
    if schema.varstores.len() >= MAX_VARSTORES {
        return Err(String::from("varstore count exceeds bound"));
    }
    let formset_index = store.formset_index;
    let index = schema.varstores.len();
    schema.varstores.push(store);
    if let Some(formset) = formset_index.and_then(|index| schema.formsets.get_mut(index)) {
        formset.varstores.push(index);
    }
    schema.stats.varstores = schema.stats.varstores.saturating_add(1);
    Ok(())
}

fn attach_varstore_device(
    schema: &mut FirmwareSchema,
    bytes: &[u8],
    context: ParseContext,
) -> Result<(), String> {
    if bytes.len() < 4 {
        return Err(String::from("VARSTORE_DEVICE opcode is truncated"));
    }
    let device_path_id = read_u16(bytes, 2)?;
    if let Some(store) = schema
        .varstores
        .iter_mut()
        .rev()
        .find(|store| store.formset_index == context.formset)
    {
        store.device_path_id = Some(device_path_id);
    }
    Ok(())
}

fn parse_default_store(
    schema: &mut FirmwareSchema,
    list_index: usize,
    opcode_offset: usize,
    bytes: &[u8],
    context: ParseContext,
) -> Result<(), String> {
    if bytes.len() < 6 {
        return Err(String::from("DEFAULTSTORE opcode is truncated"));
    }
    if schema.default_stores.len() >= MAX_DEFAULT_STORES {
        return Err(String::from("default-store count exceeds bound"));
    }
    schema.default_stores.push(DefaultStore {
        list_index,
        formset_index: context.formset,
        opcode_offset: opcode_offset as u32,
        name_id: read_u16(bytes, 2)?,
        id: read_u16(bytes, 4)?,
    });
    schema.stats.default_stores = schema.stats.default_stores.saturating_add(1);
    Ok(())
}

fn parse_question(
    schema: &mut FirmwareSchema,
    list_index: usize,
    opcode_offset: usize,
    bytes: &[u8],
    kind: QuestionKind,
    context: &mut ParseContext,
) -> Result<(), String> {
    if bytes.len() < 13 {
        return Err(alloc::format!("{} question header is truncated", kind.name()));
    }
    if schema.questions.len() >= MAX_QUESTIONS {
        return Err(String::from("question count exceeds bound"));
    }
    let kind_flags = match kind {
        QuestionKind::OneOf
        | QuestionKind::Numeric
        | QuestionKind::Checkbox
        | QuestionKind::Date
        | QuestionKind::Time => {
            if bytes.len() < 14 {
                return Err(alloc::format!("{} question flags are truncated", kind.name()));
            }
            bytes[13]
        }
        QuestionKind::String => {
            if bytes.len() < 16 {
                return Err(String::from("string question bounds are truncated"));
            }
            bytes[15]
        }
        QuestionKind::OrderedList => {
            if bytes.len() < 15 {
                return Err(String::from("ordered-list question is truncated"));
            }
            bytes[14]
        }
        QuestionKind::Password | QuestionKind::Action | QuestionKind::Reference => 0,
    };
    let mut numeric = None;
    let mut min_size = None;
    let mut max_size = None;
    let mut max_containers = None;

    match kind {
        QuestionKind::OneOf | QuestionKind::Numeric => {
            let width = numeric_width(kind_flags);
            let base = 14usize;
            let required = base
                .checked_add(usize::from(width) * 3)
                .ok_or_else(|| String::from("numeric range length overflow"))?;
            if bytes.len() < required {
                return Err(String::from("numeric range is truncated"));
            }
            numeric = Some(NumericRange {
                width,
                minimum: read_width(bytes, base, width)?,
                maximum: read_width(bytes, base + usize::from(width), width)?,
                step: read_width(bytes, base + usize::from(width) * 2, width)?,
            });
        }
        QuestionKind::Password => {
            if bytes.len() < 17 {
                return Err(String::from("password question bounds are truncated"));
            }
            min_size = Some(read_u16(bytes, 13)?);
            max_size = Some(read_u16(bytes, 15)?);
        }
        QuestionKind::String => {
            min_size = Some(u16::from(bytes[13]));
            max_size = Some(u16::from(bytes[14]));
        }
        QuestionKind::OrderedList => {
            max_containers = Some(bytes[13]);
        }
        _ => {}
    }

    let question_flags = bytes[12];
    let index = schema.questions.len();
    let mut question = Question {
        list_index,
        formset_index: context.formset,
        form_index: context.form,
        opcode_offset: opcode_offset as u32,
        id: read_u16(bytes, 6)?,
        kind,
        prompt_id: read_u16(bytes, 2)?,
        help_id: read_u16(bytes, 4)?,
        varstore_id: read_u16(bytes, 8)?,
        varstore_info: read_u16(bytes, 10)?,
        flags: question_flags,
        kind_flags,
        conditions: context.conditions,
        numeric,
        min_size,
        max_size,
        max_containers,
        options: Vec::new(),
        defaults: Vec::new(),
        validation_constraints: 0,
    };
    if kind == QuestionKind::Checkbox {
        if kind_flags & CHECKBOX_DEFAULT != 0 {
            if schema.stats.defaults as usize >= MAX_DEFAULTS {
                return Err(String::from("default count exceeds bound"));
            }
            question.defaults.push(QuestionDefault {
                default_id: 0,
                source: "checkbox-flag",
                value: TypedValue::Boolean(true),
            });
            schema.stats.defaults = schema.stats.defaults.saturating_add(1);
        }
        if kind_flags & CHECKBOX_DEFAULT_MFG != 0 {
            if schema.stats.defaults as usize >= MAX_DEFAULTS {
                return Err(String::from("default count exceeds bound"));
            }
            question.defaults.push(QuestionDefault {
                default_id: 1,
                source: "checkbox-flag",
                value: TypedValue::Boolean(true),
            });
            schema.stats.defaults = schema.stats.defaults.saturating_add(1);
        }
    }
    schema.questions.push(question);
    if let Some(form) = context.form.and_then(|index| schema.forms.get_mut(index)) {
        form.questions.push(index);
    }
    schema.stats.questions = schema.stats.questions.saturating_add(1);
    context.question = Some(index);
    Ok(())
}

fn parse_option(
    schema: &mut FirmwareSchema,
    bytes: &[u8],
    context: ParseContext,
) -> Result<(), String> {
    if bytes.len() < 6 {
        return Err(String::from("ONE_OF_OPTION opcode is truncated"));
    }
    let Some(question_index) = context.question else {
        schema.diagnostics.orphan_options = schema.diagnostics.orphan_options.saturating_add(1);
        return Ok(());
    };
    if schema.stats.options as usize >= MAX_OPTIONS {
        return Err(String::from("option count exceeds bound"));
    }
    let flags = bytes[4];
    let type_code = bytes[5];
    let value = parse_typed_value(bytes, 6, type_code)?;
    let default_value = value.clone();
    let manufacturing_value = value.clone();
    let add_standard_default = flags & OPTION_DEFAULT != 0;
    let add_manufacturing_default = flags & OPTION_DEFAULT_MFG != 0;
    let additional_defaults = (add_standard_default as usize) + (add_manufacturing_default as usize);
    if (schema.stats.defaults as usize).saturating_add(additional_defaults) > MAX_DEFAULTS {
        return Err(String::from("default count exceeds bound"));
    }
    {
        let question = schema
            .questions
            .get_mut(question_index)
            .ok_or_else(|| String::from("option question index is invalid"))?;
        question.options.push(QuestionOption {
            text_id: read_u16(bytes, 2)?,
            flags,
            type_code,
            value,
        });
        if add_standard_default {
            question.defaults.push(QuestionDefault {
                default_id: 0,
                source: "option-flag",
                value: default_value,
            });
        }
        if add_manufacturing_default {
            question.defaults.push(QuestionDefault {
                default_id: 1,
                source: "option-flag",
                value: manufacturing_value,
            });
        }
    }
    schema.stats.options = schema.stats.options.saturating_add(1);
    schema.stats.defaults = schema
        .stats
        .defaults
        .saturating_add(additional_defaults as u32);
    Ok(())
}

fn parse_default(
    schema: &mut FirmwareSchema,
    bytes: &[u8],
    context: ParseContext,
    scoped: bool,
) -> Result<(), String> {
    if bytes.len() < 5 {
        return Err(String::from("DEFAULT opcode is truncated"));
    }
    let Some(question_index) = context.question else {
        schema.diagnostics.orphan_defaults = schema.diagnostics.orphan_defaults.saturating_add(1);
        return Ok(());
    };
    if schema.stats.defaults as usize >= MAX_DEFAULTS {
        return Err(String::from("default count exceeds bound"));
    }
    let type_code = bytes[4];
    let value = if scoped && bytes.len() == 5 && known_type_width(type_code).is_some() {
        TypedValue::Expression { type_code }
    } else {
        parse_typed_value(bytes, 5, type_code)?
    };
    let default = QuestionDefault {
        default_id: read_u16(bytes, 2)?,
        source: if matches!(&value, TypedValue::Expression { .. }) {
            "default-expression"
        } else {
            "default-opcode"
        },
        value,
    };
    let question = schema
        .questions
        .get_mut(question_index)
        .ok_or_else(|| String::from("default question index is invalid"))?;
    question.defaults.push(default);
    schema.stats.defaults = schema.stats.defaults.saturating_add(1);
    Ok(())
}

fn parse_typed_value(bytes: &[u8], offset: usize, type_code: u8) -> Result<TypedValue, String> {
    match type_code {
        TYPE_U8 => Ok(TypedValue::Unsigned(u64::from(read_u8(bytes, offset)?))),
        TYPE_U16 => Ok(TypedValue::Unsigned(u64::from(read_u16(bytes, offset)?))),
        TYPE_U32 => Ok(TypedValue::Unsigned(u64::from(read_u32(bytes, offset)?))),
        TYPE_U64 => Ok(TypedValue::Unsigned(read_u64(bytes, offset)?)),
        TYPE_BOOLEAN => Ok(TypedValue::Boolean(read_u8(bytes, offset)? != 0)),
        TYPE_TIME => {
            checked_end(offset, 3, bytes.len(), "time value")?;
            Ok(TypedValue::Time {
                hour: bytes[offset],
                minute: bytes[offset + 1],
                second: bytes[offset + 2],
            })
        }
        TYPE_DATE => {
            checked_end(offset, 4, bytes.len(), "date value")?;
            Ok(TypedValue::Date {
                year: read_u16(bytes, offset)?,
                month: bytes[offset + 2],
                day: bytes[offset + 3],
            })
        }
        TYPE_STRING | TYPE_ACTION => Ok(TypedValue::StringId(read_u16(bytes, offset)?)),
        _ => {
            let remaining = bytes.len().saturating_sub(offset);
            let keep = remaining.min(16);
            Ok(TypedValue::Opaque {
                type_code,
                bytes: bytes[offset..offset + keep].to_vec(),
            })
        }
    }
}

fn known_type_width(type_code: u8) -> Option<usize> {
    match type_code {
        TYPE_U8 | TYPE_BOOLEAN => Some(1),
        TYPE_U16 | TYPE_STRING | TYPE_ACTION => Some(2),
        TYPE_U32 | TYPE_DATE => Some(4),
        TYPE_U64 => Some(8),
        TYPE_TIME => Some(3),
        _ => None,
    }
}

fn numeric_width(flags: u8) -> u8 {
    match flags & 0x03 {
        0 => 1,
        1 => 2,
        2 => 4,
        _ => 8,
    }
}

fn read_width(bytes: &[u8], offset: usize, width: u8) -> Result<u64, String> {
    match width {
        1 => Ok(u64::from(read_u8(bytes, offset)?)),
        2 => Ok(u64::from(read_u16(bytes, offset)?)),
        4 => Ok(u64::from(read_u32(bytes, offset)?)),
        8 => read_u64(bytes, offset),
        _ => Err(String::from("unsupported numeric width")),
    }
}

fn read_c_string(bytes: &[u8], start: usize) -> Result<String, String> {
    let end = bytes
        .get(start..)
        .ok_or_else(|| String::from("inline name starts outside opcode"))?
        .iter()
        .position(|byte| *byte == 0)
        .map(|relative| start + relative)
        .ok_or_else(|| String::from("inline name is not terminated"))?;
    let mut name = String::with_capacity(end - start);
    for byte in &bytes[start..end] {
        if byte.is_ascii_graphic() || *byte == b' ' {
            name.push(char::from(*byte));
        } else {
            name.push('?');
        }
    }
    Ok(name)
}

fn push_diagnostic(diagnostics: &mut IfrDiagnostics, message: String) {
    if diagnostics.messages.len() < MAX_DIAGNOSTICS {
        diagnostics.messages.push(message);
    }
}

fn checked_end(start: usize, length: usize, limit: usize, label: &str) -> Result<usize, String> {
    let end = start
        .checked_add(length)
        .ok_or_else(|| alloc::format!("{} range overflow", label))?;
    if end > limit {
        Err(alloc::format!("{} is truncated", label))
    } else {
        Ok(end)
    }
}

fn read_guid(bytes: &[u8], offset: usize) -> Result<EfiGuid, String> {
    let end = checked_end(offset, 16, bytes.len(), "GUID")?;
    let mut raw = [0u8; 16];
    raw.copy_from_slice(&bytes[offset..end]);
    Ok(EfiGuid::from_uefi_bytes(raw))
}

fn read_u8(bytes: &[u8], offset: usize) -> Result<u8, String> {
    bytes
        .get(offset)
        .copied()
        .ok_or_else(|| String::from("u8 crosses buffer"))
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, String> {
    let slice = bytes
        .get(offset..offset.saturating_add(2))
        .ok_or_else(|| String::from("u16 crosses buffer"))?;
    Ok(u16::from_le_bytes([slice[0], slice[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let slice = bytes
        .get(offset..offset.saturating_add(4))
        .ok_or_else(|| String::from("u32 crosses buffer"))?;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, String> {
    let slice = bytes
        .get(offset..offset.saturating_add(8))
        .ok_or_else(|| String::from("u64 crosses buffer"))?;
    Ok(u64::from_le_bytes([
        slice[0], slice[1], slice[2], slice[3], slice[4], slice[5], slice[6], slice[7],
    ]))
}

#[cfg(test)]
mod tests {
    use alloc::vec;

    use super::*;

    fn package_header(length: usize, package_type: u8) -> [u8; 4] {
        let raw = (length as u32 & 0x00ff_ffff) | ((package_type as u32) << 24);
        raw.to_le_bytes()
    }

    fn op(opcode: u8, scoped: bool, body: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(body.len() + 2);
        bytes.push(opcode);
        bytes.push((body.len() as u8 + 2) | if scoped { 0x80 } else { 0 });
        bytes.extend_from_slice(body);
        bytes
    }

    fn string_package(strings_in: &[&str]) -> Vec<u8> {
        let mut package = Vec::new();
        package.extend_from_slice(&[0; 4]);
        let header_bytes = 52u32;
        package.extend_from_slice(&header_bytes.to_le_bytes());
        package.extend_from_slice(&header_bytes.to_le_bytes());
        package.extend_from_slice(&[0; 32]);
        package.extend_from_slice(&1u16.to_le_bytes());
        package.extend_from_slice(b"en-US\0");
        for text in strings_in {
            package.push(0x10);
            package.extend_from_slice(text.as_bytes());
            package.push(0);
        }
        package.push(0);
        let package_len = package.len();
        package[0..4].copy_from_slice(&package_header(package_len, 0x04));
        package
    }

    fn fixture() -> Vec<u8> {
        let strings = string_package(&[
            "Setup",
            "Advanced",
            "Storage",
            "SATA Mode Selection",
            "Choose AHCI or RAID",
            "AHCI",
            "Intel RST RAID",
        ]);

        let mut ifr = Vec::new();
        let mut formset = vec![0x22; 16];
        formset.extend_from_slice(&2u16.to_le_bytes());
        formset.extend_from_slice(&0u16.to_le_bytes());
        formset.push(0);
        ifr.extend_from_slice(&op(OP_FORM_SET, true, &formset));

        let mut varstore = vec![0x33; 16];
        varstore.extend_from_slice(&1u16.to_le_bytes());
        varstore.extend_from_slice(&64u16.to_le_bytes());
        varstore.extend_from_slice(b"Setup\0");
        ifr.extend_from_slice(&op(OP_VARSTORE, false, &varstore));

        let mut form = Vec::new();
        form.extend_from_slice(&0x100u16.to_le_bytes());
        form.extend_from_slice(&3u16.to_le_bytes());
        ifr.extend_from_slice(&op(OP_FORM, true, &form));

        let mut question = Vec::new();
        question.extend_from_slice(&4u16.to_le_bytes());
        question.extend_from_slice(&5u16.to_le_bytes());
        question.extend_from_slice(&0x200u16.to_le_bytes());
        question.extend_from_slice(&1u16.to_le_bytes());
        question.extend_from_slice(&8u16.to_le_bytes());
        question.push(QUESTION_RESET_REQUIRED);
        question.push(0);
        question.extend_from_slice(&0u8.to_le_bytes());
        question.extend_from_slice(&1u8.to_le_bytes());
        question.extend_from_slice(&1u8.to_le_bytes());
        ifr.extend_from_slice(&op(OP_ONE_OF, true, &question));

        let mut option = Vec::new();
        option.extend_from_slice(&6u16.to_le_bytes());
        option.push(OPTION_DEFAULT);
        option.push(TYPE_U8);
        option.push(0);
        ifr.extend_from_slice(&op(OP_ONE_OF_OPTION, false, &option));

        let mut option = Vec::new();
        option.extend_from_slice(&7u16.to_le_bytes());
        option.push(0);
        option.push(TYPE_U8);
        option.push(1);
        ifr.extend_from_slice(&op(OP_ONE_OF_OPTION, false, &option));
        ifr.extend_from_slice(&op(OP_END, false, &[]));
        ifr.extend_from_slice(&op(OP_END, false, &[]));
        ifr.extend_from_slice(&op(OP_END, false, &[]));

        let mut forms = vec![0; 4];
        forms.extend_from_slice(&ifr);
        let forms_len = forms.len();
        forms[0..4].copy_from_slice(&package_header(forms_len, 0x02));

        let list_len = 20 + strings.len() + forms.len();
        let mut list = Vec::new();
        list.extend_from_slice(&[0x44; 16]);
        list.extend_from_slice(&(list_len as u32).to_le_bytes());
        list.extend_from_slice(&strings);
        list.extend_from_slice(&forms);
        list
    }

    #[test]
    fn decodes_one_of_question_and_storage() {
        let hii = bios_hii::parse_hii_for_test(fixture()).unwrap();
        let schema = FirmwareSchema::parse(hii).unwrap();
        assert_eq!(schema.formsets.len(), 1);
        assert_eq!(schema.forms.len(), 1);
        assert_eq!(schema.varstores.len(), 1);
        assert_eq!(schema.questions.len(), 1);
        let question = &schema.questions[0];
        assert_eq!(question.id, 0x200);
        assert_eq!(question.options.len(), 2);
        assert!(question.reset_required());
        assert_eq!(schema.question_prompt(0), Some("SATA Mode Selection"));
        assert!(schema.resolve_varstore(question).is_some());
    }
}
