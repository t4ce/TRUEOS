#!/usr/bin/env python3
"""Exercise Hull's real admission policy and image codecs on the host.

The kernel is freestanding. This harness compiles its production policy and
PNG/JPEG/BMP/ICO/WebP decoders, plus real service admission and cache checks. Worker
scheduling, UI ownership, and filesystem operations use deterministic mocks.
It does not use the network, TRUEOS storage, or graphics hardware.
Run with ``python3 tools/testpy/test_hull_image_policy.py``.
"""

import json
import base64
import os
from pathlib import Path
import re
import struct
import subprocess
import tempfile
import zlib


ROOT = Path(__file__).resolve().parents[2]
DECODERS = ROOT / "crates/trueos-graphics/decoder"


def chunk(kind: bytes, body: bytes) -> bytes:
    return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body))


def png(width: int, height: int, color_type: int, rows: bytes, *, depth: int = 8,
        before_data: bytes = b"") -> bytes:
    header = struct.pack(">IIBBBBB", width, height, depth, color_type, 0, 0, 0)
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + before_data
            + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b""))


def production_item(source: str, name: str) -> str:
    declaration = re.search(rf"^(?:pub(?:\([^)]*\))? )?(?:async )?fn {name}\b", source, re.MULTILINE)
    if declaration is None:
        raise ValueError(f"missing production function {name}")
    end = re.search(r"^}\s*\n", source[declaration.end():], re.MULTILINE)
    if end is None:
        raise ValueError(f"missing closing brace for {name}")
    return source[declaration.start():declaration.end() + end.end()]


