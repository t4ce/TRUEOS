#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Path<'a> {
    pub bytes: &'a [u8],
}

impl<'a> Path<'a> {
    pub const fn from_bytes(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TextPath<'a> {
    pub text: &'a str,
}

impl<'a> TextPath<'a> {
    pub const fn new(text: &'a str) -> Self {
        Self { text }
    }
}
