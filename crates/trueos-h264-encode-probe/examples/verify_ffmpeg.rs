use std::{
    io::{Read, Write},
    process::{Command, Stdio},
    thread,
    time::Instant,
};

fn main() {
    let encode_started = Instant::now();
    let proof = trueos_h264_encode_probe::encode_diagnostic_sequence_512x512_60()
        .expect("encode 512x512x60 H.264 diagnostic sequence");
    let encode_us = encode_started.elapsed().as_micros();
    let expected = trueos_h264_encode_probe::diagnostic_sequence_visible_i420();

    let mut child = Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-f",
            "h264",
            "-i",
            "pipe:0",
            "-frames:v",
            "60",
            "-pix_fmt",
            "yuv420p",
            "-f",
            "rawvideo",
            "pipe:1",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn ffmpeg");
    let mut stdin = child.stdin.take().expect("ffmpeg stdin");
    let encoded = proof.annex_b;
    let writer = thread::spawn(move || {
        stdin.write_all(encoded.as_slice()).expect("feed ffmpeg");
    });
    let mut decoded = Vec::new();
    child
        .stdout
        .take()
        .expect("ffmpeg stdout")
        .read_to_end(&mut decoded)
        .expect("read decoded I420");
    writer.join().expect("join ffmpeg input writer");
    let status = child.wait().expect("wait for ffmpeg");
    assert!(status.success(), "ffmpeg rejected the generated H.264 access unit");
    assert_eq!(decoded, expected, "decoded I420 differs from the source frame");

    let metrics = proof.metrics;
    eprintln!(
        "h264-encode-proof: PASS decoder=ffmpeg visible={}x{} coded={}x{} frames={} macroblocks_per_frame={} source_bytes={} encoded_bytes={} sps={} pps={} frame_min={} frame_max={} encode_us={} source_fnv=0x{:08X} encoded_fnv=0x{:08X}",
        metrics.visible_width,
        metrics.visible_height,
        metrics.coded_width,
        metrics.coded_height,
        metrics.frames,
        metrics.macroblocks_per_frame,
        metrics.source_bytes,
        metrics.encoded_bytes,
        metrics.sps_bytes,
        metrics.pps_bytes,
        metrics.frame_bytes_min,
        metrics.frame_bytes_max,
        encode_us,
        metrics.source_fnv1a32,
        metrics.encoded_fnv1a32,
    );
}
