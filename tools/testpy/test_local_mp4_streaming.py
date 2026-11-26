#!/usr/bin/env python3
"""Exercise native playback preparation with a sparse TRUEOSFS AVC MP4."""
import re
import tempfile
from pathlib import Path

from test_http_mp4_streaming import (
    ROOT, access_units, demux, memory_reader, nal_bits, run_harness, source, tail,
)


BOUNDARY = r'''
#![allow(dead_code, unused_imports, unused_variables)]
extern crate alloc;
use std::cell::RefCell;
use std::future::Future;
use std::task::{Context, Poll, Waker};

#[macro_export]
macro_rules! log { ($($args:tt)*) => {}; }
#[macro_export]
macro_rules! log_info { ($($args:tt)*) => {}; }
#[macro_export]
macro_rules! log_warn { ($($args:tt)*) => {}; }
#[macro_export]
macro_rules! log_error { ($($args:tt)*) => {}; }

mod allocators {
    pub struct Integrity { pub healthy: bool }
    pub fn host_heap_integrity_bounded() -> Integrity { Integrity { healthy: true } }
}

mod ui4 {
    #[derive(Clone, Copy)]
    pub struct VideoPlaybackSession;
    impl VideoPlaybackSession {
        pub fn is_cancelled(self) -> bool { false }
    }
}

mod disc { pub mod block {
    #[derive(Clone, Copy, PartialEq)]
    pub struct DiscId(u32);
    impl DiscId {
        pub fn from_raw(raw: u32) -> Self { Self(raw) }
    }
    #[derive(Clone, Copy)]
    pub struct DeviceHandle(DiscId);
    impl DeviceHandle {
        pub fn id(self) -> DiscId { self.0 }
    }
    pub fn device_handle(id: DiscId) -> Option<DeviceHandle> { Some(DeviceHandle(id)) }
}}

#[derive(Default)]
struct SparseFile {
    len: u64,
    contents: Vec<(u64, Vec<u8>)>,
    reads: Vec<(u64, usize)>,
}
thread_local! { static FILE: RefCell<SparseFile> = RefCell::new(SparseFile::default()); }

mod r { pub mod fs { pub mod trueosfs {
    use crate::disc::block::{DiscId, DeviceHandle, device_handle};

    pub struct Root { pub disk_id: DiscId, pub index_ready: bool }
    pub fn list_roots() -> [Root; 1] {
        [Root { disk_id: DiscId::from_raw(7), index_ready: true }]
    }
    pub fn primary_root_handle() -> Option<DeviceHandle> { device_handle(list_roots()[0].disk_id) }
    pub fn request_warm_index(_: DiscId) {}

    #[derive(Clone, Copy)]
    pub struct FileReadHandle(u64);
    impl FileReadHandle {
        pub fn data_len(self) -> u64 { self.0 }
    }
    pub fn file_read_handle_is_current(_: FileReadHandle) -> bool { true }
    pub async fn file_read_handle_range_async(
        handle: FileReadHandle, offset: u64, out: &mut [u8],
    ) -> Result<Option<usize>, ()> {
        let end = offset + out.len() as u64;
        assert!(end <= handle.data_len());
        crate::FILE.with_borrow_mut(|file| {
            file.reads.push((offset, out.len()));
            out.fill(0);
            for (start, bytes) in &file.contents {
                let first = offset.max(*start);
                let last = end.min(*start + bytes.len() as u64);
                if first < last {
                    out[(first - offset) as usize..(last - offset) as usize]
                        .copy_from_slice(&bytes[(first - start) as usize..(last - start) as usize]);
                }
            }
        });
        Ok(Some(out.len()))
    }
    pub mod authorized {
        use super::*;
        pub struct Actor;
        pub async fn open_read(_: Actor, disk: DeviceHandle, path: &str) -> Result<Option<FileReadHandle>, ()> {
            assert_eq!(path, "movie.mp4");
            assert!(disk.id() == DiscId::from_raw(7));
            Ok(Some(crate::FILE.with_borrow(|file| FileReadHandle(file.len))))
        }
    }
}}}

mod intel {
pub fn has_media_decode_engine() -> bool { true }
pub mod media {
pub mod h264_cmd {
    pub struct AvcStreamExtent { pub visible_width: u32, pub visible_height: u32 }
    pub fn parse_annexb_stream_extent(bytes: &[u8]) -> Result<AvcStreamExtent, ()> {
        bytes.windows(6).any(|nal| nal == [0, 0, 0, 1, 0x67, 0x80])
            .then_some(AvcStreamExtent { visible_width: 32, visible_height: 32 }).ok_or(())
    }
}

mod hw_vid {
use crate::*;
use alloc::string::String;
use std::fmt::Write;

struct Timer;
impl Timer {
    async fn after_millis(_: u64) { panic!("unexpected wait for a warm filesystem"); }
}

// The production dispatcher also covers online and live playback. This test
// provides just the unused variant methods so its local dispatch runs unchanged.
struct UnusedReader { failed: bool }
impl UnusedReader {
    async fn next_nal(&mut self) -> Option<H264BufferedNal> { unreachable!() }
}
type H264HlsNalReader = UnusedReader;
type H264HttpNalReader = UnusedReader;
type H264LiveNalReader = UnusedReader;
'''