TESTS = r'''
#[cfg(test)]
mod integration_tests {
    use super::*;
    use graphics::{bmp, jpeg, png, ico};

    const EXPECTED: &[u8] = &[255,0,0,255, 0,255,0,128, 0,0,255,0, 255,255,255,255];

    fn ico_with_png(png: &[u8], width: u8, height: u8) -> Vec<u8> {
        let mut bytes=vec![0u8;22];bytes[2]=1;bytes[4]=1;bytes[6]=width;bytes[7]=height;
        bytes[14..18].copy_from_slice(&(png.len() as u32).to_le_bytes());
        bytes[18..22].copy_from_slice(&22u32.to_le_bytes());bytes.extend_from_slice(png);bytes
    }
    #[test]
    fn ico_png_frame_reuses_bounded_decoder_and_preserves_alpha() {
        let bytes=ico_with_png(include_bytes!("rgba.png"),2,2);
        let info=policy::preflight(&bytes).unwrap();
        assert_eq!((info.format,info.width,info.height),(4,2,2));
        let image=ico::decode_ico_rgba(&bytes).unwrap();
        assert!(image.png_frame);assert_eq!(image.rgba,EXPECTED);
        let admitted=media::decode_ui4_bounded(&bytes).unwrap();
        assert_eq!((admitted.info.source_format,admitted.info.backend,admitted.info.width,admitted.info.height),(4,media::BACKEND_PNG,2,2));
        assert_eq!(admitted.rgba,EXPECTED);
        assert_eq!(policy::extension(4),Some("ico"));
        let metadata=policy::metadata("https://example.test/favicon.ico",4,&bytes);
        assert_eq!(policy::cache_descriptor(metadata.as_bytes(),"https://example.test/favicon.ico").map(|item|item.0),Some(4));
    }
    #[test]
    fn ico_256_png_uses_zero_directory_extent_without_extra_frame_allocation() {
        let bytes=ico_with_png(include_bytes!("large-icon.png"),0,0);
        let image=media::decode_ui4_bounded(&bytes).unwrap();
        assert_eq!((image.info.width,image.info.height,image.info.source_format),(256,256,4));
        assert_eq!(image.rgba.len(),256*256*4);
        assert!(image.rgba.chunks_exact(4).all(|p|p==[11,22,33,128]));
    }

    #[test]
    fn ico_png_rejects_animation_mismatched_directory_and_oversize_before_decode() {
        for bytes in [ico_with_png(include_bytes!("animated.png"),2,2),ico_with_png(include_bytes!("duplicate-header.png"),2,2),ico_with_png(include_bytes!("rgba.png"),16,16),ico_with_png(include_bytes!("oversized.png"),0,0)] {
            assert!(policy::preflight(&bytes).is_err());assert!(ico::decode_ico_rgba(&bytes).is_err());
        }
    }
    #[test]
    fn raster_downsize_contains_without_distortion_or_upscale() {
        assert_eq!(policy::contained_extent(256,128,28,28),Ok((28,14)));
        assert_eq!(policy::contained_extent(128,256,28,28),Ok((14,28)));
        assert_eq!(policy::contained_extent(16,16,28,28),Ok((16,16)));
        assert_eq!(policy::contained_extent(1,256,28,28),Ok((1,28)));
        assert_eq!(policy::contained_extent(256,1,28,28),Ok((28,1)));
        assert!(policy::contained_extent(0,256,28,28).is_err());
        assert_eq!(policy::contained_extent(u32::MAX,u32::MAX,28,28),Ok((28,28)));
    }

    #[test]
    fn static_webp_lossless_rgb_and_alpha_decode_by_magic() {
        for (name,bytes,expected) in [
            ("RGB",include_bytes!("webp-lossless-rgb.bin").as_slice(),[255,0,0,255,0,255,0,255,0,0,255,255,11,22,33,255]),
            ("alpha",include_bytes!("webp-lossless-alpha.bin").as_slice(),[0,0,0,0,10,20,30,64,40,50,60,128,70,80,90,255]),
        ] {
            let image=media::decode_ui4_bounded(bytes).unwrap_or_else(|e|panic!("WebP {name} failed {e}"));
            assert_eq!((image.info.source_format,image.info.width,image.info.height),(5,2,2));
            assert_eq!(image.rgba,expected,"WebP {name}");
            assert_eq!(policy::extension(5),Some("webp"));
            let metadata=policy::metadata("https://fixture.test/misleading.jpg",5,bytes);
            assert_eq!(policy::cache_descriptor(metadata.as_bytes(),"https://fixture.test/misleading.jpg").map(|v|v.0),Some(5));
        }
    }
    #[test]
    fn static_webp_vp8_lossy_rgb_and_extended_alpha_decode() {
        let opaque=media::decode_ui4_bounded(include_bytes!("webp-lossy-rgb.bin")).unwrap();
        assert_eq!((opaque.info.width,opaque.info.height),(2,2));
        assert!(opaque.rgba.chunks_exact(4).all(|pixel|pixel[3]==255));
        let alpha=media::decode_ui4_bounded(include_bytes!("webp-lossy-alpha.bin")).unwrap();
        assert_eq!((alpha.info.width,alpha.info.height),(2,2));
        assert_eq!(alpha.rgba.chunks_exact(4).map(|pixel|pixel[3]).collect::<Vec<_>>(),[0,64,128,255]);
    }
    #[test]
    fn webp_admission_rejects_animation_ranges_duplicate_frames_and_unadmitted_extents() {
        let original=include_bytes!("webp-lossy-alpha.bin");
        for length in 0..original.len(){assert!(policy::preflight(&original[..length]).is_err());}
        let mut bytes=original.to_vec();bytes[20]|=2;assert_eq!(policy::preflight(&bytes),Err(policy::ERR_UNSUPPORTED));
        let mut bytes=original.to_vec();bytes[24..27].fill(0xff);assert_eq!(policy::preflight(&bytes),Err(policy::ERR_TOO_LARGE));
        let mut bytes=original.to_vec();bytes[24]=2;assert_eq!(policy::preflight(&bytes),Err(policy::ERR_INVALID));
        let mut bytes=original.to_vec();bytes[16..20].copy_from_slice(&u32::MAX.to_le_bytes());assert_eq!(policy::preflight(&bytes),Err(policy::ERR_INVALID));
        let mut bytes=include_bytes!("webp-lossy-rgb.bin").to_vec();
        bytes.extend_from_slice(&include_bytes!("webp-lossy-rgb.bin")[12..]);
        let size=bytes.len() as u32-8;bytes[4..8].copy_from_slice(&size.to_le_bytes());
        assert_eq!(policy::preflight(&bytes),Err(policy::ERR_INVALID));
        let mut bytes=include_bytes!("webp-lossless-rgb.bin").to_vec();
        let bits=8191u32|(8191u32<<14);bytes[21..25].copy_from_slice(&bits.to_le_bytes());
        assert_eq!(policy::preflight(&bytes),Err(policy::ERR_TOO_LARGE));
    }

    #[test]
    fn png_decodes_real_rgba_without_flattening_transparency() {
        let bytes = include_bytes!("rgba.png");
        assert_eq!(policy::preflight(bytes).unwrap().format, 2);
        let image = png::decode_png_rgba_bounded(bytes, policy::MAX_SOURCE_BYTES, 8192).unwrap();
        assert_eq!((image.width, image.height), (2,2));
        assert_eq!(image.rgba, EXPECTED);
        assert!(png::decode_png_rgba_bounded(bytes, 4, 8192).is_err());
        assert!(png::decode_png_rgba_bounded(bytes, 32, 1).is_err());
    }

    #[test]
    fn optional_live_svg_proxy_pngs_pass_production_decode_with_transparency() {
        let Some(directory)=std::env::var_os("HULL_SVG_LIVE_FIXTURES") else {return;};
        for name in ["commons-logo.png","wikimedia-footer.png"] {
            let bytes=std::fs::read(std::path::Path::new(&directory).join(name)).unwrap();
            let decoded=media::decode_ui4_bounded(&bytes).unwrap();
            assert_eq!(decoded.info.source_format,2);
            assert!(decoded.info.width<=128 && decoded.info.height<=128);
            let zero=decoded.rgba.chunks_exact(4).filter(|p|p[3]==0).count();
            let partial=decoded.rgba.chunks_exact(4).filter(|p|p[3]>0 && p[3]<255).count();
            let opaque=decoded.rgba.chunks_exact(4).filter(|p|p[3]==255).count();
            assert!(zero>0 && partial>0 && opaque>0,"{name} lost transparent or antialiased pixels");
            println!("live SVG PNG {name}: {}x{} bytes={} transparent={zero} partial={partial} opaque={opaque}",decoded.info.width,decoded.info.height,bytes.len());
        }
    }

    #[test]
    fn optional_wikipedia_logos_pass_the_actual_bounded_media_decoder() {
        let Some(directory)=std::env::var_os("HULL_WIKIPEDIA_LOGO_FIXTURES") else {return;};
        let directory=std::path::Path::new(&directory);
        let conversion=crate::hull_svg_policy::conversion_url(
            "https://de.wikipedia.org/static/images/mobile/copyright/wikipedia-wordmark-en.svg",120,18).unwrap();
        assert!(std::fs::read_to_string(directory.join("http.json")).unwrap().contains(&conversion));
        for (name,width,height) in [
            ("portal-ball.png",200,183),("article-ball.png",100,100),("article-wordmark.png",120,18),
        ] {
            let bytes=std::fs::read(directory.join(name)).unwrap();
            let info=policy::preflight(&bytes).unwrap();
            assert_eq!((info.format,info.width,info.height),(2,width,height),"{name} preflight");
            let decoded=media::decode_ui4_bounded(&bytes).unwrap_or_else(|e|panic!("{name}: {e}"));
            assert_eq!((decoded.info.source_format,decoded.info.width,decoded.info.height),(2,width,height),"{name} decode");
            assert_eq!(decoded.rgba.len(),width as usize*height as usize*4);
            let transparent=decoded.rgba.chunks_exact(4).filter(|p|p[3]==0).count();
            let partial=decoded.rgba.chunks_exact(4).filter(|p|p[3]>0 && p[3]<255).count();
            let opaque=decoded.rgba.chunks_exact(4).filter(|p|p[3]==255).count();
            assert!(transparent>0 && partial>0 && opaque>0,"{name} lost meaningful alpha");
            std::fs::write(directory.join(format!("{name}.rgba")),&decoded.rgba).unwrap();
            println!("Wikipedia PNG {name}: {}x{} encoded={} rgba={} transparent={transparent} partial={partial} opaque={opaque}",width,height,bytes.len(),decoded.rgba.len());
        }
    }

    #[test]
    fn png_indexed_unpacking_preserves_palette_alpha() {
        let bytes = include_bytes!("indexed.png");
        let image = png::decode_png_rgba_bounded(bytes, policy::MAX_SOURCE_BYTES, 8192).unwrap();
        assert_eq!((image.width, image.height), (2,1));
        assert_eq!(image.rgba, [255,0,0,255, 0,0,255,64]);
    }

    #[test]
    fn admission_rejects_animation_duplicate_headers_and_oversized_pngs() {
        assert!(policy::preflight(include_bytes!("animated.png")).is_err());
        assert!(policy::preflight(include_bytes!("duplicate-header.png")).is_err());
        assert!(policy::preflight(include_bytes!("oversized.png")).is_err());
        assert!(png::decode_png_rgba_bounded(include_bytes!("oversized.png"),
            policy::MAX_SOURCE_BYTES, 8192).is_err());
    }

    #[test]
    fn jpeg_decodes_real_source_and_checks_budget_before_pixels() {
        let bytes = include_bytes!("sample.jpg");
        let source = policy::preflight(bytes).unwrap();
        assert_eq!(source.format, 1);
        let image = jpeg::decode_jpeg_rgba_bounded(bytes, policy::MAX_SOURCE_BYTES, 8192)
            .unwrap_or_else(|error| panic!("JPEG decode failed: {}", error.code()));
        assert_eq!((image.width, image.height), (source.width, source.height));
        assert_eq!(image.rgba.len(), image.width as usize * image.height as usize * 4);
        assert!(image.rgba.chunks_exact(4).all(|pixel| pixel[3] == 255));
        assert!(jpeg::decode_jpeg_rgba_bounded(bytes, 4, 8192).is_err());
        assert!(jpeg::decode_jpeg_rgba_bounded(bytes, policy::MAX_SOURCE_BYTES, 1).is_err());
    }

    fn bmp_source(top_down: bool) -> Vec<u8> {
        let mut bytes = vec![0; 70];
        bytes[..2].copy_from_slice(b"BM");
        bytes[2..6].copy_from_slice(&70u32.to_le_bytes());
        bytes[10..14].copy_from_slice(&54u32.to_le_bytes());
        bytes[14..18].copy_from_slice(&40u32.to_le_bytes());
        bytes[18..22].copy_from_slice(&2i32.to_le_bytes());
        bytes[22..26].copy_from_slice(&(if top_down { -2i32 } else { 2 }).to_le_bytes());
        bytes[26..28].copy_from_slice(&1u16.to_le_bytes());
        bytes[28..30].copy_from_slice(&24u16.to_le_bytes());
        let top = [0,0,255, 0,255,0, 0,0];
        let bottom = [255,0,0, 255,255,255, 0,0];
        bytes[54..62].copy_from_slice(if top_down { &top } else { &bottom });
        bytes[62..70].copy_from_slice(if top_down { &bottom } else { &top });
        bytes
    }

    #[test]
    fn bmp_projection_handles_both_row_directions_and_rejects_compression() {
        for top_down in [false,true] {
            let mut bytes = bmp_source(top_down);
            assert_eq!(policy::preflight(&bytes).unwrap().format, 3);
            let image = bmp::decode_bmp_rgba(&bytes).unwrap();
            assert_eq!((image.width, image.height), (2,2));
            assert_eq!(image.rgba, [255,0,0,255, 0,255,0,255, 0,0,255,255, 255,255,255,255]);
            bytes[30..34].copy_from_slice(&1u32.to_le_bytes());
            assert!(bmp::decode_bmp_rgba(&bytes).is_err());
        }
    }

    #[test]
    fn cached_payload_identity_detects_corruption_and_query_changes() {
        let url = "https://example.test/a.png?variant=1";
        let bytes = include_bytes!("rgba.png");
        let metadata = policy::metadata(url, 2, bytes);
        let (format, length, hash) = policy::cache_descriptor(metadata.as_bytes(), url).unwrap();
        assert_eq!((format,length), (2,bytes.len()));
        assert_eq!(hash, policy::digest(bytes));
        let mut corrupt = bytes.to_vec(); corrupt[30] ^= 1;
        assert_ne!(hash, policy::digest(&corrupt));
        assert!(policy::cache_descriptor(metadata.as_bytes(),
            "https://example.test/a.png?variant=2").is_none());
    }

    #[test]
    fn truncated_sources_cannot_pass_both_admission_and_decode() {
        for bytes in [include_bytes!("rgba.png").as_slice(), include_bytes!("sample.jpg").as_slice()] {
            for len in 0..bytes.len().min(128) {
                let prefix = &bytes[..len];
                if bytes.starts_with(b"\x89PNG") {
                    // PNG may finish decoding pixels before seeing IEND; the
                    // service's complete-source preflight must reject that tail.
                    assert!(policy::preflight(prefix).is_err()
                        || png::decode_png_rgba_bounded(prefix, policy::MAX_SOURCE_BYTES, 8192).is_err());
                } else {
                    assert!(jpeg::decode_jpeg_rgba_bounded(prefix, policy::MAX_SOURCE_BYTES, 8192).is_err());
                }
            }
        }
    }

    #[test]
    fn random_headers_and_extreme_targets_never_panic_or_escape_budget() {
        let mut random = 0x4d595df4d0f33173u64;
        for len in 0..512 {
            let mut bytes = vec![0;len];
            for b in &mut bytes {
                random ^= random << 13; random ^= random >> 7; random ^= random << 17;
                *b = random as u8;
            }
            for magic in [b"BM".as_slice(), b"\xff\xd8".as_slice(), b"\x89PNG\r\n\x1a\n".as_slice(), b"\0\0\x01\0".as_slice(),b"RIFF".as_slice()] {
                if bytes.len() >= magic.len() { bytes[..magic.len()].copy_from_slice(magic); }
                let _ = policy::preflight(&bytes);
            }
            let (w,h,charged) = policy::target_extent((random >> 32) as u32 | 1, random as u32 | 1).unwrap();
            assert!(w > 0 && h > 0 && w <= 2048 && h <= 2048);
            assert!(charged <= policy::MAX_TARGET_BYTES && charged % 4096 == 0);
        }
    }
}
'''


