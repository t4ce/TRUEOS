use skrifa::{raw::TableProvider, FontRef, MetadataProvider};

const BOOT_FONT_BYTES: &[u8] = include_bytes!("../tools/L_10646.TTF");

pub(crate) fn log_boot_font_probe() {
    match boot_font_probe_summary() {
        Ok(summary) => crate::log_info!(
            target: "boot";
            "font-probe: result=ok parser=skrifa font=L_10646.TTF bytes={} tables={} glyphs={} units_per_em={} cmap={} glyph_A={} glyph_space={}\n",
            summary.bytes,
            summary.tables,
            summary.glyphs,
            summary.units_per_em,
            summary.cmap_status,
            summary.glyph_a,
            summary.glyph_space
        ),
        Err(err) => crate::log_warn!(
            target: "boot";
            "font-probe: result=failed parser=skrifa font=L_10646.TTF bytes={} err={:?}\n",
            BOOT_FONT_BYTES.len(),
            err
        ),
    }
}

#[derive(Debug)]
pub(crate) struct FontProbeSummary {
    pub(crate) bytes: usize,
    pub(crate) tables: usize,
    pub(crate) glyphs: u16,
    pub(crate) units_per_em: u16,
    pub(crate) cmap_status: &'static str,
    pub(crate) glyph_a: u32,
    pub(crate) glyph_space: u32,
}

pub(crate) fn boot_font_probe_summary() -> Result<FontProbeSummary, skrifa::raw::ReadError> {
    let font = FontRef::new(BOOT_FONT_BYTES)?;
    let head = font.head()?;
    let maxp = font.maxp()?;
    let charmap = font.charmap();
    let glyph_a = charmap.map('A' as u32).map(|gid| gid.to_u32()).unwrap_or(0);
    let glyph_space = charmap
        .map(' ' as u32)
        .map(|gid| gid.to_u32())
        .unwrap_or(0);
    let cmap_status = if charmap.has_map() { "present" } else { "missing" };

    Ok(FontProbeSummary {
        bytes: BOOT_FONT_BYTES.len(),
        tables: font.table_directory().table_records().len(),
        glyphs: maxp.num_glyphs(),
        units_per_em: head.units_per_em(),
        cmap_status,
        glyph_a,
        glyph_space,
    })
}