TEST = r'''
fn complete<T>(future: impl Future<Output = T>) -> T {
    let mut future = std::pin::pin!(future);
    match future.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("mocked filesystem read did not complete"),
    }
}

fn next_vcl(reader: &mut H264NalReader) -> Option<H264BufferedNal> {
    while let Some(nal) = complete(reader.next_nal()) {
        if matches!(nal.meta.nal_type, 1 | 5) {
            return Some(nal);
        }
    }
    None
}

#[test]
fn near_two_gib_local_mp4_with_tail_moov_streams_both_pictures() {
    let first = ftyp().len() + 8;
    let moov_offset = (1 << 31) - (64 * 1024);
    let second = moov_offset - 16;
    let mdat_header = [(moov_offset as u32 - ftyp().len() as u32).to_be_bytes().as_slice(), b"mdat"].concat();
    let moov = bx(b"moov", &avc_track(vec![
        bx(b"stsz", &ints(&[0, 0, 2, 6, 6])),
        bx(b"stsc", &ints(&[0, 1, 1, 1, 1])),
        bx(b"stco", &ints(&[0, 2, first as u32, second as u32])),
        bx(b"stts", &ints(&[0, 1, 2, 3600])),
        bx(b"stss", &ints(&[0, 1, 1])),
    ]));
    let len = (moov_offset + moov.len()) as u64;
    FILE.with_borrow_mut(|file| *file = SparseFile {
        len,
        contents: vec![
            (0, [ftyp(), mdat_header].concat()),
            (first as u64, vec![0, 0, 0, 2, 0x65, 0x80]),
            (second as u64, vec![0, 0, 0, 2, 0x41, 0x81]),
            (moov_offset as u64, moov),
        ],
        reads: Vec::new(),
    });

    let mut video = complete(prepare_trueosfs_ui4_video(
        ui4::VideoPlaybackSession, "trueosfs:disc7/movie.mp4", r::fs::trueosfs::authorized::Actor,
    )).expect("large local classic MP4 should be admitted for streaming");
    assert_eq!(video.stream_bytes, len);
    assert_eq!(video.decode_source, "trueosfs-stream-mp4-avc");
    assert_eq!(video.sample_timing.len(), 2);
    assert_eq!(video.visible_extent(), (32, 32));
    let idr = next_vcl(&mut video.reader).expect("first picture");
    assert_eq!(idr.bytes, [0, 0, 0, 1, 0x65, 0x80]);
    FILE.with_borrow(|file| {
        assert!(file.reads.iter().any(|&(offset, _)| offset == moov_offset as u64));
        assert!(file.reads.iter().all(|&(offset, count)| {
            let second = second as u64;
            second < offset || second >= offset + count as u64
        }), "the second picture should be read on demand");
    });
    let inter = next_vcl(&mut video.reader).expect("picture near end of file");
    assert_eq!(inter.bytes, [0, 0, 0, 1, 0x41, 0x81]);
    assert!(inter.meta.stream_offset >= second as u64);
    assert!(next_vcl(&mut video.reader).is_none());
    FILE.with_borrow(|file| {
        assert!(file.reads.iter().all(|&(_, size)| size <= 64 * 1024));
        assert!(file.reads.iter().map(|&(_, size)| size).sum::<usize>() < 2 * 1024 * 1024);
    });
}
}}}
'''


def between(start: str, end: str, text: str = source) -> str:
    return text[text.index(start):text.index(end)]


if __name__ == "__main__":
    constants = "\n".join(re.findall(
        r"^const H264_(?:BUFFERED_VIDEO_CAP|TRUEOSFS_VIDEO_SOFT_CAP|TRUEOSFS_READ_CHUNK|FS_\w+)_BYTES:.*$",
        source, re.MULTILINE,
    ))
    audio_path = ROOT / "src/intel/media/mp4_audio.rs"
    audio = f'\nmod http_mp4 {{ use super::*; #[path = "{audio_path}"] pub(super) mod audio; }}\n'
    fixture = between(" fn bx(", " fn classic(", tail)
    rust = (
        BOUNDARY + constants + audio + demux + access_units + memory_reader + nal_bits
        + between("fn h264_has_annexb_start_code(", "fn h264_is_mp4_container(")
        + between("async fn h264_wait_for_fs_index(", "/// Run a prepared TRUEOSFS stream")
        + between("async fn h264_fs_read_exact(", "struct LocalVideoAudio {")
        + between("struct H264FileNalReader {", "async fn h264_next_stream_access_unit(")
        + between("enum H264NalReader {", "struct H264HttpNalReader {")
        + between("impl H264NalReader {", "fn h264_ticks_to_millis(")
        + fixture + TEST
    )
    with tempfile.TemporaryDirectory(prefix="local-mp4-streaming-") as directory:
        run_harness(Path(directory), rust)
