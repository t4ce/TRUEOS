#!/usr/bin/env python3
"""Exercise Shell3's physical presentation cells and actual frame sizing.

Only display discovery/EDID-derived density and physical PCI identity are
fixtures. Production font sizing and show_extent execute unchanged.
"""

from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import constant, item


ROOT = Path(__file__).resolve().parents[2]
FONT = "src/r/services/shell3_font_service.rs"


def main() -> None:
    source = r'''
#![allow(dead_code)]
mod pci {
    thread_local! { pub static LAPTOP: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
    pub fn experimental_tgl_9a49_active()->bool { LAPTOP.with(std::cell::Cell::get) }
}
mod ui4 {
    thread_local! {
        pub static OUTPUT: std::cell::Cell<Option<(u32,u32)>> = const { std::cell::Cell::new(Some((3840,2160))) };
        pub static MILLIMETERS: std::cell::Cell<Option<(u32,u32)>> = const { std::cell::Cell::new(Some((309,173))) };
    }
    pub fn output_dimensions()->Option<(u32,u32)> { OUTPUT.with(std::cell::Cell::get) }
    pub fn physical_extent_pixels_fraction(width:u32,height:u32,denominator:u32)->Option<(u32,u32)> {
        if width==0 || height==0 || denominator==0 { return None; }
        let (pixels_x,pixels_y)=output_dimensions()?;
        let (mm_x,mm_y)=MILLIMETERS.with(std::cell::Cell::get)?;
        let axis=|pixels:u32,requested:u32,millimeters:u32| {
            let divisor=u64::from(millimeters)*u64::from(denominator);
            let result=(u64::from(pixels)*u64::from(requested)+divisor/2)/divisor;
            u32::try_from(result).ok().filter(|&result|result!=0)
        };
        Some((axis(pixels_x,width,mm_x)?,axis(pixels_y,height,mm_y)?))
    }
}
mod microfont { pub const FWIDTH:usize=6; pub const FHEIGHT:usize=11; }
mod r { pub mod services { pub mod shell3_font_service {
    use crate::microfont;
'''
    for name in ("DEFAULT_UI4_FONT_SCALE", "ATLAS_EM_HALF_MM", "LAPTOP_SHELL_EM_HALF_MM"):
        source += constant(FONT, name) + "\n"
    source += item(FONT, "cell_extent") + item(FONT, "cell_extent_for_em")
    source += r'''
    #[test]
    fn laptop_uses_four_mm_cells_and_retains_the_canonical_atlas_height() {
        crate::pci::LAPTOP.with(|value|value.set(true));
        assert_eq!(DEFAULT_UI4_FONT_SCALE,2,"100% is still scale 2");
        assert_eq!(cell_extent(2),Some((27,50)));
        assert_eq!(cell_extent_for_em(2,ATLAS_EM_HALF_MM),Some((51,94)),
            "the existing native R8 producer retains its 7.5-mm atlas");
        assert_eq!(cell_extent(1),Some((14,25)));
        assert_eq!(cell_extent(3),Some((41,75)));
        assert_eq!(cell_extent(8),Some((109,200)));
    }

    #[test]
    fn desktop_and_unknown_physical_size_retain_their_previous_cells() {
        crate::pci::LAPTOP.with(|value|value.set(false));
        crate::ui4::OUTPUT.with(|value|value.set(Some((2560,1440))));
        crate::ui4::MILLIMETERS.with(|value|value.set(Some((597,336))));
        assert_eq!(cell_extent(2),Some((17,32)));
        assert_eq!(cell_extent(1),Some((9,16)));
        crate::ui4::MILLIMETERS.with(|value|value.set(None));
        for laptop in [false,true] {
            crate::pci::LAPTOP.with(|value|value.set(laptop));
            assert_eq!(cell_extent(2),Some((12,22)));
            assert_eq!(cell_extent(1),Some((6,11)));
        }
        crate::ui4::OUTPUT.with(|value|value.set(None));
        assert_eq!(cell_extent(2),None);
        assert_eq!(cell_extent(0),None);
        assert_eq!(cell_extent(u32::MAX),None);
    }

    #[test]
    fn laptop_policy_uses_physical_density_instead_of_a_fixed_pixel_height() {
        crate::pci::LAPTOP.with(|value|value.set(true));
        crate::ui4::OUTPUT.with(|value|value.set(Some((1920,1080))));
        assert_eq!(cell_extent(2),Some((14,25)));
        crate::pci::LAPTOP.with(|value|value.set(false));
        crate::ui4::OUTPUT.with(|value|value.set(Some((3840,2160))));
        assert_eq!(cell_extent(2),Some((51,94)),
            "resolution alone does not select the laptop presentation policy");
    }
}}}
mod shell3 {
'''
    for name in ("DEFAULT_COLUMNS", "DEFAULT_ROWS", "MIN_COLUMNS", "MIN_ROWS"):
        source += constant("src/shell3/shell3.rs", name) + "\n"
    source += "mod show {\n" + item("src/shell3/show/show.rs", "show_extent")
    source += r'''
    #[test]
    fn default_laptop_frame_fits_one_hundred_by_twenty_five_four_mm_cells() {
        crate::pci::LAPTOP.with(|value|value.set(true));
        assert_eq!(show_extent(super::DEFAULT_COLUMNS,super::DEFAULT_ROWS,2),Ok((2700,1250)));
        assert_eq!(show_extent(super::DEFAULT_COLUMNS,super::DEFAULT_ROWS,1),Ok((1400,625)));
        let (width,height)=show_extent(super::DEFAULT_COLUMNS,super::DEFAULT_ROWS,2).unwrap();
        let (cell_width,cell_height)=crate::r::services::shell3_font_service::cell_extent(2).unwrap();
        assert_eq!((width/cell_width,height/cell_height),(100,25));
    }

    #[test]
    fn desktop_frame_extent_is_unchanged_and_zoom_still_clips_to_the_viewport() {
        crate::pci::LAPTOP.with(|value|value.set(false));
        crate::ui4::OUTPUT.with(|value|value.set(Some((2560,1440))));
        crate::ui4::MILLIMETERS.with(|value|value.set(Some((597,336))));
        assert_eq!(show_extent(super::DEFAULT_COLUMNS,super::DEFAULT_ROWS,2),Ok((1700,800)));
        crate::pci::LAPTOP.with(|value|value.set(true));
        crate::ui4::OUTPUT.with(|value|value.set(Some((3840,2160))));
        crate::ui4::MILLIMETERS.with(|value|value.set(Some((309,173))));
        assert_eq!(show_extent(super::DEFAULT_COLUMNS,super::DEFAULT_ROWS,8),Ok((3815,2000)));
    }
}}
'''
    with tempfile.TemporaryDirectory(prefix="trueos-shell3-physical-size-") as directory:
        code = Path(directory) / "tests.rs"
        binary = Path(directory) / "tests"
        code.write_text(source)
        subprocess.run([
            "rustc", "--edition=2024", "--target=x86_64-unknown-linux-gnu", "--test",
            "-C", "linker=" + str(ROOT / "tools/rust-host-ld"), str(code), "-o", str(binary),
        ], cwd=ROOT, check=True)
        subprocess.run([str(binary)], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
