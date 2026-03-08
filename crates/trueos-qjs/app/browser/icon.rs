#![cfg(feature = "trueos")]

pub(crate) const ICON_STROKE_RGBA: u32 = 0xFF202020;

#[derive(Clone, Copy)]
pub(crate) struct WindowIconDef {
    pub svg_source: &'static str,
    pub line_cmds: &'static [f64],
}

const CLOSE_LINES: [f64; 12] = [
    0.22,
    0.22,
    0.78,
    0.78,
    2.0,
    ICON_STROKE_RGBA as f64,
    0.78,
    0.22,
    0.22,
    0.78,
    2.0,
    ICON_STROKE_RGBA as f64,
];

const MINIMIZE_LINES: [f64; 6] = [0.20, 0.62, 0.80, 0.62, 2.0, ICON_STROKE_RGBA as f64];

const MAXIMIZE_LINES: [f64; 24] = [
    0.24,
    0.26,
    0.76,
    0.26,
    2.0,
    ICON_STROKE_RGBA as f64,
    0.76,
    0.26,
    0.76,
    0.74,
    2.0,
    ICON_STROKE_RGBA as f64,
    0.76,
    0.74,
    0.24,
    0.74,
    2.0,
    ICON_STROKE_RGBA as f64,
    0.24,
    0.74,
    0.24,
    0.26,
    2.0,
    ICON_STROKE_RGBA as f64,
];

const ARROW_LEFT_LINES: [f64; 12] = [
    0.70,
    0.24,
    0.36,
    0.50,
    2.0,
    ICON_STROKE_RGBA as f64,
    0.70,
    0.76,
    0.36,
    0.50,
    2.0,
    ICON_STROKE_RGBA as f64,
];

const ARROW_RIGHT_LINES: [f64; 12] = [
    0.30,
    0.24,
    0.64,
    0.50,
    2.0,
    ICON_STROKE_RGBA as f64,
    0.30,
    0.76,
    0.64,
    0.50,
    2.0,
    ICON_STROKE_RGBA as f64,
];

const ARROW_UP_LINES: [f64; 12] = [
    0.22,
    0.64,
    0.50,
    0.34,
    2.0,
    ICON_STROKE_RGBA as f64,
    0.78,
    0.64,
    0.50,
    0.34,
    2.0,
    ICON_STROKE_RGBA as f64,
];

const ARROW_DOWN_LINES: [f64; 12] = [
    0.22,
    0.36,
    0.50,
    0.66,
    2.0,
    ICON_STROKE_RGBA as f64,
    0.78,
    0.36,
    0.50,
    0.66,
    2.0,
    ICON_STROKE_RGBA as f64,
];

pub(crate) const WINDOW_ICON_DEFS: [WindowIconDef; 7] = [
    WindowIconDef {
        svg_source: r#"<svg viewBox='0 0 32 32' xmlns='http://www.w3.org/2000/svg'><path d='M7 7L25 25M25 7L7 25' stroke='#202020' stroke-width='2' fill='none'/></svg>"#,
        line_cmds: &CLOSE_LINES,
    },
    WindowIconDef {
        svg_source: r#"<svg viewBox='0 0 32 32' xmlns='http://www.w3.org/2000/svg'><path d='M6 20L26 20' stroke='#202020' stroke-width='2' fill='none'/></svg>"#,
        line_cmds: &MINIMIZE_LINES,
    },
    WindowIconDef {
        svg_source: r#"<svg viewBox='0 0 32 32' xmlns='http://www.w3.org/2000/svg'><rect x='8' y='8' width='16' height='16' fill='none' stroke='#202020' stroke-width='2'/></svg>"#,
        line_cmds: &MAXIMIZE_LINES,
    },
    WindowIconDef {
        svg_source: r#"<svg viewBox='0 0 32 32' xmlns='http://www.w3.org/2000/svg'><path d='M22 8L11 16L22 24' stroke='#202020' stroke-width='2' fill='none'/></svg>"#,
        line_cmds: &ARROW_LEFT_LINES,
    },
    WindowIconDef {
        svg_source: r#"<svg viewBox='0 0 32 32' xmlns='http://www.w3.org/2000/svg'><path d='M10 8L21 16L10 24' stroke='#202020' stroke-width='2' fill='none'/></svg>"#,
        line_cmds: &ARROW_RIGHT_LINES,
    },
    WindowIconDef {
        svg_source: r#"<svg viewBox='0 0 32 32' xmlns='http://www.w3.org/2000/svg'><path d='M7 21L16 11L25 21' stroke='#202020' stroke-width='2' fill='none'/></svg>"#,
        line_cmds: &ARROW_UP_LINES,
    },
    WindowIconDef {
        svg_source: r#"<svg viewBox='0 0 32 32' xmlns='http://www.w3.org/2000/svg'><path d='M7 11L16 21L25 11' stroke='#202020' stroke-width='2' fill='none'/></svg>"#,
        line_cmds: &ARROW_DOWN_LINES,
    },
];

pub(crate) const RADIO_SELECTED_SVG: &str =
    r#"<svg viewBox='0 0 32 32' xmlns='http://www.w3.org/2000/svg'><circle cx='16' cy='16' r='9' fill='none' stroke='#202020' stroke-width='2'/><circle cx='16' cy='16' r='3' fill='none' stroke='#202020' stroke-width='2'/></svg>"#;
pub(crate) const RADIO_SELECTED_SEGS: usize = 24;
pub(crate) const RADIO_SELECTED_OUTER_R: f32 = 0.30;
pub(crate) const RADIO_SELECTED_INNER_R: f32 = 0.10;
