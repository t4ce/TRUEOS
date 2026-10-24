#!/usr/bin/env python3
"""Exercise the actual physical cell authority and Shell3 initial-frame sizing."""
from pathlib import Path
import re
import subprocess
import tempfile
import test_clip_position3_uv_texture as extract

ROOT = Path(__file__).resolve().parents[2]
extract.ROOT = ROOT

TESTS = r'''
#[test] fn default_frame_fits_one_hundred_columns_and_twenty_five_rows() {
    assert_eq!(r::services::shell3_font_service::DEFAULT_UI4_FONT_SCALE,2);
    assert_eq!(r::services::shell3_font_service::cell_extent(2),Some((15,28)));
    assert_eq!(show::show_extent(100,25,2),Ok((1500,700)));
    let mut view=show::Show {font_scale:2,initial_geometry:None};
    assert_eq!(view.initial_grid(100,25),Ok((100,25)));
}
#[test] fn edid_density_drives_the_source_cell_frame_and_clamped_grid_together() {
    DENSITY.set(Some(80)); // 80 pixels per 10 mm: 7.5mm becomes 60px.
    assert_eq!(r::services::shell3_font_service::cell_extent(2),Some((33,60)));
    assert_eq!(show::show_extent(100,25,2),Ok((1914,1080)));
    let mut view=show::Show {font_scale:2,initial_geometry:None};
    assert_eq!(view.initial_grid(100,25),Ok((58,18)));
}
#[test] fn relative_zoom_and_pointer_coordinates_use_the_same_fractional_cell() {
    assert_eq!(r::services::shell3_font_service::cell_extent(1),Some((8,14)));
    assert_eq!(r::services::shell3_font_service::cell_extent(4),Some((31,57)));
    let (w,h)=r::services::shell3_font_service::cell_extent(2).unwrap();
    for row in 0..25 {for col in 0..100 {
        assert_eq!(((col*w+w-1)/w,(row*h+h-1)/h),(col,row));
    }}
}
#[test] fn frame_sizes_are_whole_cells_and_initial_geometry_respects_new_minima() {
    let (w,h)=show::show_extent(usize::MAX,usize::MAX,2).unwrap();
    assert_eq!((w,h),(1920,1064));
    assert_eq!((w%15,h%28),(0,0));
    let mut view=show::Show {font_scale:2,initial_geometry:Some(show::WindowGeometry {width:100,height:100})};
    assert_eq!(view.initial_grid(100,25),Ok((20,5)));
    let geometry=view.initial_geometry.unwrap();
    assert_eq!((geometry.width,geometry.height),(300,140));
}
#[test] fn absent_metrics_invalid_zoom_and_too_small_screens_fail_closed() {
    OUTPUT.set(None);
    assert_eq!(r::services::shell3_font_service::cell_extent(2),None);
    assert!(show::show_extent(100,25,2).is_err());
    OUTPUT.set(Some((299,139)));
    assert!(show::show_extent(100,25,2).is_err());
    assert_eq!(r::services::shell3_font_service::cell_extent(0),None);
    assert_eq!(r::services::shell3_font_service::cell_extent(u32::MAX),None);
}
'''


def main():
    metrics = str(ROOT / 'src/ui4/display_metrics.rs')
    source = r'''
extern crate self as microfont;
std::thread_local! {
    static DENSITY:std::cell::Cell<Option<u32>>=const {std::cell::Cell::new(None)};
    static OUTPUT:std::cell::Cell<Option<(u32,u32)>>=const {std::cell::Cell::new(Some((1920,1080)))};
}
mod intel {
    pub fn physical_extent_pixels(w:u32,h:u32)->Option<(u32,u32)> {physical_extent_pixels_fraction(w,h,1)}
    pub fn physical_extent_pixels_fraction(w:u32,h:u32,d:u32)->Option<(u32,u32)> {
        let density=crate::DENSITY.get()?;
        Some((((w as u64*density as u64+5*d as u64)/(10*d as u64))as u32,
              ((h as u64*density as u64+5*d as u64)/(10*d as u64))as u32))
    }
}
mod virtio_gpu_logo {pub fn output_dimensions()->Option<(u32,u32)> {crate::OUTPUT.get()}}
mod ui4 {
    pub fn output_dimensions()->Option<(u32,u32)> {crate::OUTPUT.get()}
    #[path="__METRICS__"] mod display_metrics;
    pub(crate) use display_metrics::physical_extent_pixels_fraction;
}
'''.replace('__METRICS__', metrics)
    for name in ('FWIDTH', 'FHEIGHT'):
        source += extract.constant('vendor/microfont/src/lib.rs', name)
    for name in ('MIN_COLUMNS', 'MIN_ROWS'):
        source += extract.constant('src/shell3/shell3.rs', name)
    source += '\nmod r {pub mod services {pub mod shell3_font_service {\n'
    source += extract.constant('src/r/services/shell3_font_service.rs', 'DEFAULT_UI4_FONT_SCALE')
    source += extract.item('src/r/services/shell3_font_service.rs', 'cell_extent')
    source += '\n}}}\nmod show {\n'
    source += '#[derive(Clone,Copy)] pub struct WindowGeometry {pub width:u32,pub height:u32}\n'
    source += 'pub struct Show {pub font_scale:u32,pub initial_geometry:Option<WindowGeometry>}\nimpl Show {\n'
    method = re.search(r'^    pub\(crate\) fn initial_grid\(.*?^    }',
                       (ROOT / 'src/shell3/show/show.rs').read_text(), re.M | re.S)
    assert method is not None
    source += method.group()
    source += '\n}\n'
    source += extract.item('src/shell3/show/show.rs', 'show_extent').replace('fn show_extent(', 'pub(crate) fn show_extent(')
    source += '\n}\n' + TESTS
    with tempfile.TemporaryDirectory(prefix='shell3-physical-cells-') as directory:
        work = Path(directory)
        rust = work / 'tests.rs'
        rust.write_text(source)
        subprocess.run(['rustc', '--edition=2024', '--test', '-Adead_code', str(rust),
                        '-o', str(work / 'tests')], check=True)
        subprocess.run([str(work / 'tests')], check=True)


if __name__ == '__main__':
    main()
