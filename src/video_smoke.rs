extern crate alloc;

use alloc::vec::Vec;
use core::cmp::min;

use embassy_time::{Duration as EmbassyDuration, Timer};

use crate::v::net::https::fetch_https_body_async;
use crate::vga::{self, Image};
use vp9_parser::{FrameType, Subsampling, Vp9Parser};

const VIDEO_SMOKE_URL: &str = "https://avtshare01.rz.tu-ilmenau.de/avt-vqdb-uhd-1/test_1/segments/bigbuck_bunny_8bit_750kbps_360p_60.0fps_vp9.mkv";
const FETCH_TIMEOUT_MS: u32 = 30_000;
const FETCH_MAX_BYTES: usize = 32 * 1024 * 1024;
const TARGET_WIDTH: usize = 640;
const TARGET_HEIGHT: usize = 360;
const TARGET_FPS: u32 = 60;
const PREVIEW_ORIGIN_X: usize = 0;
const PREVIEW_ORIGIN_Y: usize = 0;

pub struct VgaImage {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u32>,
}

#[derive(Clone, Copy)]
enum ContainerKind {
    Ivf,
    Matroska,
    Unknown,
}

struct PreviewRenderer {
    parser: Vp9Parser,
    frame_seq: u32,
    fallback_width: usize,
    fallback_height: usize,
}

impl PreviewRenderer {
    fn new(width: usize, height: usize) -> Self {
        Self {
            parser: Vp9Parser::new(),
            frame_seq: 0,
            fallback_width: width,
            fallback_height: height,
        }
    }

    fn reset_for_replay(&mut self) {
        self.parser.reset();
        self.frame_seq = 0;
    }

    fn parse_and_render(&mut self, packet: &[u8]) -> Option<VgaImage> {
        let frames = self.parser.parse_packet(packet.to_vec()).ok()?;
        for frame in frames.iter() {
            if !frame.show_frame() || frame.show_existing_frame() {
                continue;
            }
            let image = self.render_frame_preview(frame, packet);
            self.frame_seq = self.frame_seq.wrapping_add(1);
            return Some(image);
        }
        None
    }

    fn render_frame_preview(&mut self, frame: &vp9_parser::Frame, packet: &[u8]) -> VgaImage {
        let width = match frame.width() as usize {
            0 => self.fallback_width,
            value => value,
        };
        let height = match frame.height() as usize {
            0 => self.fallback_height,
            value => value,
        };
        self.fallback_width = width;
        self.fallback_height = height;

        let mut pixels = vec![0u32; width * height];
        let payload = if frame.tile_data().is_empty() {
            if frame.compressed_header_and_tile_data().is_empty() {
                packet
            } else {
                frame.compressed_header_and_tile_data()
            }
        } else {
            frame.tile_data()
        };
        let payload_len = payload.len().max(1);
        self.paint_payload_raster(&mut pixels, width, height, payload, payload_len, frame);

        self.overlay_tile_boundaries(&mut pixels, width, height, frame);
        self.overlay_frame_border(&mut pixels, width, height, frame.frame_type());

        VgaImage {
            width,
            height,
            pixels,
        }
    }

    fn paint_payload_raster(
        &self,
        pixels: &mut [u32],
        width: usize,
        height: usize,
        payload: &[u8],
        payload_len: usize,
        frame: &vp9_parser::Frame,
    ) {
        if width == 0 || height == 0 {
            return;
        }

        let source_w = min(width.max(1), 256).min(payload_len.max(1));
        let source_h = payload_len.div_ceil(source_w);
        let (u_tint, v_tint) = subtle_tint(frame.subsampling(), frame.frame_type());

        for py in 0..height {
            let src_y = py * source_h / height;
            for px in 0..width {
                let src_x = px * source_w / width;
                let idx = min(src_y * source_w + src_x, payload_len - 1);
                let idx_r = min(idx.saturating_add(1), payload_len - 1);
                let idx_d = min(idx.saturating_add(source_w), payload_len - 1);
                let idx_l = idx.saturating_sub(1);

                let lap = payload[idx]
                    .abs_diff(payload[idx_r])
                    .saturating_add(payload[idx].abs_diff(payload[idx_d]))
                    .saturating_add(payload[idx].abs_diff(payload[idx_l]));
                let base = ((payload[idx] as u16 + payload[idx_r] as u16 + payload[idx_d] as u16)
                    / 3) as u8;
                let edge_boost = (lap / 3).min(48);
                let screen_shade = (((px * 6) / width.max(1)) as u8)
                    .saturating_add(((py * 4) / height.max(1)) as u8);
                let y_val = base.saturating_add(edge_boost).saturating_add(screen_shade);

                pixels[py * width + px] = yuv_to_rgb32(y_val, u_tint, v_tint);
            }
        }
    }

