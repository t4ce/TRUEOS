//! Text plus Shell3 style metadata, without encoding it into the text.

use alloc::string::String;

use super::RgbaColor;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MetaFmtStr {
    pub text: String,
    pub color: Option<RgbaColor>,
    pub bold: bool,
}

impl MetaFmtStr {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            color: None,
            bold: false,
        }
    }

    pub fn color(mut self, color: RgbaColor) -> Self {
        self.color = Some(color);
        self
    }

    pub fn bold(mut self) -> Self {
        self.bold = true;
        self
    }
}
