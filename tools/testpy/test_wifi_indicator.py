#!/usr/bin/env python3
"""Check retained Wi-Fi square writes, clipping, pitch padding and colors."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
text = (ROOT / 'src/r/services/microfont_log_service.rs').read_text()
start = text.index('fn paint_wifi_square(')
end = text.index('\nfn repaint(', start)
ip_start = text.index('fn paint_wifi_ip(')
ip_end = text.index('\nfn paint_wifi_square(', ip_start)
source = r'''
#![allow(dead_code)]
use core::sync::atomic::AtomicU32;
static WIFI_IP_OCTET: core::sync::atomic::AtomicU16 = core::sync::atomic::AtomicU16::new(256);
static WIFI_COLOR: AtomicU32 = AtomicU32::new(0xff00_00ff);
#[derive(Clone, Copy)]
struct Surface { virt: usize, width: u32, height: u32, pitch_bytes: u32 }
mod intel {
    pub fn dma_flush_strided_rows(_: *mut u8, _: usize, _: usize, _: usize) -> bool { true }
}
PAINTER
IP_PAINTER
#[test]
fn square_preserves_all_surrounding_pixels_and_padding() {
    const GUARD: u32 = 0x12345678;
    for (width, height) in [(128, 96), (64, 64), (37, 21)] {
        let pitch = width + 8;
        let mut pixels = vec![GUARD; pitch * height + 2];
        let surface = Surface {
            virt: unsafe { pixels.as_mut_ptr().add(1) } as usize,
            width: width as u32, height: height as u32, pitch_bytes: (pitch * 4) as u32,
        };
        for color in [0xff00_00ff, 0xffff_0000, 0xff00_ff00] {
            WIFI_COLOR.store(color, core::sync::atomic::Ordering::Relaxed);
            paint_wifi_square(surface);
            assert_eq!(pixels[0], GUARD);
            assert_eq!(*pixels.last().unwrap(), GUARD);
            for y in 0..height {
                for x in 0..pitch {
                    let expected = if y < height.min(64) && x >= width.saturating_sub(64) && x < width {
                        color
                    } else { GUARD };
                    assert_eq!(pixels[1 + y * pitch + x], expected, "pixel {x},{y}");
                }
            }
        }
    }
}
#[test]
fn ip_row_is_msb_first_dot_or_line_and_clears_when_address_disappears() {
    const GUARD: u32 = 0x12345678;
    for (width, height) in [(160usize, 100usize), (37, 80), (37, 21)] {
        let pitch = width + 8;
        let mut pixels = vec![GUARD; pitch * height + 2];
        let surface = Surface {
            virt: unsafe { pixels.as_mut_ptr().add(1) } as usize,
            width: width as u32, height: height as u32, pitch_bytes: (pitch * 4) as u32,
        };
        for octet in [90u16, 0, 255, 256] {
            WIFI_IP_OCTET.store(octet, core::sync::atomic::Ordering::Relaxed);
            paint_wifi_ip(surface);
            let left = width.saturating_sub(128);
            for y in 0..height {
                for x in 0..pitch {
                    let expected = if (72..88).contains(&y) && x >= left && x < width {
                        let cell = (x - left) / 16;
                        let dx = (x - left) % 16;
                        let dy = y - 72;
                        let one = octet < 256 && (octet >> (7 - cell)) & 1 == 1;
                        let ink = octet < 256 && (7..10).contains(&dx)
                            && if one { (2..14).contains(&dy) } else { (11..14).contains(&dy) };
                        if ink { 0xff00_0000 } else { 0xffff_ffff }
                    } else { GUARD };
                    assert_eq!(pixels[1 + y * pitch + x], expected, "octet {octet}, pixel {x},{y}");
                }
            }
            assert_eq!(pixels[0], GUARD);assert_eq!(*pixels.last().unwrap(), GUARD);
        }
    }
}
'''.replace('IP_PAINTER', text[ip_start:ip_end]).replace('PAINTER', text[start:end])
with tempfile.TemporaryDirectory(prefix='trueos-wifi-indicator-') as temporary:
    directory = Path(temporary)
    (directory / 'test.rs').write_text(source)
    subprocess.run(['rustc', '--edition=2024', '--test', str(directory / 'test.rs'), '-o', str(directory / 'test')], check=True)
    subprocess.run([str(directory / 'test')], check=True)