SERVICE_TESTS = r'''
#[cfg(test)]
mod service {
    use alloc::{collections::VecDeque, string::String, sync::Arc, vec::Vec};
    use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use spin::Mutex;
    use crate::policy::{self, ERR_INVALID, ERR_TOO_LARGE, MAX_ENCODED_BYTES};
    use crate::r::fs::request_broker as fs;
    use crate::ui;
    use crate::test_time::{Duration,Timer};
    #[derive(Clone, Copy, Default)]
    pub(crate) struct ImageInfo { state:u32, sprite_id:u32, width:u32, height:u32, generation:u64, error:i32, reserved:u32 }
    SERVICE_CONSTANTS
    static NEXT_ID: AtomicU32 = AtomicU32::new(1);
    static REGISTRY: Mutex<Registry> = Mutex::new(Registry::new());
    static DECODING: AtomicBool = AtomicBool::new(false);
    SERVICE_DECODE_PERMIT
    SERVICE_TYPES
    fn ensure_workers() -> Result<(),i32> { Ok(()) }
    fn cache_scope(_: &str, _: &str, _: &str) -> Result<Option<Cache>,i32> { Ok(None) }
    SERVICE_FUNCTIONS
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    fn setup() -> std::sync::MutexGuard<'static,()> {
        let guard = TEST_LOCK.lock().unwrap();
        *REGISTRY.lock() = Registry::new();
        NEXT_ID.store(1, Ordering::Relaxed);
        crate::WARNINGS.lock().unwrap().clear();
        crate::hv::RUN.store(1, Ordering::Relaxed);
        ui::INCARNATION.store(1, Ordering::Relaxed);
        ui::BUSY.store(false, Ordering::Relaxed);
        ui::RELEASES.store(0, Ordering::Relaxed);
        fs::FILES.lock().clear(); fs::READS.store(0, Ordering::Relaxed);
        fs::FORCE_LENGTH.store(0, Ordering::Relaxed);
        DECODING.store(false,Ordering::Relaxed);
        crate::r::net::https::REQUESTS.lock().clear();
        crate::r::net::https::REPLIES.lock().clear();
        crate::r::net::https::CANCEL_AFTER_FETCH.store(false,Ordering::Relaxed);
        crate::hull_svg_cache::VALUES.lock().clear();
        crate::hull_svg_cache::WRITES.lock().clear();
        crate::hull_svg_cache::FAIL_WRITE.store(false,Ordering::Relaxed);
        ui::INSTALLS.store(0,Ordering::Relaxed);
        ui::INSTALL_ERROR.store(0,Ordering::Relaxed);
        ui::PREPARES.store(0,Ordering::Relaxed);
        ui::PREPARE_BUSY.store(0,Ordering::Relaxed);
        ui::PREPARE_ERROR.store(0,Ordering::Relaxed);
        ui::CANCEL_PREPARE.store(false,Ordering::Relaxed);
        guard
    }
    fn admit(owner:u32, window:u32, url:&str, w:u32, h:u32, generation:u64) -> Result<i32,i32> {
        request_inner(owner, window, url, "apps/hull", "example.test", w,h,generation)
    }

    #[test]
    fn admission_deduplicates_ready_images_and_bounds_window_memory() {
        let _guard = setup();
        let url = "https://example.test/0.png";
        let first = admit(7,1,url,2048,2048,9).unwrap();
        {
            let mut r = REGISTRY.lock();
            r.operations[0].info.state=1;
            r.operations[0].info.width=1; r.operations[0].info.height=1;
        }
        assert_eq!(admit(7,1,url,2048,2048,9), Ok(first));
        let reservation=policy::target_extent(2048,2048).unwrap().2;
        let capacity=WINDOW_BYTES/reservation;
        assert!(capacity>0 && capacity<42);
        for i in 1..capacity { admit(7,1,&format!("https://example.test/{i}.png"),2048,2048,9).unwrap(); }
        assert_eq!(admit(7,1,"https://example.test/overflow.png",2048,2048,9),Err(ERR_BUSY));
        let r = REGISTRY.lock();
        assert_eq!(r.operations.len(),capacity);
        assert!(r.operations.iter().map(|op|op.reserved_bytes).sum::<usize>()<=WINDOW_BYTES);
    }

    #[test]
    fn admission_accepts_42_small_images_and_reuses_a_released_slot() {
        let _guard = setup();
        let url = "https://example.test/0.png";
        let first = admit(7,1,url,20,10,9).unwrap() as u32;
        for i in 1..42 {
            admit(7,1,&format!("https://example.test/{i}.png"),20,10,9).unwrap();
        }
        assert_eq!(REGISTRY.lock().operations.len(),42);
        assert_eq!(admit(7,1,url,20,10,9),Ok(first as i32));
        assert_eq!(admit(7,1,"https://example.test/overflow.png",20,10,9),Err(ERR_BUSY));
        assert_eq!(release(7,1,first),0);
        assert!(admit(7,1,"https://example.test/overflow.png",20,10,9).is_ok());
        assert_eq!(REGISTRY.lock().operations.len(),42);
    }

    #[test]
    fn admission_keeps_the_global_64_ticket_limit_across_windows() {
        let _guard = setup();
        for (window, count) in [(1,42),(2,22)] {
            for i in 0..count {
                admit(7,window,&format!("https://example.test/{i}.png"),20,10,9).unwrap();
            }
        }
        assert_eq!(REGISTRY.lock().operations.len(),64);
        assert_eq!(admit(7,2,"https://example.test/overflow.png",20,10,9),Err(ERR_BUSY));
    }

    #[test]
    fn admission_bounds_global_memory_and_window_registry() {
        let _guard = setup();
        for owner in [7,8] {
            for i in 0..8 { admit(owner,1,&format!("https://example.test/{i}.png"),2048,2048,9).unwrap(); }
        }
        let budget = REGISTRY.lock().operations.iter().map(|op|op.reserved_bytes).sum::<usize>();
        // A target may charge less than 4MiB after fitting; keep admitting only
        // while the real policy has space, then verify no overcommit occurred.
        let rejected=(0..8).filter(|i|admit(9,1,&format!("https://example.test/{i}.png"),2048,2048,9).is_err()).count();
        assert!(rejected>0);
        assert!(budget<=GLOBAL_BYTES);
        assert!(REGISTRY.lock().operations.iter().map(|op|op.reserved_bytes).sum::<usize>()<=GLOBAL_BYTES);
        *REGISTRY.lock()=Registry::new();
        for window in 1..=16 { admit(7,window,"https://example.test/a.png",1,1,9).unwrap(); }
        assert_eq!(admit(7,17,"https://example.test/a.png",1,1,9),Err(ERR_BUSY));
    }

    #[test]
    fn status_contention_is_retryable_and_wrong_owner_cannot_observe_or_release() {
        let _guard = setup();
        let id=admit(7,1,"https://example.test/a.png",20,10,9).unwrap() as u32;
        {
            let _registry=REGISTRY.lock();
            assert!(matches!(status(7,1,id),Err(ERR_BUSY)));
        }
        ui::BUSY.store(true,Ordering::Relaxed);
        assert!(matches!(status(7,1,id),Err(ERR_BUSY)));
        ui::BUSY.store(false,Ordering::Relaxed);
        assert!(matches!(status(8,1,id),Err(ERR_NOT_FOUND)));
        assert_eq!(release(8,1,id),ERR_NOT_FOUND);
        assert_eq!(status(7,1,id).unwrap().generation,9);
    }

    #[test]
    fn retirement_retries_authentication_contention_without_losing_the_source() {
        let _guard = setup();
        let id=admit(7,1,"https://example.test/a.png",20,10,9).unwrap() as u32;
        {
            let mut r=REGISTRY.lock();
            r.operations[0].info.state=1; r.operations[0].info.sprite_id=42;
        }
        ui::BUSY.store(true,Ordering::Relaxed);
        assert_eq!(release(7,1,id),ERR_BUSY);
        assert_eq!(REGISTRY.lock().operations.len(),1);
        assert_eq!(ui::RELEASES.load(Ordering::Relaxed),0);
        ui::BUSY.store(false,Ordering::Relaxed);
        assert_eq!(release(7,1,id),0);
        assert!(REGISTRY.lock().operations.is_empty());
        assert_eq!(ui::RELEASES.load(Ordering::Relaxed),1);
    }

    #[test]
    fn reset_cancels_queued_work_and_late_results_cannot_touch_reused_ticket() {
        let _guard = setup();
        let id=admit(7,1,"https://example.test/old.png",20,10,9).unwrap() as u32;
        let old=REGISTRY.lock().operations[0].life.clone();
        admit(8,2,"https://example.test/other.png",20,10,9).unwrap();
        assert_eq!(reset(7,1,10),0);
        assert!(old.cancelled.load(Ordering::Acquire));
        assert!(!old.live());
        assert_eq!(REGISTRY.lock().queue.len(),1);
        NEXT_ID.store(id,Ordering::Relaxed);
        let new=admit(7,1,"https://example.test/new.png",20,10,10).unwrap() as u32;
        assert_eq!(new,id);
        complete_error(&old,ERR_FAILED);
        assert_eq!(status(7,1,new).unwrap().state,0);
        assert_eq!(admit(7,1,"https://example.test/stale.png",20,10,9),Err(ERR_INVALID));
    }

    #[test]
    fn liveness_retries_busy_but_rejects_recycled_vm_and_window_incarnations() {
        let _guard = setup();
        admit(7,1,"https://example.test/a.png",20,10,9).unwrap();
        let life=REGISTRY.lock().operations[0].life.clone();
        assert!(life.live());
        ui::BUSY.store(true,Ordering::Relaxed); assert!(life.live());
        ui::BUSY.store(false,Ordering::Relaxed);
        ui::INCARNATION.store(2,Ordering::Relaxed); assert!(!life.live());
        ui::INCARNATION.store(1,Ordering::Relaxed);
        crate::hv::RUN.store(2,Ordering::Relaxed); assert!(!life.live());
    }

    #[test]
    fn complete_cache_hits_need_matching_url_length_digest_and_format() {
        let _guard = setup();
        let cache=Cache { disk:1,directory:"cache".into(),svg_database:"svg.redb".into() };
        let url="https://example.test/a.png?q=1";
        let body=include_bytes!("rgba.png");
        fs::FILES.lock().insert("cache/source.png".into(),body.to_vec());
        fs::FILES.lock().insert("cache/metadata".into(),policy::metadata(url,2,body).into_bytes());
        assert_eq!(read_cache(&cache,url).unwrap(),body);
        assert!(read_cache(&cache,"https://example.test/a.png?q=2").is_none());
        fs::FILES.lock().get_mut("cache/source.png").unwrap()[30]^=1;
        assert!(read_cache(&cache,url).is_none());
        fs::FILES.lock().remove("cache/metadata");
        assert!(read_cache(&cache,url).is_none());
    }

    #[test]
    fn oversized_cache_records_are_rejected_before_reading_their_body() {
        let _guard = setup();
        let cache=Cache { disk:1,directory:"cache".into(),svg_database:"svg.redb".into() };
        fs::FORCE_LENGTH.store(policy::MAX_ENCODED_BYTES+1,Ordering::Relaxed);
        assert!(read_bounded(&cache,"source.png",policy::MAX_ENCODED_BYTES).is_none());
        assert_eq!(fs::READS.load(Ordering::Relaxed),0);
    }

    fn job(url:&str)->Job {
        admit(7,1,url,128,128,9).unwrap();
        let mut job=REGISTRY.lock().queue.pop_front().unwrap();
        job.cache=Some(Cache {disk:1,directory:"cache".into(),svg_database:"svg.redb".into()});
        job
    }
    fn ready<T>(future:impl core::future::Future<Output=T>)->T {
        let mut future=Box::pin(future);
        let mut context=core::task::Context::from_waker(core::task::Waker::noop());
        match future.as_mut().poll(&mut context) {
            core::task::Poll::Ready(value)=>value,
            core::task::Poll::Pending=>panic!("deterministic service mocks unexpectedly suspended"),
        }
    }
    fn reply(url:&str,bytes:&[u8]) {
        crate::r::net::https::REPLIES.lock().insert(url.into(),Ok(bytes.to_vec()));
    }
    fn requested()->Vec<String> {
        crate::r::net::https::REQUESTS.lock().iter().map(|r|r.0.clone()).collect()
    }
    fn converted(job:&Job)->String {
        crate::hull_svg_policy::conversion_url(&job.url,job.width,job.height).unwrap()
    }

    #[test]
    fn image_errors_distinguish_network_decode_and_publication_without_url_secrets() {
        {
            let _guard=setup();let job=job("https://example.test/globe.png?token=secret");
            crate::r::net::https::REPLIES.lock().insert(job.url.clone(),Err("dns Timeout".into()));
            assert_eq!(ready(process(&job)),Err(ERR_FAILED));
            let logs=crate::WARNINGS.lock().unwrap();
            assert!(logs.iter().any(|s|s.contains("stage=fetch")&&s.contains("reason=dns Timeout")));
            assert!(logs.iter().all(|s|!s.contains("token=")&&!s.contains("secret")));
        }
        {
            let _guard=setup();let job=job("https://example.test/globe.png");reply(&job.url,b"invalid PNG");
            assert!(ready(process(&job)).is_err());
            assert!(crate::WARNINGS.lock().unwrap().iter().any(|s|s.contains("stage=decode")&&s.contains("encoded_bytes=11")));
        }
        {
            let _guard=setup();let job=job("https://example.test/globe.png");reply(&job.url,include_bytes!("rgba.png"));
            ui::INSTALL_ERROR.store(-6,Ordering::Relaxed);
            assert_eq!(ready(process(&job)),Err(-6));
            assert!(crate::WARNINGS.lock().unwrap().iter().any(|s|s.contains("stage=publish")&&s.contains("code=-6")));
        }
    }

    #[test]
    fn busy_upload_preparation_reuses_decoded_pixels_and_recovers_without_refetch() {
        let _guard=setup();let job=job("https://example.test/globe.png");
        reply(&job.url,include_bytes!("rgba.png"));
        ui::PREPARE_BUSY.store(2,Ordering::Relaxed);
        assert_eq!(ready(process(&job)),Ok(()));
        assert_eq!(ui::PREPARES.load(Ordering::Relaxed),3);
        assert_eq!(ui::INSTALLS.load(Ordering::Relaxed),1);
        assert_eq!(status(7,1,job.life.id).unwrap().state,1);
        assert_eq!(requested(),[job.url]);
        assert!(crate::WARNINGS.lock().unwrap().is_empty());
    }

    #[test]
    fn upload_preparation_retry_is_bounded_and_permanent_errors_fail_immediately() {
        for error in [ERR_BUSY,ERR_FAILED] {
            let _guard=setup();let job=job("https://example.test/globe.png");
            reply(&job.url,include_bytes!("rgba.png"));
            ui::PREPARE_ERROR.store(error,Ordering::Relaxed);
            assert_eq!(ready(process(&job)),Err(error));
            assert_eq!(ui::PREPARES.load(Ordering::Relaxed),if error==ERR_BUSY{500}else{1});
            assert_eq!(ui::INSTALLS.load(Ordering::Relaxed),0);
            assert!(crate::WARNINGS.lock().unwrap().iter().any(|s|s.contains("stage=prepare-upload")));
        }
    }

    #[test]
    fn cancelled_preparation_retry_never_publishes_or_refetches() {
        let _guard=setup();let job=job("https://example.test/globe.png");
        reply(&job.url,include_bytes!("rgba.png"));
        ui::PREPARE_BUSY.store(1,Ordering::Relaxed);
        ui::CANCEL_PREPARE.store(true,Ordering::Relaxed);
        assert_eq!(ready(process(&job)),Err(ERR_NOT_FOUND));
        assert_eq!(ui::PREPARES.load(Ordering::Relaxed),1);
        assert_eq!(ui::INSTALLS.load(Ordering::Relaxed),0);
        assert_eq!(requested(),[job.url]);
    }

    #[test]
    fn remote_svg_fetches_only_the_bounded_png_conversion() {
        let _guard=setup();let job=job("https://example.test/logo.svg?theme=dark&v=1");
        let proxy=converted(&job);reply(&proxy,include_bytes!("rgba.png"));
        let (bytes,key)=ready(encoded_source(&job)).unwrap();
        assert_eq!(bytes,include_bytes!("rgba.png"));assert_eq!(key,Some(proxy.clone()));
        assert_eq!(requested(),[proxy]);
        let requests=crate::r::net::https::REQUESTS.lock();
        assert_eq!((requests[0].1,requests[0].2),(FETCH_TIMEOUT_MS,MAX_ENCODED_BYTES));
        assert!(crate::hull_svg_cache::WRITES.lock().is_empty());
    }
    #[test]
    fn extensionless_svg_is_sniffed_then_converted_but_rasters_stay_native() {
        let _guard=setup();
        let svg=job("https://example.test/asset?id=svg");let proxy=converted(&svg);
        reply(&svg.url,b"<?xml version=\"1.0\"?><!-- logo --><svg xmlns=\"http://www.w3.org/2000/svg\"/>");
        reply(&proxy,include_bytes!("rgba.png"));
        assert_eq!(ready(encoded_source(&svg)).unwrap().1,Some(proxy.clone()));
        assert_eq!(requested(),[svg.url.clone(),proxy]);
        crate::r::net::https::REQUESTS.lock().clear();
        let raster=job("https://example.test/asset?id=png");reply(&raster.url,include_bytes!("rgba.png"));
        assert_eq!(ready(encoded_source(&raster)).unwrap(),(include_bytes!("rgba.png").to_vec(),None));
        assert_eq!(requested(),[raster.url]);
    }
    #[test]
    fn svg_cache_hit_avoids_network_and_missing_or_invalid_cache_retries_conversion() {
        for cached in [None,Some(b"corrupt entry".as_slice()),Some(include_bytes!("rgba.png").as_slice())] {
            let _guard=setup();let job=job("https://example.test/cached.svg");let proxy=converted(&job);
            if let Some(bytes)=cached {
                crate::hull_svg_cache::VALUES.lock().insert((1,"svg.redb".into(),proxy.clone()),bytes.to_vec());
            }
            reply(&proxy,include_bytes!("rgba.png"));
            let (bytes,key)=ready(encoded_source(&job)).unwrap();assert_eq!(bytes,include_bytes!("rgba.png"));
            if cached==Some(include_bytes!("rgba.png").as_slice()) {
                assert_eq!(key,None);assert!(requested().is_empty());
            } else {
                assert_eq!(key,Some(proxy.clone()));assert_eq!(requested(),[proxy]);
            }
        }
    }
    #[test]
    fn provider_error_documents_non_png_and_fetch_errors_are_not_converted_assets() {
        for body in [b"<!doctype html><html>provider error</html>".as_slice(),b"not a png",include_bytes!("sample.jpg")] {
            let _guard=setup();let job=job("https://example.test/logo.svg");reply(&converted(&job),body);
            assert!(ready(encoded_source(&job)).is_err());assert!(crate::hull_svg_cache::WRITES.lock().is_empty());
        }
        let _guard=setup();let job=job("https://example.test/logo.svg");
        crate::r::net::https::REPLIES.lock().insert(converted(&job),Err("HTTP429 provider rate limit".into()));
        assert_eq!(ready(encoded_source(&job)),Err(ERR_FAILED));
        assert!(crate::hull_svg_cache::WRITES.lock().is_empty());
    }
    #[test]
    fn png_conversion_is_persisted_only_after_real_decode_and_successful_publication() {
        let _guard=setup();let job=job("https://example.test/logo.svg");let proxy=converted(&job);
        reply(&proxy,include_bytes!("rgba.png"));
        assert_eq!(ready(process(&job)),Ok(()));
        assert_eq!(status(7,1,job.life.id).unwrap().state,1);
        assert_eq!(ui::INSTALLS.load(Ordering::Relaxed),1);
        assert_eq!(&*crate::hull_svg_cache::WRITES.lock(),&[(1,"svg.redb".into(),proxy,include_bytes!("rgba.png").to_vec())]);
    }
    #[test]
    fn conversion_decoder_or_publication_failure_and_stale_response_never_persist() {
        {
            let _guard=setup();let job=job("https://example.test/bad-pixels.svg");
            let malformed=include_bytes!("invalid-pixels.png");assert_eq!(policy::preflight(malformed).unwrap().format,2);
            reply(&converted(&job),malformed);
            assert!(ready(process(&job)).is_err());assert_eq!(ui::INSTALLS.load(Ordering::Relaxed),0);
            assert!(crate::hull_svg_cache::WRITES.lock().is_empty());
        }
        {
            let _guard=setup();let job=job("https://example.test/publication-failed.svg");
            reply(&converted(&job),include_bytes!("rgba.png"));ui::INSTALL_ERROR.store(ERR_FAILED,Ordering::Relaxed);
            assert_eq!(ready(process(&job)),Err(ERR_FAILED));assert!(crate::hull_svg_cache::WRITES.lock().is_empty());
        }
        {
            let _guard=setup();let job=job("https://example.test/cancelled.svg");
            reply(&converted(&job),include_bytes!("rgba.png"));
            crate::r::net::https::CANCEL_AFTER_FETCH.store(true,Ordering::Relaxed);
            assert_eq!(ready(process(&job)),Err(ERR_NOT_FOUND));assert!(crate::hull_svg_cache::WRITES.lock().is_empty());
            assert_eq!(ui::INSTALLS.load(Ordering::Relaxed),0);
        }
    }
    #[test]
    fn cache_write_failure_cannot_undo_an_already_published_svg() {
        let _guard=setup();let job=job("https://example.test/logo.svg");reply(&converted(&job),include_bytes!("rgba.png"));
        crate::hull_svg_cache::FAIL_WRITE.store(true,Ordering::Relaxed);
        assert_eq!(ready(process(&job)),Ok(()));assert_eq!(status(7,1,job.life.id).unwrap().state,1);
        assert_eq!(ui::INSTALLS.load(Ordering::Relaxed),1);assert_eq!(crate::hull_svg_cache::WRITES.lock().len(),1);
    }
}
'''


