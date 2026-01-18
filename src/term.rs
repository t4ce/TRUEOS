//! Terminal / ANSI escape helpers.
//!
//! Centralizes escape sequences so callers can keep their code readable.

/// Control Sequence Introducer (CSI).
pub const CSI: &str = "\x1b[";

/// Reset all attributes.
pub const RESET: &str = "\x1b[0m";

/// Save cursor position (DEC private mode).
pub const SAVE_CURSOR: &str = "\x1b[s";

/// Restore cursor position (DEC private mode).
pub const RESTORE_CURSOR: &str = "\x1b[u";

/// Shell prompt string.
///
/// `\x1b[38;2;...m` sets 24-bit RGB foreground color.
pub const PROMPT: &str = "\x1b[38;2;255;85;255m§\x1b[0m ";
