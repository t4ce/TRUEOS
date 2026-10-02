#!/usr/bin/env python3
"""Exercise production file/NAL/picture readers against fault-injected range I/O."""
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
s = (ROOT / 'src/intel/media/hw_vid.rs').read_text()
def item(name):
    match = re.search(r'^(?:async )?fn ' + name + r'\b.*?^}\n', s, re.M | re.S)
    assert match, name
    return match.group()
production = s[s.index('#[derive(Clone, Copy, Debug)]\nstruct Mp4Box'):s.index('fn h264_prepare_trueosfs_asset')]
production += s[s.index('#[derive(Copy, Clone, Debug)]\nstruct H264StreamNal'):s.index('struct H264IndexedFrame')]
production += s[s.index('struct H264MemoryNalReader'):s.index('fn h264_ticks_to_millis')]
production += ''.join(item(n) for n in ['h264_find_start_code', 'h264_slice_first_mb_in_slice', 'h264_read_first_ue_from_ebsp', 'h264_ebsp_bit'])
harness = r'''
#![allow(dead_code, unused_variables)]
extern crate alloc;
use alloc::{string::String, vec::Vec};
use std::{fmt::Write, future::Future, task::{Context, Poll, Waker}, sync::Mutex};
const H264_TRUEOSFS_READ_CHUNK_BYTES: usize = 16;
const H264_FS_METADATA_CAP_BYTES: usize = 32 * 1024 * 1024;
const H264_FS_PICTURE_CAP_BYTES: usize = 8 * 1024 * 1024;
struct Rig { bytes: Vec<u8>, read: usize, short: usize, fail_at: usize, cancel: bool }
static RIG: Mutex<Rig> = Mutex::new(Rig { bytes: Vec::new(), read: 0, short: usize::MAX, fail_at: usize::MAX, cancel: false });
#[macro_export] macro_rules! log { ($($x:tt)*) => {}; }
#[macro_export] macro_rules! log_info { ($($x:tt)*) => {}; }
#[macro_export] macro_rules! log_error { ($($x:tt)*) => {}; }
mod ui4 {
    #[derive(Clone, Copy)] pub struct VideoPlaybackSession;
    impl VideoPlaybackSession { pub fn is_cancelled(self) -> bool { crate::RIG.lock().unwrap().cancel } }
}
mod allocators {
    pub struct Heap { pub healthy: bool, pub reason: &'static str, pub nodes: usize }
    pub fn host_heap_integrity_bounded() -> Heap { Heap { healthy: true, reason: "ok", nodes: 0 } }
}
struct Timer;
impl Timer { async fn after_millis(_: u64) {} }
mod r { pub mod fs { pub mod trueosfs {
    #[derive(Clone, Copy)] pub struct FileReadHandle(pub u64);
    impl FileReadHandle { pub fn data_len(self) -> u64 { self.0 } }
    pub async fn file_read_handle_range_async(_: FileReadHandle, offset: u64, out: &mut [u8]) -> Result<Option<usize>, ()> {
        let mut r = crate::RIG.lock().unwrap();
        let offset = offset as usize;
        if offset >= r.fail_at { return Err(()); }
        let n = out.len().min(r.short).min(r.bytes.len().saturating_sub(offset));
        out[..n].copy_from_slice(&r.bytes[offset..offset+n]); r.read += n;
        Ok(Some(n))
    }
}}}
fn run<T>(future: impl Future<Output=T>) -> T {
    let mut f = std::pin::pin!(future);
    match f.as_mut().poll(&mut Context::from_waker(Waker::noop())) { Poll::Ready(v) => v, _ => panic!("unexpected wait") }
}
fn setup(bytes: Vec<u8>) -> (ui4::VideoPlaybackSession, r::fs::trueosfs::FileReadHandle) {
    let len = bytes.len() as u64;
    *RIG.lock().unwrap() = Rig { bytes, read: 0, short: usize::MAX, fail_at: usize::MAX, cancel: false };
    (ui4::VideoPlaybackSession, r::fs::trueosfs::FileReadHandle(len))
}
fn annex(nals: &[Vec<u8>]) -> Vec<u8> {
    nals.iter().flat_map(|n| [vec![0,0,0,1], n.clone()].concat()).collect()
}
#[test] fn nal_chunk_boundaries_short_reads_and_last_nal() {
    for padding in 0..20 {
        let nals = vec![vec![0x67; padding+1], vec![0x68; 31], vec![0x65, 0x80], vec![0x41, 0x80]];
        let bytes = annex(&nals);
        let (session, file) = setup(bytes.clone());
        RIG.lock().unwrap().short = 3;
        let mut reader = H264FileNalReader::new(session, file, None);
        for nal in &nals { assert_eq!(run(reader.next_nal()).unwrap().bytes, annex(&[nal.clone()])); }
        assert!(run(reader.next_nal()).is_none()); assert!(!reader.failed);
    }
}
#[test] fn first_picture_does_not_read_entire_file_and_prefix_replays() {
    let nals = vec![vec![0x67, 1], vec![0x68, 1], vec![0x65, 0x80], vec![0x41, 0x80]];
    let mut bytes = annex(&nals); bytes.extend(annex(&vec![vec![0x41,0x80]; 100]));
    let (session, file) = setup(bytes);
    let (mut reader, timing, _) = run(h264_open_fs_reader(session, file)).unwrap(); assert!(timing.is_empty());
    let prefix = run(reader.next_nal()).unwrap().bytes; reader.replay(prefix);
    let mut pending = None; let mut sps = None; let mut pps = None; let mut missing = 0; let mut count = 0;
    let unit = run(h264_next_stream_access_unit(session, &mut reader, &mut pending, &mut sps, &mut pps, &mut missing, &mut count)).unwrap();
    assert_eq!(unit.nal_type, 5); assert!(!unit.sps.is_empty());
    assert!(RIG.lock().unwrap().read < file.data_len() as usize / 2);
}
#[test] fn read_errors_cancel_and_oversized_nals_stop() {
    let (session, file) = setup(annex(&[vec![0x65; H264_FS_PICTURE_CAP_BYTES + 32]]));
    let mut r = H264FileNalReader::new(session, file, None);
    assert!(run(r.next_nal()).is_none()); assert!(r.failed);
    let (session, file) = setup(annex(&[vec![0x65; 100]])); RIG.lock().unwrap().fail_at = 16;
    let mut r = H264FileNalReader::new(session, file, None);
    assert!(run(r.next_nal()).is_none()); assert!(r.failed);
    let (session, file) = setup(annex(&[vec![0x65; 10]])); RIG.lock().unwrap().cancel = true;
    let mut r = H264FileNalReader::new(session, file, None);
    assert!(run(r.next_nal()).is_none()); assert!(!r.failed); assert_eq!(RIG.lock().unwrap().read, 0);
}
#[test] fn three_readers_keep_independent_positions() {
    let nals = vec![vec![0x67,1], vec![0x68,1], vec![0x65,0x80], vec![0x41,0x80]];
    let (session, file) = setup(annex(&nals));
    let mut readers: Vec<_> = (0..3).map(|_| H264FileNalReader::new(session, file, None)).collect();
    for expected in nals {
        for r in &mut readers { assert_eq!(run(r.next_nal()).unwrap().bytes, annex(&[expected.clone()])); }
    }
    for r in &mut readers { assert!(run(r.next_nal()).is_none()); assert!(!r.failed); }
}
#[test] fn mp4_sample_payload_is_read_on_demand() {
    let mut bytes = vec![0; 1000]; bytes[500..507].copy_from_slice(&[0,0,0,3,0x65,0x80,0]);
    let (session, file) = setup(bytes);
    let track = Mp4AvcTrack { track_id: 1, timescale: 30, length_size: 4, colour: None, sps: vec![vec![0x67,1]], pps: vec![vec![0x68,1]], samples: vec![Mp4SampleRef { offset: 500, size: 7, keyframe: true, decode_time: 0, duration: 1, composition_offset: 0 }] };
    let mut r = H264FileNalReader::new(session, file, Some(track));
    let mut types = Vec::new(); while let Some(n) = run(r.next_nal()) { types.push(n.meta.nal_type); }
    assert_eq!(types, vec![9,7,8,5]); assert_eq!(RIG.lock().unwrap().read, 7); assert!(!r.failed);
}

#[test] fn real_mp4_moov_head_and_tail_match_buffered_vcl_and_timing() {
    for path in std::env::var("VIDEO_STREAM_FIXTURES").unwrap_or_default().split(':').filter(|p| !p.is_empty()) {
        let bytes = std::fs::read(path).unwrap();
        let expected = mp4_avc1_to_annexb(&bytes).unwrap();
        let (session, file) = setup(bytes);
        let (mut reader, timing, _) = run(h264_open_fs_reader(session, file)).unwrap();
        assert!(reader.streaming()); assert_eq!(timing, expected.timing);
        let mut expected_reader = H264MemoryNalReader::new(expected.annexb, "reference");
        let mut expected_vcl = Vec::new();
        while let Some(n) = run(expected_reader.next_nal()) { if matches!(n.meta.nal_type, 1|5) { expected_vcl.push(n.bytes); } }
        let mut actual_vcl = Vec::new();
        while let Some(n) = run(reader.next_nal()) { if matches!(n.meta.nal_type, 1|5) { actual_vcl.push(n.bytes); } }
        assert!(!reader.failed()); assert_eq!(actual_vcl, expected_vcl);
        let bytes = std::fs::read(path).unwrap();
        let (session, file) = setup(bytes);
        let (mut reader, _, _) = run(h264_open_fs_reader(session, file)).unwrap();
        let mut pending = None; let mut sps = None; let mut pps = None; let mut missing = 0; let mut count = 0; let mut pictures = 0;
        while let Some(unit) = run(h264_next_stream_access_unit(session, &mut reader, &mut pending, &mut sps, &mut pps, &mut missing, &mut count)) {
            assert!(!unit.sps.is_empty() && !unit.pps.is_empty()); pictures += 1;
        }
        assert_eq!(pictures, timing.len()); assert_eq!(missing, 0); assert!(!reader.failed());

        assert!(timing.windows(2).any(|p| p[1].pts < p[0].pts), "fixture must exercise B-frame timing");
    }
}

// Host oracle helper: consume the exact same incremental index to EOF.
async fn mkv_open_avc_track(
    session: crate::ui4::VideoPlaybackSession,
    file: crate::r::fs::trueosfs::FileReadHandle,
) -> Result<Mp4AvcTrack, &'static str> {
    let mut index = mkv_open_avc_index(session, file).await?;
    while index.next_group().await? {}
    if index.track.samples.is_empty() {
        return Err("mkv AVC track has no pictures");
    }
    Ok(index.track)
}

#[test] fn mkv_vints_reject_truncation_and_zero() {
    assert_eq!(mkv_vint(&[0x81], false).unwrap(), (1, 1));
    assert_eq!(mkv_vint(&[0x40, 0x80], false).unwrap(), (128, 2));
    assert!(mkv_vint(&[0], false).is_err());
    assert!(mkv_vint(&[0x40], false).is_err());
}
#[test] fn real_mkv_matches_ffprobe_timestamps_and_sample_bytes() {
    let paths = std::env::var("MKV_STREAM_FIXTURES").unwrap_or_default();
    for path in paths.split('|').filter(|p| !p.is_empty()) {
        let oracle = std::process::Command::new("ffprobe").args(["-v", "error", "-select_streams", "v:0", "-show_packets", "-show_entries", "packet=pts_time,size", "-of", "csv=p=0", path]).output().unwrap();
        assert!(oracle.status.success());
        let expected: Vec<(i64, usize)> = String::from_utf8(oracle.stdout).unwrap().lines().map(|line| {
            let fields: Vec<_> = line.split(',').collect();
            (((fields[0].parse::<f64>().unwrap() * 1e9).round()) as i64, fields[1].parse().unwrap())
        }).collect();
        let (session, file) = setup(std::fs::read(path).unwrap());
        let track = run(mkv_open_avc_track(session, file)).unwrap();
        assert_eq!(track.samples.len(), expected.len());
        for (sample, (pts, size)) in track.samples.iter().zip(expected) {
            assert_eq!(sample.decode_time as i64 + sample.composition_offset, pts);
            assert_eq!(sample.size, size);
        }
        assert!(RIG.lock().unwrap().read < file.data_len() as usize / 2 + 4096, "index must skip picture payloads");
        RIG.lock().unwrap().read = 0;
        let (mut reader, timing, source) = run(h264_open_fs_reader(session, file)).unwrap();
        assert_eq!(source, "trueosfs-stream-mkv-avc");
        assert!(timing.is_empty());
        assert!(reader.mkv_track().unwrap().samples.is_empty());
        assert!(RIG.lock().unwrap().read < 64 * 1024, "startup must read metadata only");
        let mut pending = None; let mut sps = None; let mut pps = None; let mut missing = 0; let mut count = 0;
        // Check packet conversion against the indexed length-prefixed bytes.
        for sample in track.samples.iter().take(100) {
            let unit = run(h264_next_stream_access_unit(session, &mut reader, &mut pending, &mut sps, &mut pps, &mut missing, &mut count)).unwrap();
            let rig = RIG.lock().unwrap();
            let payload = &rig.bytes[sample.offset..sample.offset + sample.size];
            let mut cursor = 0; let mut expected = Vec::new();
            while cursor < payload.len() {
                let size = payload[cursor..cursor+track.length_size].iter().fold(0usize, |v, b| (v<<8) | *b as usize);
                cursor += track.length_size;
                if matches!(payload[cursor] & 31, 1|5) { mp4_emit_annexb_nal(&mut expected, &payload[cursor..cursor+size]); }
                cursor += size;
            }
            let mut actual = H264MemoryNalReader::new(unit.data, "actual");
            let mut vcl = Vec::new();
            while let Some(nal) = run(actual.next_nal()) { if matches!(nal.meta.nal_type, 1|5) { vcl.extend(nal.bytes); } }
            assert_eq!(vcl, expected);
        }
        assert_eq!(missing, 0); assert!(!reader.failed());
        let mut index = run(mkv_open_avc_index(session, file)).unwrap();
        let mut incremental_timing = Vec::new(); let mut ranks = Vec::new(); let mut base = 0;
        let before = RIG.lock().unwrap().read;
        assert!(run(index.next_group()).unwrap());
        if track.samples.len() > 1000 {
            assert!(index.track.samples.len() < track.samples.len() / 10, "first GOP must not scan the episode");
            assert!(RIG.lock().unwrap().read - before < 64 * 1024, "first GOP header I/O must be bounded");
        }
        loop {
            let old_ranks = ranks.clone();
            assert!(h264_extend_mkv_timing(&index.track, &mut incremental_timing, &mut ranks, &mut base));
            assert_eq!(&ranks[..old_ranks.len()], old_ranks.as_slice(), "lookahead must preserve published ranks");
            assert!(!h264_extend_mkv_timing(&index.track, &mut incremental_timing, &mut ranks, &mut base));
            if !run(index.next_group()).unwrap() { break; }
        }
        let mut ordered = vec![None; ranks.len()];
        for (i, rank) in ranks.iter().copied().enumerate() {
            assert!(ordered[rank].replace(incremental_timing[i].pts).is_none());
        }
        assert!(ordered.windows(2).all(|pair| pair[0] <= pair[1]), "B frames must present in PTS order across GOPs");
        assert_eq!(incremental_timing.len(), track.samples.len());
        // Selected video lacing must fail explicitly, never decode lace headers.
        let flag_offset = track.samples[0].offset - 1;
        let original_flags = RIG.lock().unwrap().bytes[flag_offset];
        RIG.lock().unwrap().bytes[flag_offset] |= 0x02;
        assert_eq!(run(mkv_open_avc_track(session, file)).err(), Some("mkv laced or invisible AVC blocks unsupported"));
        RIG.lock().unwrap().bytes[flag_offset] = original_flags;
        RIG.lock().unwrap().short = 3;
        assert_eq!(run(mkv_open_avc_track(session, file)).unwrap().samples.len(), track.samples.len());
        RIG.lock().unwrap().fail_at = 0;
        assert!(run(mkv_open_avc_track(session, file)).is_err());
        RIG.lock().unwrap().cancel = true;
        assert!(run(mkv_open_avc_track(session, file)).is_err());
    }
}
'''
with tempfile.TemporaryDirectory(prefix='trueos-video-streaming-') as tmp:
    src = Path(tmp) / 'test.rs'; binary = Path(tmp) / 'test'
    fixtures = []
    if shutil.which('ffmpeg'):
        for fast in [False, True]:
            fixture = Path(tmp) / ('head.mp4' if fast else 'tail.mp4')
            command = ['ffmpeg', '-v', 'error', '-f', 'lavfi', '-i', 'color=c=blue:s=32x32:r=30', '-frames:v', '12', '-c:v', 'libx264', '-bf', '2']
            if fast:
                command += ['-movflags', '+faststart']
            subprocess.run(command + ['-y', str(fixture)], check=True)
            fixtures.append(str(fixture))
    mkv_fixtures = []
    if shutil.which('ffmpeg'):
        fixture = Path(tmp) / 'bframes.mkv'
        subprocess.run(['ffmpeg', '-v', 'error', '-f', 'lavfi', '-i',
                        'color=c=blue:s=32x32:r=24000/1001', '-frames:v', '24',
                        '-c:v', 'libx264', '-bf', '2', '-g', '4', '-y', str(fixture)], check=True)
        mkv_fixtures.append(str(fixture))
    import os
    if os.environ.get('MKV_STREAM_FIXTURE'):
        mkv_fixtures.append(os.environ['MKV_STREAM_FIXTURE'])
    src.write_text(harness + production)
    subprocess.run(['rustc', '--edition=2024', '--test', str(src), '-o', str(binary)], check=True)
    import os
    subprocess.run([str(binary), '--test-threads=1'], check=True, env={**os.environ, 'VIDEO_STREAM_FIXTURES': ':'.join(fixtures), 'MKV_STREAM_FIXTURES': '|'.join(mkv_fixtures)})