SERVICE_MOCKS = r'''
#[cfg(test)] mod hv {
    use core::sync::atomic::{AtomicU64,Ordering};
    pub static RUN: AtomicU64=AtomicU64::new(1);
    pub fn vm_run_generation(_:u8)->Option<u64> { Some(RUN.load(Ordering::Relaxed)) }
}
#[cfg(test)] mod ui {
    use core::sync::atomic::{AtomicBool,AtomicI32,AtomicU64,AtomicUsize,Ordering};
    pub static INCARNATION:AtomicU64=AtomicU64::new(1);
    pub static BUSY:AtomicBool=AtomicBool::new(false);
    pub static RELEASES:AtomicUsize=AtomicUsize::new(0);
    pub static INSTALLS:AtomicUsize=AtomicUsize::new(0);
    pub static INSTALL_ERROR:AtomicI32=AtomicI32::new(0);
    pub static PREPARES:AtomicUsize=AtomicUsize::new(0);
    pub static PREPARE_BUSY:AtomicUsize=AtomicUsize::new(0);
    pub static PREPARE_ERROR:AtomicI32=AtomicI32::new(0);
    pub static CANCEL_PREPARE:AtomicBool=AtomicBool::new(false);
    pub fn authenticate_window(_:u32,_:u32)->Result<u64,i32> {
        if BUSY.load(Ordering::Relaxed) {Err(-7)} else {Ok(INCARNATION.load(Ordering::Relaxed))}
    }
    pub fn release_rgba(_:u32,_:u32,_:u64,_:u32)->i32 { RELEASES.fetch_add(1,Ordering::Relaxed);0 }
    pub fn prepare_rgba(width:u32,height:u32,pixels:&[u8])->Result<(),i32>{
        assert_eq!(pixels.len(),width as usize*height as usize*4);
        PREPARES.fetch_add(1,Ordering::Relaxed);
        if CANCEL_PREPARE.load(Ordering::Relaxed){crate::hv::RUN.store(2,Ordering::Relaxed);}
        if PREPARE_BUSY.fetch_update(Ordering::Relaxed,Ordering::Relaxed,|n|n.checked_sub(1)).is_ok(){return Err(-7);}
        let error=PREPARE_ERROR.load(Ordering::Relaxed);if error!=0{return Err(error);}
        Ok(())
    }
    pub fn install_prepared(_:u32,_:u32,_:u64,_:u32,_:&mut ())->Result<u32,i32>{
        let error=INSTALL_ERROR.load(Ordering::Relaxed);if error!=0{return Err(error);}
        INSTALLS.fetch_add(1,Ordering::Relaxed);Ok(42)
    }
}
#[cfg(test)] mod test_time {
    pub use std::time::Duration;
    pub struct Timer;
    impl Timer {pub async fn after(_:Duration){}}
}
#[cfg(test)] mod hull_svg_cache {
    use alloc::{collections::BTreeMap,string::String,vec::Vec};
    use core::sync::atomic::{AtomicBool,Ordering};
    pub static VALUES:spin::Mutex<BTreeMap<(u32,String,String),Vec<u8>>>=spin::Mutex::new(BTreeMap::new());
    pub static WRITES:spin::Mutex<Vec<(u32,String,String,Vec<u8>)>>=spin::Mutex::new(Vec::new());
    pub static FAIL_WRITE:AtomicBool=AtomicBool::new(false);
    pub async fn read(disk:u32,path:&str,key:&str)->Option<Vec<u8>>{
        VALUES.lock().get(&(disk,path.into(),key.into())).filter(|bytes|crate::policy::preflight(bytes).is_ok_and(|info|info.format==2)).cloned()
    }
    pub async fn write(disk:u32,path:&str,key:&str,bytes:&[u8])->Result<(),i32>{
        assert!(crate::ui::INSTALLS.load(Ordering::Relaxed)>0,"cache write preceded sprite publication");
        WRITES.lock().push((disk,path.into(),key.into(),bytes.into()));
        if FAIL_WRITE.load(Ordering::Relaxed){Err(-6)}else{Ok(())}
    }
}
#[cfg(test)] mod disc { pub mod block {pub type DeviceHandle=u32;} }
#[cfg(test)] mod r {
    pub mod net { pub mod https {
        use alloc::{collections::BTreeMap,string::String,vec::Vec};
        use core::sync::atomic::{AtomicBool,Ordering};
        pub static REQUESTS:spin::Mutex<Vec<(String,u32,usize)>>=spin::Mutex::new(Vec::new());
        pub static REPLIES:spin::Mutex<BTreeMap<String,Result<Vec<u8>,String>>>=spin::Mutex::new(BTreeMap::new());
        pub static CANCEL_AFTER_FETCH:AtomicBool=AtomicBool::new(false);
        pub struct MediaFetchCancellation(core::sync::atomic::AtomicBool);
        impl MediaFetchCancellation {
            pub fn new()->Self {Self(core::sync::atomic::AtomicBool::new(false))}
            pub fn cancel(&self) {self.0.store(true,core::sync::atomic::Ordering::Release);}
        }
        pub async fn get_image_bytes_shared(url:&str,timeout:u32,max:usize,cancellation:&MediaFetchCancellation)->Result<Vec<u8>,String>{
            REQUESTS.lock().push((url.into(),timeout,max));
            if cancellation.0.load(Ordering::Relaxed){return Err("cancelled".into());}
            let result=REPLIES.lock().get(url).cloned().unwrap_or_else(||Err("unconfigured test URL".into()));
            if CANCEL_AFTER_FETCH.load(Ordering::Relaxed){crate::ui::INCARNATION.store(2,Ordering::Relaxed);}
            result
        }
    }}
    pub mod fs {pub mod request_broker {
        use alloc::{collections::BTreeMap,string::String,vec::Vec};
        use core::sync::atomic::{AtomicUsize,Ordering};
        pub static FILES:spin::Mutex<BTreeMap<String,Vec<u8>>>=spin::Mutex::new(BTreeMap::new());
        pub static READS:AtomicUsize=AtomicUsize::new(0);
        pub static FORCE_LENGTH:AtomicUsize=AtomicUsize::new(0);
        pub fn read_file_len(_:u32,path:String)->Result<Result<usize,()>,()> {
            let forced=FORCE_LENGTH.load(Ordering::Relaxed);
            Ok(if forced!=0 {Ok(forced)} else {FILES.lock().get(&path).map(Vec::len).ok_or(())})
        }
        pub fn read_file_range(_:u32,path:String,offset:u64,cap:usize)->Result<Result<Vec<u8>,()>,()> {
            READS.fetch_add(1,Ordering::Relaxed);
            Ok(FILES.lock().get(&path).and_then(|bytes|bytes.get(offset as usize..))
                .map(|bytes|bytes[..bytes.len().min(cap)].to_vec()).ok_or(()))
        }
    }}
}
'''


