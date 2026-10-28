// The native warm pass requires the GPU. Seed its real immutable atlas type
// with distinctive coverage so host tests verify consumption of resident
// pixels, not a coincidentally similar second font rasterization.
static FIXTURE_PRODUCTIONS: core::sync::atomic::AtomicUsize =
    core::sync::atomic::AtomicUsize::new(0);

pub(crate) fn warm_fixture_atlas() -> &'static OwnedAlphaAtlas {
    ALPHA_ATLAS.call_once(|| {
        FIXTURE_PRODUCTIONS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        let em_height_px = 44;
        let gutter_px = 6;
        let content_width = (em_height_px * 6 + 5) / 11;
        let glyph_width = content_width + 2 * gutter_px;
        let glyph_height = em_height_px + 2 * gutter_px;
        let bytes = (glyph_width * glyph_height) as usize;
        let mut pixels = alloc::vec![0; SLOT_COUNT * bytes];
        for character in ('!'..='~').chain(core::iter::once('§')) {
            let slot = glyph_slot(character).unwrap();
            for y in 0..em_height_px {
                for x in 0..content_width {
                    let coverage = if (x + 2 * y + slot as u32) % 9 < 4 {
                        (31 + (x * 17 + y * 23 + slot as u32) % 225) as u8
                    } else { 0 };
                    pixels[slot * bytes
                        + ((y + gutter_px) * glyph_width + x + gutter_px) as usize] = coverage;
                }
            }
        }
        OwnedAlphaAtlas { em_height_px, line_width_px: 2.75,
            glyph_width, glyph_height, gutter_px, pixels }
    })
}

pub(crate) fn fixture_productions() -> usize {
    FIXTURE_PRODUCTIONS.load(core::sync::atomic::Ordering::Relaxed)
}
