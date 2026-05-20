use crate::path::{Path, TextPath};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ArgumentTemplate {
    pub name: &'static str,
    pub kind: ArgumentKind,
    pub optional: bool,
}

impl ArgumentTemplate {
    pub const fn required(name: &'static str, kind: ArgumentKind) -> Self {
        Self {
            name,
            kind,
            optional: false,
        }
    }

    pub const fn optional(name: &'static str, kind: ArgumentKind) -> Self {
        Self {
            name,
            kind,
            optional: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArgumentKind {
    Text,
    Bytes,
    Number,
    Flag,
    Path,
    TextPath,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Argument<'a> {
    Text(&'a str),
    Bytes(&'a [u8]),
    Number(i64),
    Flag(&'a str),
    Path(Path<'a>),
    TextPath(TextPath<'a>),
}
