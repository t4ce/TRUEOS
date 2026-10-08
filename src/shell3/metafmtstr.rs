//! Text plus Shell3 style metadata, without encoding it into the text.

use alloc::string::String;

use super::RgbaColor;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MetaFmtStr {
    pub text: String,
    pub color: Option<RgbaColor>,
    pub bold: bool,
    pub underline: bool,
    pub blink: bool,
}

impl MetaFmtStr {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            color: None,
            bold: false,
            underline: false,
            blink: false,
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

    pub fn blink(mut self) -> Self {
        self.blink = true;
        self
    }

    pub fn underline(mut self) -> Self {
        self.underline = true;
        self
    }
}
