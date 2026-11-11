#![allow(dead_code)]
extern crate alloc;

use std::cell::RefCell;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct WindowId(u32);
impl WindowId { fn raw(self) -> u32 { self.0 } }
#[derive(Clone, Copy, Debug)]
struct WindowPlacement { x: i32, y: i32, width: u32, height: u32 }
#[derive(Clone, Copy, Debug)]
struct WindowSnapshot {
    id: WindowId, frame: u64, placement: WindowPlacement, revision: u64, publish_serial: u64,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FramePoolError { Closed, ViewUnavailable }
#[derive(Clone)]
struct Frame { width: u32, height: u32, pitch: u32, bytes: Vec<u8> }
#[derive(Default)]
struct Pool {
    frames: BTreeMap<u64, Frame>,
    pinned: BTreeMap<u64, Frame>,
    acquired: Vec<u64>, released: Vec<u64>, flushed: usize,
    close_after_acquire: bool, replace_after_acquire: bool, fail_view: bool,
}
thread_local! { static POOL: RefCell<Pool> = RefCell::new(Pool::default()); }
fn pool<R>(f: impl FnOnce(&mut Pool) -> R) -> R { POOL.with(|p| f(&mut p.borrow_mut())) }
fn acquire_published_frame(frame: u64) -> Result<u64, FramePoolError> {
    pool(|p| {
        p.acquired.push(frame);
        let pinned = p.frames.get(&frame).ok_or(FramePoolError::Closed)?.clone();
        p.pinned.insert(frame, pinned);
        if p.close_after_acquire { p.frames.remove(&frame); }
        if p.replace_after_acquire { p.frames.get_mut(&frame).unwrap().bytes.fill(231); }
        Ok(frame)
    })
}
struct FrameRgbaView { width: u32, height: u32, pitch: u32, virt: *mut u8, byte_len: usize }
fn published_rgba_view(lease: u64) -> Result<FrameRgbaView, FramePoolError> {
    pool(|p| {
        if p.fail_view { return Err(FramePoolError::ViewUnavailable); }
        let f = p.pinned.get_mut(&lease).ok_or(FramePoolError::Closed)?;
        Ok(FrameRgbaView { width: f.width, height: f.height, pitch: f.pitch,
            virt: f.bytes.as_mut_ptr(), byte_len: f.bytes.len() })
    })
}
fn release_published_frame(lease: u64) -> Result<(), FramePoolError> {
    pool(|p| { p.released.push(lease); p.pinned.remove(&lease); }); Ok(())
}
mod intel { pub fn dma_flush(_: *mut u8, _: usize) { super::pool(|p| p.flushed += 1); } }
mod graphics { pub mod encoder { #[path="@PNG@"] pub mod png; } }
mod compositor_service {
    pub fn presentation_placement(window: super::WindowSnapshot, _: u32, _: u32) -> super::WindowPlacement {
        window.placement
    }
}
mod capture {
    use super::{WindowSnapshot, FramePoolError, acquire_published_frame, published_rgba_view, release_published_frame};
    @PRODUCTION@
    #[cfg(test)] mod tests {
        use super::*;
        use crate::{Frame, FramePoolError, Pool, WindowId, WindowPlacement, WindowSnapshot, pool};