    fn overlay_tile_boundaries(
        &self,
        pixels: &mut [u32],
        width: usize,
        height: usize,
        frame: &vp9_parser::Frame,
    ) {
        let tile_cols = 1usize << usize::from(frame.tile_cols_log2());
        let tile_rows = 1usize << usize::from(frame.tile_rows_log2());
        let sb_cols = width.div_ceil(64);
        let sb_rows = height.div_ceil(64);

        for tile_col in 1..tile_cols {
            let boundary_sb = (tile_col * sb_cols) / tile_cols;
            let x = min(boundary_sb * 64, width.saturating_sub(1));
            draw_vline(pixels, width, height, x, 0, height, pack_rgb(150, 150, 150));
        }
        for tile_row in 1..tile_rows {
            let boundary_sb = (tile_row * sb_rows) / tile_rows;
            let y = min(boundary_sb * 64, height.saturating_sub(1));
            draw_hline(pixels, width, height, 0, y, width, pack_rgb(150, 150, 150));
        }
    }

    fn overlay_frame_border(
        &self,
        pixels: &mut [u32],
        width: usize,
        height: usize,
        frame_type: FrameType,
    ) {
        let color = match frame_type {
            FrameType::KeyFrame => pack_rgb(64, 255, 96),
            FrameType::NonKeyFrame => pack_rgb(255, 96, 64),
        };
        draw_rect_outline(pixels, width, height, 0, 0, width, height, color);
    }
}

struct IvfSliceReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> IvfSliceReader<'a> {
    fn new(data: &'a [u8]) -> Option<Self> {
        if data.len() < 32 || &data[0..4] != b"DKIF" {
            return None;
        }
        Some(Self { data, pos: 32 })
    }

    fn next_packet(&mut self) -> Option<&'a [u8]> {
        if self.pos + 12 > self.data.len() {
            return None;
        }
        let size = u32::from_le_bytes([
            self.data[self.pos],
            self.data[self.pos + 1],
            self.data[self.pos + 2],
            self.data[self.pos + 3],
        ]) as usize;
        let data_start = self.pos + 12;
        let data_end = data_start.checked_add(size)?;
        if data_end > self.data.len() {
            return None;
        }
        self.pos = data_end;
        Some(&self.data[data_start..data_end])
    }
}

fn detect_container(bytes: &[u8]) -> ContainerKind {
    if bytes.len() >= 4 && &bytes[0..4] == b"DKIF" {
        ContainerKind::Ivf
    } else if bytes.len() >= 4
        && bytes[0] == 0x1A
        && bytes[1] == 0x45
        && bytes[2] == 0xDF
        && bytes[3] == 0xA3
    {
        ContainerKind::Matroska
    } else {
        ContainerKind::Unknown
    }
}

fn read_ebml_vint(data: &[u8], pos: usize) -> Option<(u64, usize)> {
    let first = *data.get(pos)?;
    if first == 0 {
        return None;
    }
    let len = first.leading_zeros() as usize + 1;
    if len > 8 || pos + len > data.len() {
        return None;
    }

    let mut value = (first & ((1u8 << (8 - len)) - 1)) as u64;
    for idx in 1..len {
        value = (value << 8) | data[pos + idx] as u64;
    }
    Some((value, len))
}

fn extract_matroska_block_payload(block: &[u8]) -> Option<&[u8]> {
    let (_track, track_len) = read_ebml_vint(block, 0)?;
    if block.len() < track_len + 3 {
        return None;
    }
    let flags = block[track_len + 2];
    let lacing = (flags >> 1) & 0x03;
    if lacing != 0 {
        return None;
    }
    Some(&block[track_len + 3..])
}

