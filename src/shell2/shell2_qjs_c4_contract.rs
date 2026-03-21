use alloc::string::String;
use alloc::vec::Vec;

pub(crate) const C4_CORE_CONTRACT_VERSION: u16 = 1;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResultHint {
    String,
    Number,
    Boolean,
    Null,
    Undefined,
    Function,
    Object,
    Unknown,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SymbolRole {
    Decl,
    Assign,
    Read,
    Call,
}

#[derive(Clone)]
pub(crate) struct SymbolRef {
    pub(crate) name: String,
    pub(crate) role: SymbolRole,
}

#[derive(Clone)]
pub(crate) struct Diagnostic {
    pub(crate) code: &'static str,
    pub(crate) message: String,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

#[derive(Clone)]
pub(crate) struct Analysis {
    pub(crate) hint: ResultHint,
    pub(crate) symbols: Vec<SymbolRef>,
    pub(crate) diagnostic: Option<Diagnostic>,
}

pub(crate) const fn version() -> u16 {
    C4_CORE_CONTRACT_VERSION
}