        fn window() -> WindowSnapshot {
            WindowSnapshot { id: WindowId(7), frame: 17, revision: 91, publish_serial: 123,
                placement: WindowPlacement { x: 140, y: 100, width: 63, height: 63 } }
        }
        fn reset(alpha: u8) -> Vec<u8> {
            let mut frame = Frame { width: 63, height: 63, pitch: 63 * 4 + 16,
                bytes: vec![197; (63 * 4 + 16) * 63 + 64] };
            let pixel = if alpha == 0 { [0, 0, 0, 0] } else { [19, 71, 129, alpha] };
            let expected = pixel.repeat(63 * 63);
            for y in 0..63 {
                frame.bytes[y * frame.pitch as usize..y * frame.pitch as usize + 63 * 4]
                    .copy_from_slice(&pixel.repeat(63));
            }
            pool(|p| {
                *p = Pool::default();
                p.frames.insert(17, frame.clone());
                // Another window/desktop frame occupies the same rectangle but
                // has distinct sentinel pixels. No path may acquire it.
                frame.bytes.fill(231); p.frames.insert(99, frame);
            });
            expected
        }
        fn png_rgb(png: &[u8]) -> Vec<u8> {
            assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
            let mut position = 8; let mut compressed = Vec::new();
            while position < png.len() {
                let length = u32::from_be_bytes(png[position..position + 4].try_into().unwrap()) as usize;
                if &png[position + 4..position + 8] == b"IDAT" {
                    compressed.extend_from_slice(&png[position + 8..position + 8 + length]);
                }
                position += 12 + length;
            }
            let rows = miniz_oxide::inflate::decompress_to_vec_zlib(&compressed).unwrap();
            let mut rgb = Vec::new();
            for row in rows.chunks_exact(63 * 3 + 1) {
                assert_eq!(row[0], 0); rgb.extend_from_slice(&row[1..]);
            }
            rgb
        }
        #[test]
        fn exact_frame_copy_excludes_occluders_pitch_padding_and_trailing_allocation_bytes() {
            let expected = reset(255);
            let frame = capture_window_rgba(window()).unwrap();
            assert_eq!((frame.width, frame.height), (63, 63));
            assert_eq!(frame.rgba_premultiplied, expected);
            pool(|p| { assert_eq!(p.acquired, [17]); assert_eq!(p.released, [17]); assert!(p.pinned.is_empty()); });
        }
        #[test]
        fn png_observation_keeps_exact_window_metadata_and_only_its_pixels() {
            reset(255);
            let observation = capture_compact_window_observation(window()).unwrap();
            assert_eq!(observation.window_id, WindowId(7));
            assert_eq!((observation.revision, observation.publish_serial), (91, 123));
            assert_eq!((observation.capture_width, observation.capture_height), (63, 63));
            let rgb = png_rgb(&observation.png);
            // Interior point is outside the coordinate grid/labels.
            assert_eq!(&rgb[(3 * 63 + 3) * 3..(3 * 63 + 3) * 3 + 3], &[19, 71, 129]);
            pool(|p| assert_eq!(p.acquired, [17]));
        }
        #[test]
        fn transparent_pixels_use_fixed_background_instead_of_desktop_pixels() {
            reset(0);
            let observation = capture_compact_window_observation(window()).unwrap();
            let rgb = png_rgb(&observation.png);
            assert_eq!(&rgb[(3 * 63 + 3) * 3..(3 * 63 + 3) * 3 + 3], &COMPACT_WINDOW_BACKGROUND);
        }
        #[test]
        fn closed_or_replanned_frame_keeps_its_pinned_pixels_and_never_falls_back() {
            for close in [true, false] {
                let expected = reset(255);
                pool(|p| { p.close_after_acquire = close; p.replace_after_acquire = !close; });
                assert_eq!(capture_window_rgba(window()).unwrap().rgba_premultiplied, expected);
                pool(|p| { assert_eq!(p.acquired, [17]); assert_eq!(p.released, [17]); });
            }
            reset(255); pool(|p| { p.frames.remove(&17); });
            assert!(matches!(capture_window_rgba(window()), Err(CaptureError::Frame(FramePoolError::Closed))));
            pool(|p| { assert_eq!(p.acquired, [17]); assert!(p.released.is_empty()); });
        }
        #[test]
        fn unavailable_view_or_invalid_pitch_releases_lease_without_returning_pixels() {
            reset(255); pool(|p| p.fail_view = true);
            assert!(matches!(capture_window_rgba(window()), Err(CaptureError::Frame(FramePoolError::ViewUnavailable))));
            pool(|p| assert_eq!(p.released, [17]));
            reset(255); pool(|p| p.frames.get_mut(&17).unwrap().pitch = 1);
            assert!(matches!(capture_window_rgba(window()), Err(CaptureError::InvalidFrameLayout)));
            pool(|p| assert_eq!(p.released, [17]));
        }
        #[test]
        fn row_copy_rejects_incomplete_or_empty_frames() {
            assert!(copy_window_rgba_rows(0, 1, 4, &[0; 4]).is_err());
            assert!(copy_window_rgba_rows(1, 0, 4, &[0; 4]).is_err());
            assert!(copy_window_rgba_rows(2, 2, 8, &[0; 15]).is_err());
            assert_eq!(copy_window_rgba_rows(1, 2, 8, &[1,2,3,4,99,99,99,99,5,6,7,8]).unwrap(), [1,2,3,4,5,6,7,8]);
        }
    }
}