fn next_matroska_packet<'a>(data: &'a [u8], pos: &mut usize) -> Option<&'a [u8]> {
    while *pos + 2 < data.len() {
        let id = data[*pos];
        if id != 0xA3 && id != 0xA1 {
            *pos += 1;
            continue;
        }

        let Some((size, size_len)) = read_ebml_vint(data, *pos + 1) else {
            *pos += 1;
            continue;
        };
        let payload_start = *pos + 1 + size_len;
        let payload_end = match payload_start.checked_add(size as usize) {
            Some(value) => value,
            None => {
                *pos += 1;
                continue;
            }
        };
        if payload_end > data.len() {
            *pos += 1;
            continue;
        }

        *pos = payload_end;
        if let Some(packet) = extract_matroska_block_payload(&data[payload_start..payload_end]) {
            if !packet.is_empty() {
                return Some(packet);
            }
        }
    }
    None
}

async fn preview_packets(
    renderer: &mut PreviewRenderer,
    bytes: &[u8],
    container: ContainerKind,
) -> usize {
    let frame_interval = EmbassyDuration::from_millis((1000 / TARGET_FPS) as u64);
    let mut shown_frames = 0usize;
    let mut packet_count = 0usize;

    match container {
        ContainerKind::Ivf => {
            let Some(mut ivf) = IvfSliceReader::new(bytes) else {
                return 0;
            };
            while let Some(packet) = ivf.next_packet() {
                packet_count += 1;
                if let Some(image) = renderer.parse_and_render(packet) {
                    blit_preview(&image, PREVIEW_ORIGIN_X, PREVIEW_ORIGIN_Y);
                    shown_frames += 1;
                    log_preview_frame(shown_frames, packet_count, "ivf", image.width, image.height);
                    Timer::after(frame_interval).await;
                }
            }
        }
        ContainerKind::Matroska | ContainerKind::Unknown => {
            let mut pos = 0usize;
            while let Some(packet) = next_matroska_packet(bytes, &mut pos) {
                packet_count += 1;
                if let Some(image) = renderer.parse_and_render(packet) {
                    blit_preview(&image, PREVIEW_ORIGIN_X, PREVIEW_ORIGIN_Y);
                    shown_frames += 1;
                    log_preview_frame(shown_frames, packet_count, "mkv", image.width, image.height);
                    Timer::after(frame_interval).await;
                }
            }
        }
    }

    shown_frames
}

fn blit_preview(image: &VgaImage, origin_x: usize, origin_y: usize) {
    let image = Image {
        width: image.width,
        height: image.height,
        pixels: image.pixels.as_slice(),
    };
    let _ = vga::blit_image(origin_x, origin_y, &image);
}

fn log_preview_frame(
    shown_frames: usize,
    packet_count: usize,
    container: &str,
    width: usize,
    height: usize,
) {
    crate::log!(
        "video-smoke: preview frame={} packet={} container={} size={}x{}\n",
        shown_frames,
        packet_count,
        container,
        width,
        height
    );
}

fn chroma_tints(subsampling: Subsampling, frame_type: FrameType, seed: u8) -> (u8, u8) {
    let base = match subsampling {
        Subsampling::Yuv420 => (110u8, 170u8),
        Subsampling::Yuv422 => (96u8, 190u8),
        Subsampling::Yuv440 => (140u8, 150u8),
        Subsampling::Yuv444 => (128u8, 208u8),
    };
    match frame_type {
        FrameType::KeyFrame => (base.0.saturating_add(seed & 0x0F), base.1),
        FrameType::NonKeyFrame => (base.0, base.1.saturating_sub(seed & 0x0F)),
    }
}

fn subtle_tint(subsampling: Subsampling, frame_type: FrameType) -> (u8, u8) {
    let (u, v) = match subsampling {
        Subsampling::Yuv420 => (128u8, 132u8),
        Subsampling::Yuv422 => (126u8, 136u8),
        Subsampling::Yuv440 => (132u8, 128u8),
        Subsampling::Yuv444 => (130u8, 138u8),
    };
    match frame_type {
        FrameType::KeyFrame => (u.saturating_add(4), v),
        FrameType::NonKeyFrame => (u, v.saturating_sub(2)),
    }
}

#[inline(always)]
fn partition_mode(seed: u8, frame_type: FrameType, depth: usize) -> u8 {
    let depth_bias = (depth as u8).wrapping_mul(19);
    let value = seed.wrapping_add(depth_bias) & 0x0F;
    match frame_type {
        FrameType::KeyFrame => {
            if value < 2 {
                0
            } else if value < 5 {
                1
            } else if value < 8 {
                2
            } else {
                3
            }
        }
        FrameType::NonKeyFrame => {
            if value < 4 {
                0
            } else if value < 8 {
                1
            } else if value < 12 {
                2
            } else {
                3
            }
        }
    }
}

