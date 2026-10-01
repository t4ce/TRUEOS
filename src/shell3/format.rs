//! Small ANSI formatter for Shell3's supported text styles.

use super::RgbaColor;

pub const BOLD_ON: &str = "\x1b[1m";
pub const RESET: &str = "\x1b[0m";

/// Formats text in bold and/or one of Shell3's six named foreground colors.
///
/// The formatter uses 24-bit foreground color codes derived from the RGB
/// components of `RgbaColor`; alpha is intentionally ignored by ANSI output.
pub fn styled(text: &str, bold: bool, color: Option<RgbaColor>) -> String {
    let mut output = String::new();

    if bold {
        output.push_str(BOLD_ON);
    }

    if let Some(color) = color {
        let [r, g, b, _] = color.rgba();
        output.push_str(&alloc::format!("\x1b[38;2;{r};{g};{b}m"));
    }

    output.push_str(text);

    if bold || color.is_some() {
        output.push_str(RESET);
    }

    output
}

/// Formats text in bold without applying color.
pub fn bold(text: &str) -> String {
    styled(text, true, None)
}

