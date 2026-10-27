#!/usr/bin/env python3
"""Exercise the production update consumer/tar parser with real native bundles.

The standalone host harness uses liblz4 at the codec boundary instead of the
kernel's GPU/worker dispatch. It needs rustc and host liblz4, but never Cargo.
"""
import ctypes
import ctypes.util
import io
import json
from pathlib import Path
import subprocess
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def frame(data):
    library = ctypes.util.find_library("lz4")
    if library is None:
        raise RuntimeError("host liblz4 is required")
    lz4 = ctypes.CDLL(library)
    lz4.LZ4F_compressFrameBound.argtypes = [ctypes.c_size_t, ctypes.c_void_p]
    lz4.LZ4F_compressFrameBound.restype = ctypes.c_size_t
    lz4.LZ4F_compressFrame.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p,
                                     ctypes.c_size_t, ctypes.c_void_p]
    lz4.LZ4F_compressFrame.restype = ctypes.c_size_t
    lz4.LZ4F_isError.argtypes = [ctypes.c_size_t]
    destination = ctypes.create_string_buffer(lz4.LZ4F_compressFrameBound(len(data), None))
    size = lz4.LZ4F_compressFrame(destination, len(destination), data, len(data), None)
    if lz4.LZ4F_isError(size):
        raise RuntimeError("fixture LZ4 frame compression failed")
    return destination.raw[:size]


def archive(entries):
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w", format=tarfile.USTAR_FORMAT) as tar:
        for name, data in entries:
            member = tarfile.TarInfo(name)
            member.size = len(data)
            tar.addfile(member, io.BytesIO(data))
    return buffer.getvalue()