#[inline(always)]
fn split_point(len: usize) -> usize {
    if len <= 8 {
        len
    } else {
        (len / 2).max(8).min(len.saturating_sub(8))
    }
}

#[inline(always)]
fn yuv_to_rgb32(y: u8, u: u8, v: u8) -> u32 {
    let y = y as i32;
    let u = u as i32 - 128;
    let v = v as i32 - 128;

    let r = y + ((359 * v) >> 8);
    let g = y - ((88 * u + 183 * v) >> 8);
    let b = y + ((454 * u) >> 8);

    pack_rgb(clamp_to_u8(r), clamp_to_u8(g), clamp_to_u8(b))
}

#[inline(always)]
fn clamp_to_u8(value: i32) -> u8 {
    if value < 0 {
        0
    } else if value > 255 {
        255
    } else {
        value as u8
    }
}

#[inline(always)]
fn pack_rgb(r: u8, g: u8, b: u8) -> u32 {
    ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

fn fill_rect(
    pixels: &mut [u32],
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    rect_w: usize,
    rect_h: usize,
    color: u32,
) {
    let end_x = min(x.saturating_add(rect_w), width);
    let end_y = min(y.saturating_add(rect_h), height);
    for py in y..end_y {
        let row = py * width;
        for px in x..end_x {
            pixels[row + px] = color;
        }
    }
}

fn draw_rect_outline(
    pixels: &mut [u32],
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    rect_w: usize,
    rect_h: usize,
    color: u32,
) {
    if rect_w == 0 || rect_h == 0 {
        return;
    }
    draw_hline(pixels, width, height, x, y, rect_w, color);
    draw_hline(
        pixels,
        width,
        height,
        x,
        y + rect_h.saturating_sub(1),
        rect_w,
        color,
    );
    draw_vline(pixels, width, height, x, y, rect_h, color);
    draw_vline(
        pixels,
        width,
        height,
        x + rect_w.saturating_sub(1),
        y,
        rect_h,
        color,
    );
}

fn draw_hline(
    pixels: &mut [u32],
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    len: usize,
    color: u32,
) {
    if y >= height {
        return;
    }
    let end = min(x.saturating_add(len), width);
    for px in x..end {
        pixels[y * width + px] = color;
    }
}

fn draw_vline(
    pixels: &mut [u32],
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    len: usize,
    color: u32,
) {
    if x >= width {
        return;
    }
    let end = min(y.saturating_add(len), height);
    for py in y..end {
        pixels[py * width + x] = color;
    }
}

async fn stream_preview_frames(bytes: &[u8]) {
    let container = detect_container(bytes);
    crate::log!(
        "video-smoke: container={}\n",
        match container {
            ContainerKind::Ivf => "ivf",
            ContainerKind::Matroska => "mkv",
            ContainerKind::Unknown => "unknown",
        }
    );

    let mut renderer = PreviewRenderer::new(TARGET_WIDTH, TARGET_HEIGHT);
    let mut replay_pass = 0usize;
    loop {
        renderer.reset_for_replay();
        let shown = preview_packets(&mut renderer, bytes, container).await;
        if shown == 0 {
            crate::log!("video-smoke: no VP9 frames rendered\n");
            break;
        }

        replay_pass = replay_pass.saturating_add(1);
        crate::log!(
            "video-smoke: replay pass={} frames={}\n",
            replay_pass,
            shown
        );
        Timer::after(EmbassyDuration::from_millis(150)).await;
    }
}

#[embassy_executor::task]
pub async fn video_smoke_task() {
    crate::log!("video-smoke: start url={}\n", VIDEO_SMOKE_URL);
    for attempt in 1..=3 {
        match fetch_https_body_async(VIDEO_SMOKE_URL, FETCH_TIMEOUT_MS, FETCH_MAX_BYTES).await {
            Ok(bytes) => {
                crate::log!(
                    "video-smoke: fetch ok bytes={} attempt={}\n",
                    bytes.len(),
                    attempt
                );
                stream_preview_frames(bytes.as_slice()).await;
                break;
            }
            Err(e) => {
                crate::log!(
                    "video-smoke: fetch failed attempt={} err={:?}\n",
                    attempt,
                    e
                );
                Timer::after(EmbassyDuration::from_millis(600)).await;
            }
        }
    }
    crate::log!("video-smoke: done\n");
}
