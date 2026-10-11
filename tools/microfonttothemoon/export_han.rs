//! Read the vendored decoder itself so the inspection sheet preserves every mask.
#[path = "../../vendor/microfont/src/gui_han.rs"]
mod gui_han;

fn main() {
    let mut count = 0;
    for cp in 0..=u16::MAX as u32 {
        let Some(ch) = char::from_u32(cp) else {
            continue;
        };
        if let Some(bits) = gui_han::glyph_cell_pixels(ch) {
            println!("{cp:04x} {bits:017x}");
            count += 1;
        }
    }
    assert_eq!(count, gui_han::GUI_HAN_COUNT);
}