def main():
    members = [("TRUEOS.elf", b"\x7fELFcandidate"),
               ("EFI/BOOT/BOOTX64.EFI", b"MZnative-loader"),
               ("EFI/BOOT/tgl_guc_70.bin", b"native-guc-firmware")]
    fixtures = {"valid": frame(archive(members))}
    for index in range(3):
        fixtures[f"missing-{index}"] = frame(archive(members[:index] + members[index + 1:]))
        fixtures[f"duplicate-{index}"] = frame(archive(members + [members[index]]))
    fixtures["unexpected"] = frame(archive(members + [("../TRUEOS.elf", b"\x7fELFother")]))
    fixtures["bad-kernel"] = frame(archive([(members[0][0], b"ISO9660"), *members[1:]]))
    fixtures["bad-loader"] = frame(archive([members[0], (members[1][0], b"invalid"), members[2]]))
    fixtures["empty-firmware"] = frame(archive([*members[:2], (members[2][0], b"")]))
    fixtures["truncated-tar"] = frame(archive(members)[:512])
    fixtures["too-many-members"] = frame(archive([members[0]] * 33))
    fixtures["not-lz4"] = archive(members)
    fixtures["truncated-lz4"] = fixtures["valid"][:-3]
    fixtures["concatenated-lz4"] = fixtures["valid"] * 2
    source = r'''
#![allow(dead_code)]
extern crate alloc;
#[macro_export] macro_rules! log_warn { ($($tt:tt)*) => {} }
mod r {
    #[path=TAR_SOURCE] pub mod tar;
    pub mod lz4 {
        use std::ffi::c_void;
        pub const MAGIC: [u8; 4] = [4, 34, 77, 24];
        #[link(name="lz4")]
        unsafe extern "C" {
            fn LZ4F_createDecompressionContext(context: *mut *mut c_void, version: u32) -> usize;
            fn LZ4F_freeDecompressionContext(context: *mut c_void) -> usize;
            fn LZ4F_decompress(context: *mut c_void, destination: *mut c_void,
                destination_size: *mut usize, source: *const c_void,
                source_size: *mut usize, options: *const c_void) -> usize;
            fn LZ4F_isError(code: usize) -> u32;
        }
        struct Context(*mut c_void);
        impl Drop for Context {
            fn drop(&mut self) { unsafe { LZ4F_freeDecompressionContext(self.0); } }
        }
        pub fn decompress_frame_cpu(input: &[u8], limit: usize) -> Result<Vec<u8>, ()> {
            let mut context = std::ptr::null_mut();
            if unsafe { LZ4F_isError(LZ4F_createDecompressionContext(&mut context, 100)) } != 0 {
                return Err(());
            }
            let context = Context(context);
            let mut output = Vec::new();
            let mut offset = 0;
            loop {
                let mut chunk = [0u8; 65536];
                let mut written = chunk.len();
                let mut consumed = input.len() - offset;
                let remaining = unsafe { LZ4F_decompress(context.0, chunk.as_mut_ptr().cast(),
                    &mut written, input.as_ptr().add(offset).cast(), &mut consumed, std::ptr::null()) };
                if unsafe { LZ4F_isError(remaining) } != 0 || output.len() + written > limit {
                    return Err(());
                }
                output.extend_from_slice(&chunk[..written]);
                offset += consumed;
                if remaining == 0 {
                    return if offset == input.len() { Ok(output) } else { Err(()) };
                }
                if consumed == 0 && written == 0 { return Err(()); }
            }
        }
        pub async fn decompress_frame(input: Vec<u8>, limit: usize) -> Result<Vec<u8>, ()> {
            decompress_frame_cpu(&input, limit)
        }
    }
}
#[path=UPDATE_SOURCE] mod update_bundle;
fn decode(name: &str) -> Result<Vec<u8>, &'static str> {
    let root = std::path::PathBuf::from(std::env::var_os("TRUEOS_UPDATE_FIXTURES").unwrap());
    let payload = std::fs::read(root.join(name)).unwrap();
    complete(update_bundle::kernel_from_archive(payload))
}
fn complete<T>(future: impl std::future::Future<Output=T>) -> T {
    let waker = std::task::Waker::noop();
    let mut context = std::task::Context::from_waker(waker);
    let mut future = std::pin::pin!(future);
    match future.as_mut().poll(&mut context) {
        std::task::Poll::Ready(value) => value,
        std::task::Poll::Pending => panic!("host codec must complete synchronously"),
    }
}
#[test] fn native_bundle_returns_the_kernel_directly() {
    assert_eq!(decode("valid").unwrap(), b"\x7fELFcandidate");
}
#[test] fn each_required_member_is_checked_for_presence_and_duplicates() {
    for (index, path) in ["TRUEOS.elf", "EFI/BOOT/BOOTX64.EFI", "EFI/BOOT/tgl_guc_70.bin"].iter().enumerate() {
        assert_eq!(decode(&format!("missing-{index}")).unwrap_err(), format!("update archive missing {path}"));
        assert_eq!(decode(&format!("duplicate-{index}")).unwrap_err(), format!("duplicate {path}"));
    }
}
#[test] fn native_file_formats_and_exact_paths_are_required() {
    for fixture in ["unexpected", "bad-kernel", "bad-loader", "empty-firmware"] {
        assert!(decode(fixture).is_err(), "{fixture}");
    }
}
#[test] fn invalid_archives_and_frames_are_rejected() {
    for fixture in ["truncated-tar", "too-many-members", "not-lz4", "truncated-lz4", "concatenated-lz4"] {
        assert!(decode(fixture).is_err(), "{fixture}");
    }
}
#[test] fn compressed_payload_limit_is_checked_before_decoding() {
    let mut payload = vec![0u8; update_bundle::MAX_COMPRESSED_BYTES + 1];
    payload[..4].copy_from_slice(&r::lz4::MAGIC);
    assert_eq!(complete(update_bundle::kernel_from_archive(payload)).unwrap_err(),
        "compressed update bundle exceeds limit");
}
'''.replace("TAR_SOURCE", json.dumps(str(ROOT / "src/r/tar.rs"))).replace(
        "UPDATE_SOURCE", json.dumps(str(ROOT / "src/update_bundle.rs")))
    with tempfile.TemporaryDirectory(prefix="trueos-update-bundle-") as directory:
        directory = Path(directory)
        for name, payload in fixtures.items():
            (directory / name).write_bytes(payload)
        path = directory / "tests.rs"
        path.write_text(source)
        executable = directory / "tests"
        subprocess.run(["rustc", "--edition=2024", "--test", str(path), "-o", str(executable)], check=True)
        import os
        subprocess.run([str(executable)], env={**os.environ, "TRUEOS_UPDATE_FIXTURES": str(directory)}, check=True)


if __name__ == "__main__":
    main()
