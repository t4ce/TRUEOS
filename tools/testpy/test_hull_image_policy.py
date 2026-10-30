#!/usr/bin/env python3
"""Exercise Hull's real admission policy and image codecs on the host.

The kernel is freestanding. This harness compiles its production policy and
PNG/JPEG/BMP decoders, plus real service admission and cache checks. Worker
scheduling, UI ownership, and filesystem operations use deterministic mocks.
It does not use the network, TRUEOS storage, or graphics hardware.
Run with ``python3 tools/testpy/test_hull_image_policy.py``.
"""

import json
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
    use graphics::{bmp, jpeg, png};

    const EXPECTED: &[u8] = &[255,0,0,255, 0,255,0,128, 0,0,255,0, 255,255,255,255];

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
            for magic in [b"BM".as_slice(), b"\xff\xd8".as_slice(), b"\x89PNG\r\n\x1a\n".as_slice()] {
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
    use crate::policy::{self, ERR_INVALID};
    use crate::r::fs::request_broker as fs;
    use crate::ui;
    #[derive(Clone, Copy, Default)]
    pub(crate) struct ImageInfo { state:u32, sprite_id:u32, width:u32, height:u32, generation:u64, error:i32, reserved:u32 }
    SERVICE_CONSTANTS
    static NEXT_ID: AtomicU32 = AtomicU32::new(1);
    static REGISTRY: Mutex<Registry> = Mutex::new(Registry::new());
    SERVICE_TYPES
    fn ensure_workers() -> Result<(),i32> { Ok(()) }
    fn cache_scope(_: &str, _: &str, _: &str) -> Result<Option<Cache>,i32> { Ok(None) }
    SERVICE_FUNCTIONS
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    fn setup() -> std::sync::MutexGuard<'static,()> {
        let guard = TEST_LOCK.lock().unwrap();
        *REGISTRY.lock() = Registry::new();
        NEXT_ID.store(1, Ordering::Relaxed);
        crate::hv::RUN.store(1, Ordering::Relaxed);
        ui::INCARNATION.store(1, Ordering::Relaxed);
        ui::BUSY.store(false, Ordering::Relaxed);
        ui::RELEASES.store(0, Ordering::Relaxed);
        fs::FILES.lock().clear(); fs::READS.store(0, Ordering::Relaxed);
        fs::FORCE_LENGTH.store(0, Ordering::Relaxed);
        guard
    }
    fn admit(owner:u32, window:u32, url:&str, w:u32, h:u32, generation:u64) -> Result<i32,i32> {
        request_inner(owner, window, url, "apps/hull", "example.test", w,h,generation)
    }

    #[test]
    fn admission_deduplicates_ready_images_and_bounds_each_window() {
        let _guard = setup();
        let url = "https://example.test/0.png";
        let first = admit(7,1,url,2048,2048,9).unwrap();
        {
            let mut r = REGISTRY.lock();
            r.operations[0].info.state=1;
            r.operations[0].info.width=1; r.operations[0].info.height=1;
        }
        assert_eq!(admit(7,1,url,2048,2048,9), Ok(first));
        for i in 1..8 { admit(7,1,&format!("https://example.test/{i}.png"),2048,2048,9).unwrap(); }
        assert_eq!(admit(7,1,"https://example.test/overflow.png",1,1,9),Err(ERR_BUSY));
        let r = REGISTRY.lock();
        assert_eq!(r.operations.len(),8);
        assert!(r.operations.iter().map(|op|op.reserved_bytes).sum::<usize>()<=WINDOW_BYTES);
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
        let cache=Cache { disk:1,directory:"cache".into() };
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
        let cache=Cache { disk:1,directory:"cache".into() };
        fs::FORCE_LENGTH.store(policy::MAX_ENCODED_BYTES+1,Ordering::Relaxed);
        assert!(read_bounded(&cache,"source.png",policy::MAX_ENCODED_BYTES).is_none());
        assert_eq!(fs::READS.load(Ordering::Relaxed),0);
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
    use core::sync::atomic::{AtomicBool,AtomicU64,AtomicUsize,Ordering};
    pub static INCARNATION:AtomicU64=AtomicU64::new(1);
    pub static BUSY:AtomicBool=AtomicBool::new(false);
    pub static RELEASES:AtomicUsize=AtomicUsize::new(0);
    pub fn authenticate_window(_:u32,_:u32)->Result<u64,i32> {
        if BUSY.load(Ordering::Relaxed) {Err(-7)} else {Ok(INCARNATION.load(Ordering::Relaxed))}
    }
    pub fn release_rgba(_:u32,_:u32,_:u64,_:u32)->i32 { RELEASES.fetch_add(1,Ordering::Relaxed);0 }
}
#[cfg(test)] mod disc { pub mod block {pub type DeviceHandle=u32;} }
#[cfg(test)] mod r {
    pub mod net { pub mod https {
        pub struct MediaFetchCancellation(core::sync::atomic::AtomicBool);
        impl MediaFetchCancellation {
            pub fn new()->Self {Self(core::sync::atomic::AtomicBool::new(false))}
            pub fn cancel(&self) {self.0.store(true,core::sync::atomic::Ordering::Release);}
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
    ))
    return (SERVICE_MOCKS + SERVICE_TESTS.replace("SERVICE_CONSTANTS", constants)
            .replace("SERVICE_TYPES", types).replace("SERVICE_FUNCTIONS", functions))


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
                            for name in ("png", "jpeg", "jpeg_layout", "bmp"))
        harness = r'''#![allow(dead_code)]
extern crate alloc;
#[macro_export] macro_rules! log { ($($tt:tt)*) => { let _ = format_args!($($tt)*); } }
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
        harness += f"#[path={json.dumps(str(ROOT / 'src/r/services/hull_image_policy.rs'))}] mod policy;\n"
        harness += "mod graphics {\n" + modules + "\n"
        harness += f"#[path={json.dumps(str(source / 'png_decode_pool.rs'))}] mod png_decode_pool;\n}}\n"
        (source / "lib.rs").write_text(harness + TESTS + service_harness())
        rgba = bytes([255,0,0,255, 0,255,0,128, 0,0,255,0, 255,255,255,255])
        (source / "rgba.png").write_bytes(png(2, 2, 6, b"\0" + rgba[:8] + b"\0" + rgba[8:]))
        (source / "indexed.png").write_bytes(png(2, 1, 3, b"\0\x40", depth=1,
            before_data=chunk(b"PLTE", bytes([255,0,0, 0,0,255])) + chunk(b"tRNS", bytes([255,64]))))
        (source / "animated.png").write_bytes(png(2, 2, 6, b"\0" * 18,
            before_data=chunk(b"acTL", struct.pack(">II", 1, 0))))
        (source / "duplicate-header.png").write_bytes(png(2, 2, 6, b"\0" * 18,
            before_data=chunk(b"IHDR", struct.pack(">IIBBBBB", 8192, 8192, 8, 6, 0, 0, 0))))
        (source / "oversized.png").write_bytes(png(8192, 8192, 6, b""))
        (source / "sample.jpg").write_bytes((ROOT / "vendor/infer/testdata/sample.jpg").read_bytes())
        (project / "Cargo.toml").write_text(f'''[package]
name = "hull-image-host-tests"
version = "0.1.0"
edition = "2024"
[workspace]
[dependencies]
core3 = {{ version = "0.1.2", default-features = false, features = ["alloc"] }}
png = {{ path = {json.dumps(str(ROOT / 'vendor/png-0.18.1'))}, default-features = false }}
zune-core = {{ path = {json.dumps(str(ROOT / 'vendor/zune-core-0.5.1'))}, default-features = false }}
zune-jpeg = {{ path = {json.dumps(str(ROOT / 'vendor/zune-jpeg-0.5.15'))}, default-features = false, features = ["x86", "portable_simd"] }}
sha2 = {{ version = "=0.10.9", default-features = false }}
spin = "0.10"
[patch.crates-io]
zune-core = {{ path = {json.dumps(str(ROOT / 'vendor/zune-core-0.5.1'))} }}
''')
        environment = dict(os.environ)
        environment["CARGO_TARGET_DIR"] = str(ROOT / "tgt/hull-image-host-tests")
        environment["RUSTUP_TOOLCHAIN"] = "nightly-2026-07-10"
        subprocess.run(["cargo", "test", "--manifest-path", str(project / "Cargo.toml")],
                       cwd=project, env=environment, check=True)


if __name__ == "__main__":
    main()