def service_harness() -> str:
    source = (ROOT / "src/r/services/hull_image_service.rs").read_text()
    constants = source[source.index("const ERR_NOT_FOUND:"):source.index("static NEXT_ID:")]
    types = source[source.index("struct Life {"):source.index("\nfn ensure_workers(")]
    functions = "\n".join(production_item(source, name) for name in (
        "request_inner", "status", "release", "reset", "complete_error", "read_bounded", "read_cache",
        "asset_failure", "encoded_source", "converted_svg", "process", "resize", "decode_permit",
    ))
    permit = source[source.index("struct DecodePermit;"):source.index("async fn decode_permit(")]
    return (SERVICE_MOCKS + SERVICE_TESTS.replace("SERVICE_CONSTANTS", constants)
            .replace("SERVICE_TYPES", types).replace("SERVICE_FUNCTIONS", functions)
            .replace("SERVICE_DECODE_PERMIT", permit))


def bounded_media_harness() -> str:
    source = (ROOT / "src/r/services/media_service.rs").read_text()
    constants = source[source.index("pub const FORMAT_JPEG:"):source.index("static NEXT_ID:")]
    structures = []
    for name in ("ImageInfo", "DecodedImage"):
        declaration = re.search(rf"^pub(?:\(crate\))? struct {name} \{{", source, re.MULTILINE)
        end = source.index("\n}", declaration.end()) + 2
        structures.append(source[declaration.start():end])
    functions = "\n".join(production_item(source, name) for name in
                           ("decode_ui4_bounded", "validated_image", "rgba_byte_len_within_limit"))
    return ("pub(crate) use policy as hull_image_policy;\nmod media {\nuse alloc::vec::Vec;\n"
            + constants + "\n" + "\n".join(structures) + "\n" + functions + "\n}\n")


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="trueos-hull-images-") as directory:
        project = Path(directory)
        source = project / "src"
        source.mkdir()
        # Production PNG expansion currently disables its parallel kernel pool.
        # Preserve all real expansion code and replace only that unused scheduler.
        pool = (DECODERS / "png_decode_pool.rs").read_text()
        pool = pool.replace(production_item(pool, "spawn_parallel"),
                            "fn spawn_parallel(_: &Arc<RowExpandJob>) { unreachable!() }\n")
        pool = re.sub(r"^#\[trueos_executor::task[^\n]*\]\n", "", pool, flags=re.MULTILINE)
        pool = pool.replace(production_item(pool, "png_row_expand_task"), "")
        (source / "png_decode_pool.rs").write_text(pool)
        modules = "\n".join(f"#[path={json.dumps(str(DECODERS / (name + '.rs')))}] pub mod {name};"
                            for name in ("png", "jpeg", "jpeg_layout", "bmp", "ico", "webp"))
        harness = r'''#![allow(dead_code)]
extern crate alloc;
#[macro_export] macro_rules! log { ($($tt:tt)*) => { let _ = format_args!($($tt)*); } }
static WARNINGS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
#[macro_export] macro_rules! log_warn { (target: $target:expr; $($tt:tt)*) => { crate::WARNINGS.lock().unwrap().push(format!($($tt)*)); } }
mod wait {
    pub struct WaitQueue;
    impl WaitQueue {
        pub fn new() -> Self { Self }
        pub fn notify_all(&self) {}
        pub fn wait_for_event_blocking(&self, _: u64) { unreachable!() }
    }
}
mod workers { pub fn has_background_worker_slot() -> bool { false } }
'''
        harness += f"#[path={json.dumps(str(ROOT / 'src/r/services/hull_image_policy.rs'))}] pub(crate) mod policy;\n"
        harness += f"#[path={json.dumps(str(ROOT / 'src/r/services/hull_svg_policy.rs'))}] pub(crate) mod hull_svg_policy;\n"
        harness += "mod graphics {\n" + modules + "\npub(crate) use ico as ico_codec;\npub(crate) use png as png_codec;\npub(crate) use jpeg as jpeg_codec;\npub(crate) use bmp as bmp_codec;\npub(crate) use webp as webp_codec;\n"
        harness += f"#[path={json.dumps(str(source / 'png_decode_pool.rs'))}] mod png_decode_pool;\n}}\n"
        (source / "lib.rs").write_text(harness + bounded_media_harness() + "#[cfg(test)] use media as media_service;\n" + TESTS + service_harness())
        rgba = bytes([255,0,0,255, 0,255,0,128, 0,0,255,0, 255,255,255,255])
        (source / "rgba.png").write_bytes(png(2, 2, 6, b"\0" + rgba[:8] + b"\0" + rgba[8:]))
        (source / "large-icon.png").write_bytes(png(256,256,6,(b"\0"+bytes([11,22,33,128])*256)*256))
        (source / "indexed.png").write_bytes(png(2, 1, 3, b"\0\x40", depth=1,
            before_data=chunk(b"PLTE", bytes([255,0,0, 0,0,255])) + chunk(b"tRNS", bytes([255,64]))))
        (source / "animated.png").write_bytes(png(2, 2, 6, b"\0" * 18,
            before_data=chunk(b"acTL", struct.pack(">II", 1, 0))))
        (source / "duplicate-header.png").write_bytes(png(2, 2, 6, b"\0" * 18,
            before_data=chunk(b"IHDR", struct.pack(">IIBBBBB", 8192, 8192, 8, 6, 0, 0, 0))))
        (source / "oversized.png").write_bytes(png(8192, 8192, 6, b""))
        (source / "invalid-pixels.png").write_bytes(png(2, 2, 6, b""))
        # Benign 2x2 colour swatches encoded once with Pillow/libwebp; no runtime
        # host encoder dependency and no website image bytes are checked in.
        webp_fixtures = {
            "lossless-rgb":"UklGRjIAAABXRUJQVlA4TCYAAAAvAUAAAB8gECCY8H9qQyCQBLC/7gIBghT/keY/8MlRwQ0YIvofAg==",
            "lossless-alpha":"UklGRjQAAABXRUJQVlA4TCcAAAAvAUAAEB8gFkxxEN0TC6a4O4iNh0AgGS2z1aP0EQfZAGWqMIvof+wA",
            "lossy-alpha":"UklGRl4AAABXRUJQVlA4WAoAAAAQAAAAAQAAAQAAQUxQSAUAAAAAAECA/wBWUDggMgAAALABAJ0BKgIAAgAAwBIllAJ0AQ72xMgA/v8R4Xj/S3vhvwdCpI8hyOwhutf64ubIbwAA",
            "lossy-rgb":"UklGRkIAAABXRUJQVlA4IDYAAADQAQCdASoCAAIAAMASJZwCdAEO+M37wAD+/07h/wef5qgTORTplm27AMSx475578boUomgAAA=",
        }
        for name, data in webp_fixtures.items():
            (source / ("webp-"+name+".bin")).write_bytes(base64.b64decode(data))
        (source / "sample.jpg").write_bytes((ROOT / "vendor/infer/testdata/sample.jpg").read_bytes())
        (project / "Cargo.toml").write_text(f'''[package]
name = "hull-image-host-tests"
version = "0.1.0"
edition = "2024"
[workspace]
[dependencies]
core3 = {{ version = "0.1.2", default-features = false, features = ["alloc"] }}
image-webp = {{ path = {json.dumps(str(ROOT / "vendor/image-webp-0.2.4-trueos"))}, default-features = false }}
png = {{ path = {json.dumps(str(ROOT / 'vendor/png-0.18.1'))}, default-features = false }}
zune-core = {{ path = {json.dumps(str(ROOT / 'vendor/zune-core-0.5.1'))}, default-features = false }}
zune-jpeg = {{ path = {json.dumps(str(ROOT / 'vendor/zune-jpeg-0.5.15'))}, default-features = false, features = ["x86", "portable_simd"] }}
sha2 = {{ version = "=0.10.9", default-features = false }}
spin = "0.10"
[patch.crates-io]
zune-core = {{ path = {json.dumps(str(ROOT / 'vendor/zune-core-0.5.1'))} }}
fdeflate = {{ path = {json.dumps(str(ROOT / 'vendor/fdeflate-0.3.7'))} }}
simd-adler32 = {{ path = {json.dumps(str(ROOT / 'vendor/simd-adler32-0.3.8'))} }}
crc32fast = {{ path = {json.dumps(str(ROOT / 'vendor/crc32fast-1.5.0'))} }}
''')
        environment = dict(os.environ)
        environment["CARGO_TARGET_DIR"] = str(ROOT / "tgt/hull-image-host-tests")
        environment["RUSTUP_TOOLCHAIN"] = "nightly-2026-07-10"
        subprocess.run(["cargo", "test", "--manifest-path", str(project / "Cargo.toml")],
                       cwd=project, env=environment, check=True)


if __name__ == "__main__":
    main()
