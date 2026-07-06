use alloc::format;

use super::super::{ShellBackend2, print_native_line, print_shell_line, term_style};
use crate::shell2::shell2_cmd::ParseOutcome;

const FONT_CMD_RGB: (u8, u8, u8) = (255, 190, 90);

pub(crate) fn try_parse(io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let rest = rest.trim();
    if !rest.is_empty() && !rest.eq_ignore_ascii_case("probe") {
        print_shell_line(io, "font: usage `font` | `font probe`");
        return ParseOutcome::Handled;
    }

    match crate::font_probe::boot_font_probe_summary() {
        Ok(summary) => {
            print_native_line(
                io,
                format!(
                    "{}",
                    term_style::paint("font: skrifa probe ok")
                        .bold()
                        .color(FONT_CMD_RGB)
                )
                .as_str(),
            );
            print_shell_line(
                io,
                format!(
                    "font: L_10646.TTF bytes={} tables={} glyphs={} units_per_em={} cmap={} glyph_A={} glyph_space={}",
                    summary.bytes,
                    summary.tables,
                    summary.glyphs,
                    summary.units_per_em,
                    summary.cmap_status,
                    summary.glyph_a,
                    summary.glyph_space
                )
                .as_str(),
            );
        }
        Err(err) => {
            print_native_line(
                io,
                format!(
                    "{}",
                    term_style::paint("font: skrifa probe failed")
                        .bold()
                        .color(FONT_CMD_RGB)
                )
                .as_str(),
            );
            print_shell_line(io, format!("font: L_10646.TTF err={:?}", err).as_str());
        }
    }

    ParseOutcome::Handled
}
