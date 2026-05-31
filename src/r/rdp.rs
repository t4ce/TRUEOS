//! TRUEOS Remote Draw Protocol server.
//!
//! This is the one-way monitor/control point for remote gfx command replay.  The
//! first step is deliberately small: keep a TCP listener alive on port 100,
//! track connected clients, and send a compact HELLO frame so desktop clients
//! can validate they reached the right service.

extern crate alloc;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use embassy_executor::task;
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

use crate::net::adapter::{
    NetCommand, NetEvent, NetHandle, NetQueue, SocketKind, register_app_queues,
};

pub const TRUEOS_RDP_PORT: u16 = crate::allports::services::TRUEOS_RDP_TCP_PORT;
const OWNER: &str = "trueos-rdp";
const PROTOCOL_VERSION: u16 = 1;
const MSG_HELLO: u16 = 1;
const MSG_BEGIN_FRAME: u16 = 2;
const MSG_END_FRAME: u16 = 3;
const MSG_SET_BLEND: u16 = 4;
const MSG_SET_SAMPLER: u16 = 5;
const MSG_SET_SCISSOR: u16 = 6;
const MSG_CLEAR_SCISSOR: u16 = 7;
const MSG_SET_RENDER_TARGET: u16 = 8;
const MSG_CLEAR_RENDER_TARGET: u16 = 9;
const MSG_CLEAR_RECT: u16 = 10;
const MSG_TEXTURE_RGBA: u16 = 11;
const MSG_TEXTURE_PNG: u16 = 12;
const MSG_TEXTURE_JPEG: u16 = 13;
const MSG_TEXTURE_SVG: u16 = 14;
const MSG_DRAW_RGB_TRIANGLES: u16 = 15;
const MSG_DRAW_TEX_TRIANGLES: u16 = 16;
const MSG_RESOURCE_SNAPSHOT_BEGIN: u16 = 17;
const MSG_RESOURCE_SNAPSHOT_END: u16 = 18;
const MSG_INPUT_TABLET_ABS: u16 = 100;
const MSG_INPUT_KEYBOARD_BOOT: u16 = 101;
const CAP_ONE_WAY_MONITOR: u32 = 1;
const CAP_GFX_COMMAND_STREAM: u32 = 1 << 1;
const CAP_RESOURCE_SNAPSHOT: u32 = 1 << 2;
const CAP_ABSOLUTE_TABLET_INPUT: u32 = 1 << 3;
const RDP_TEXTURE_CACHE_CAP: usize = 512;
const RDP_INPUT_MAX_FRAME_BYTES: usize = 1024;

#[derive(Clone)]
struct CachedTexture {
    tex_id: u32,
    msg: u16,
    fields: Vec<u32>,
    data: Vec<u8>,
    seq: u32,
}

struct ClientInputBuffer {
    handle: NetHandle,
    bytes: Vec<u8>,
}

static TRUEOS_RDP_STARTED: AtomicBool = AtomicBool::new(false);
static TRUEOS_RDP_CLIENTS: AtomicU32 = AtomicU32::new(0);
static TRUEOS_RDP_DROPPED_SENDS: AtomicU32 = AtomicU32::new(0);
static TRUEOS_RDP_RESOURCE_SEQ: AtomicU32 = AtomicU32::new(1);
static TRUEOS_RDP_TEXTURE_CACHE_BYTES: AtomicU32 = AtomicU32::new(0);
static TRUEOS_RDP_COMMAND_QUEUE: Mutex<Option<&'static NetQueue<NetCommand>>> = Mutex::new(None);
static TRUEOS_RDP_CLIENT_HANDLES: Mutex<Vec<NetHandle>> = Mutex::new(Vec::new());
static TRUEOS_RDP_TEXTURE_CACHE: Mutex<Vec<CachedTexture>> = Mutex::new(Vec::new());

#[inline]
pub fn client_count() -> u32 {
    TRUEOS_RDP_CLIENTS.load(Ordering::Acquire)
}

#[inline]
pub fn has_clients() -> bool {
    client_count() != 0
}

#[inline]
pub fn cached_texture_count() -> u32 {
    TRUEOS_RDP_TEXTURE_CACHE.lock().len() as u32
}

#[inline]
pub fn cached_texture_bytes() -> u32 {
    TRUEOS_RDP_TEXTURE_CACHE_BYTES.load(Ordering::Acquire)
}

fn active_view_dimensions() -> (u32, u32) {
    crate::intel::active_scanout_dimensions()
        .or_else(|| {
            crate::limine::framebuffer_response()
                .and_then(|resp| resp.framebuffers().first().copied())
                .map(|fb| (fb.width as u32, fb.height as u32))
        })
        .unwrap_or((1280, 800))
}

fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn begin_payload(msg: u16, extra_capacity: usize) -> Vec<u8> {
    let mut payload = Vec::with_capacity(8 + extra_capacity);
    payload.extend_from_slice(b"TRDP");
    push_u16(&mut payload, PROTOCOL_VERSION);
    push_u16(&mut payload, msg);
    payload
}

fn frame_from_payload(payload: Vec<u8>) -> Vec<u8> {
    let mut frame = Vec::with_capacity(4 + payload.len());
    push_u32(&mut frame, payload.len().min(u32::MAX as usize) as u32);
    frame.extend_from_slice(payload.as_slice());
    frame
}

fn hello_frame() -> Vec<u8> {
    let (view_w, view_h) = active_view_dimensions();
    let caps = CAP_ONE_WAY_MONITOR
        | CAP_GFX_COMMAND_STREAM
        | CAP_RESOURCE_SNAPSHOT
        | CAP_ABSOLUTE_TABLET_INPUT;

    let mut payload = begin_payload(MSG_HELLO, 16);
    push_u32(&mut payload, view_w);
    push_u32(&mut payload, view_h);
    push_u32(&mut payload, caps);

    frame_from_payload(payload)
}

fn input_buffer_mut<'a>(
    buffers: &'a mut Vec<ClientInputBuffer>,
    handle: NetHandle,
) -> &'a mut Vec<u8> {
    if let Some(idx) = buffers.iter().position(|buffer| buffer.handle == handle) {
        return &mut buffers[idx].bytes;
    }
    buffers.push(ClientInputBuffer {
        handle,
        bytes: Vec::new(),
    });
    let idx = buffers.len() - 1;
    &mut buffers[idx].bytes
}

fn read_u16(data: &[u8], off: usize) -> Option<u16> {
    Some(u16::from_le_bytes(data.get(off..off + 2)?.try_into().ok()?))
}

fn read_u32(data: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(off..off + 4)?.try_into().ok()?))
}

fn handle_client_payload(handle: NetHandle, payload: &[u8]) {
    if payload.len() < 8 || payload.get(0..4) != Some(b"TRDP") {
        return;
    }
    let Some(version) = read_u16(payload, 4) else {
        return;
    };
    if version != PROTOCOL_VERSION {
        return;
    }
    let Some(msg) = read_u16(payload, 6) else {
        return;
    };
    let body = &payload[8..];

    match msg {
        MSG_INPUT_TABLET_ABS => {
            if body.len() < 20 {
                return;
            }

            let slot_id = crate::usb3::hid::rdp_tablet_slot_id();
            let x_q16 = read_u32(body, 4).unwrap_or(0).min(65535);
            let y_q16 = read_u32(body, 8).unwrap_or(0).min(65535);
            let buttons_down = read_u32(body, 12).unwrap_or(0);
            let flags = read_u32(body, 16).unwrap_or(0);
            let x = f64::from(x_q16) / 65535.0;
            let y = f64::from(y_q16) / 65535.0;
            crate::usb3::hid::inject_virtual_tablet_absolute_event(
                slot_id,
                x,
                y,
                buttons_down,
                flags,
            );

            static TABLET_INPUT_LOGS: AtomicU32 = AtomicU32::new(0);
            let n = TABLET_INPUT_LOGS.fetch_add(1, Ordering::Relaxed);
            if n < 8 {
                crate::log!(
                    "trueos-rdp: input tablet handle={} slot={} x={} y={} buttons=0x{:X} flags=0x{:X}\n",
                    handle.0,
                    slot_id,
                    x_q16,
                    y_q16,
                    buttons_down,
                    flags
                );
            }
        }
        MSG_INPUT_KEYBOARD_BOOT => {
            if body.len() < 16 {
                return;
            }
            let modifiers = read_u32(body, 4).unwrap_or(0).min(u8::MAX as u32) as u8;
            let keys_lo = read_u32(body, 8).unwrap_or(0).to_le_bytes();
            let keys_hi = read_u32(body, 12).unwrap_or(0).to_le_bytes();
            let keys = [keys_lo[0], keys_lo[1], keys_lo[2], keys_lo[3], keys_hi[0], keys_hi[1]];
            crate::usb3::hid::inject_virtual_keyboard_boot_report(modifiers, keys);

            static KEYBOARD_INPUT_LOGS: AtomicU32 = AtomicU32::new(0);
            let n = KEYBOARD_INPUT_LOGS.fetch_add(1, Ordering::Relaxed);
            if n < 8 {
                crate::log!(
                    "trueos-rdp: input keyboard handle={} mods=0x{:02X} keys=[{},{},{},{},{},{}]\n",
                    handle.0,
                    modifiers,
                    keys[0],
                    keys[1],
                    keys[2],
                    keys[3],
                    keys[4],
                    keys[5],
                );
            }
        }
        _ => {}
    }
}

fn handle_client_data(handle: NetHandle, data: Vec<u8>, buffers: &mut Vec<ClientInputBuffer>) {
    let buffer = input_buffer_mut(buffers, handle);
    buffer.extend_from_slice(data.as_slice());

    loop {
        if buffer.len() < 4 {
            return;
        }
        let Some(len) = read_u32(buffer.as_slice(), 0).map(|len| len as usize) else {
            buffer.clear();
            return;
        };
        if len > RDP_INPUT_MAX_FRAME_BYTES {
            crate::log!(
                "trueos-rdp: input frame too large handle={} bytes={} clearing\n",
                handle.0,
                len
            );
            buffer.clear();
            return;
        }
        let total = 4usize.saturating_add(len);
        if buffer.len() < total {
            return;
        }
        let payload = buffer[4..total].to_vec();
        buffer.drain(0..total);
        handle_client_payload(handle, payload.as_slice());
    }
}

fn publish_client_handles(clients: &[NetHandle]) {
    TRUEOS_RDP_CLIENTS.store(clients.len() as u32, Ordering::Release);
    *TRUEOS_RDP_CLIENT_HANDLES.lock() = clients.to_vec();
}

fn remove_client_handle(
    clients: &mut Vec<NetHandle>,
    input_buffers: &mut Vec<ClientInputBuffer>,
    handle: NetHandle,
) -> bool {
    let before = clients.len();
    clients.retain(|client| *client != handle);
    input_buffers.retain(|buffer| buffer.handle != handle);
    if clients.len() == before {
        return false;
    }
    if clients.is_empty() {
        crate::usb3::hid::remove_rdp_tablet();
    }
    publish_client_handles(clients.as_slice());
    true
}

fn clear_client_handles(clients: &mut Vec<NetHandle>, input_buffers: &mut Vec<ClientInputBuffer>) -> usize {
    let cleared = clients.len();
    if cleared == 0 {
        return 0;
    }
    clients.clear();
    input_buffers.clear();
    crate::usb3::hid::remove_rdp_tablet();
    publish_client_handles(clients.as_slice());
    cleared
}

fn protocol_frame(msg: u16, fields: &[u32], data: &[u8]) -> Vec<u8> {
    let mut payload = begin_payload(msg, fields.len().saturating_mul(4).saturating_add(data.len()));
    for field in fields {
        push_u32(&mut payload, *field);
    }
    payload.extend_from_slice(data);
    frame_from_payload(payload)
}

fn broadcast_frame(frame: Vec<u8>) {
    if client_count() == 0 {
        return;
    }

    let Some(cmds) = *TRUEOS_RDP_COMMAND_QUEUE.lock() else {
        return;
    };
    let clients = TRUEOS_RDP_CLIENT_HANDLES.lock().clone();
    for handle in clients {
        if cmds
            .push(NetCommand::SendTcp {
                handle,
                data: frame.clone(),
            })
            .is_err()
        {
            let n = TRUEOS_RDP_DROPPED_SENDS.fetch_add(1, Ordering::Relaxed);
            if n < 16 {
                crate::log!(
                    "trueos-rdp: send queue full handle={} bytes={} dropped={}\n",
                    handle.0,
                    frame.len(),
                    n + 1
                );
            }
        }
    }
}

fn publish_small(msg: u16, fields: &[u32]) {
    if !has_clients() {
        return;
    }
    broadcast_frame(protocol_frame(msg, fields, &[]));
}

fn publish_bytes(msg: u16, fields: &[u32], data: &[u8]) {
    if !has_clients() {
        return;
    }
    broadcast_frame(protocol_frame(msg, fields, data));
}

fn encoded_msg(kind: crate::r::resource_monitor::EncodedKind) -> u16 {
    match kind {
        crate::r::resource_monitor::EncodedKind::Png => MSG_TEXTURE_PNG,
        crate::r::resource_monitor::EncodedKind::Jpeg => MSG_TEXTURE_JPEG,
        crate::r::resource_monitor::EncodedKind::Svg => MSG_TEXTURE_SVG,
    }
}

fn encoded_fields(asset: &crate::r::resource_monitor::EncodedAsset) -> [u32; 3] {
    [
        asset.tex_id,
        asset.flags,
        asset.bytes.len().min(u32::MAX as usize) as u32,
    ]
}

fn update_cached_texture(tex_id: u32, msg: u16, fields: &[u32], data: &[u8]) -> u32 {
    if tex_id == 0 {
        return TRUEOS_RDP_RESOURCE_SEQ.load(Ordering::Acquire);
    }

    let seq = TRUEOS_RDP_RESOURCE_SEQ.fetch_add(1, Ordering::AcqRel);
    let mut cache = TRUEOS_RDP_TEXTURE_CACHE.lock();
    if let Some(entry) = cache.iter_mut().find(|entry| entry.tex_id == tex_id) {
        entry.msg = msg;
        entry.fields.clear();
        entry.fields.extend_from_slice(fields);
        entry.data.clear();
        entry.data.extend_from_slice(data);
        entry.seq = seq;
    } else {
        if cache.len() >= RDP_TEXTURE_CACHE_CAP {
            if let Some((oldest_idx, _)) =
                cache.iter().enumerate().min_by_key(|(_, entry)| entry.seq)
            {
                cache.remove(oldest_idx);
            }
        }
        cache.push(CachedTexture {
            tex_id,
            msg,
            fields: fields.to_vec(),
            data: data.to_vec(),
            seq,
        });
    }

    let bytes = cache
        .iter()
        .fold(0usize, |acc, entry| acc.saturating_add(entry.data.len()))
        .min(u32::MAX as usize) as u32;
    TRUEOS_RDP_TEXTURE_CACHE_BYTES.store(bytes, Ordering::Release);
    seq
}

fn texture_cache_snapshot() -> Vec<CachedTexture> {
    let mut textures = TRUEOS_RDP_TEXTURE_CACHE.lock().clone();
    textures.sort_by_key(|entry| entry.seq);
    textures
}

fn send_frame_to(
    cmds: &NetQueue<NetCommand>,
    handle: NetHandle,
    frame: Vec<u8>,
    label: &'static str,
) {
    if cmds
        .push(NetCommand::SendTcp {
            handle,
            data: frame,
        })
        .is_err()
    {
        crate::log!("trueos-rdp: {} queue full handle={}\n", label, handle.0);
    }
}

async fn send_resource_snapshot(cmds: &NetQueue<NetCommand>, handle: NetHandle) {
    let target_seq = crate::r::resource_monitor::latest_encoded_seq();
    let flushed = crate::r::resource_monitor::wait_until_flushed(target_seq, 1_500).await;
    if !flushed {
        crate::log!(
            "trueos-rdp: asset sync ramdisk wait timeout target_seq={}\n",
            target_seq
        );
    }

    let assets = crate::r::resource_monitor::encoded_assets_snapshot();
    let bytes = assets
        .iter()
        .fold(0usize, |acc, asset| acc.saturating_add(asset.bytes.len()))
        .min(u32::MAX as usize) as u32;
    let latest_seq = assets.last().map(|asset| asset.seq).unwrap_or(0);

    send_frame_to(
        cmds,
        handle,
        protocol_frame(
            MSG_RESOURCE_SNAPSHOT_BEGIN,
            &[
                assets.len().min(u32::MAX as usize) as u32,
                bytes,
                latest_seq.min(u32::MAX as u64) as u32,
            ],
            &[],
        ),
        "asset-sync-begin",
    );

    for asset in assets {
        send_frame_to(
            cmds,
            handle,
            protocol_frame(
                encoded_msg(asset.kind),
                encoded_fields(&asset).as_slice(),
                asset.bytes.as_slice(),
            ),
            "asset-sync-texture",
        );
    }

    send_frame_to(
        cmds,
        handle,
        protocol_frame(
            MSG_RESOURCE_SNAPSHOT_END,
            &[
                crate::r::resource_monitor::preserved_count(),
                crate::r::resource_monitor::preserved_bytes().min(u32::MAX as u64) as u32,
                latest_seq.min(u32::MAX as u64) as u32,
            ],
            &[],
        ),
        "asset-sync-end",
    );
}

pub fn publish_begin_frame(seq: u32, flags: u32, clear_rgb: u32) {
    publish_small(MSG_BEGIN_FRAME, &[seq, flags, clear_rgb & 0x00FF_FFFF]);
}

pub fn publish_end_frame(seq: u32, flags: u32, rgb_draws: u32, tex_draws: u32, draw_bytes: u32) {
    publish_small(MSG_END_FRAME, &[seq, flags, rgb_draws, tex_draws, draw_bytes]);
}

pub fn publish_set_blend(
    frame_seq: u32,
    enabled: u32,
    src_rgb: u32,
    dst_rgb: u32,
    src_alpha: u32,
    dst_alpha: u32,
) {
    publish_small(MSG_SET_BLEND, &[frame_seq, enabled, src_rgb, dst_rgb, src_alpha, dst_alpha]);
}

pub fn publish_set_sampler(
    frame_seq: u32,
    wrap_s: u32,
    wrap_t: u32,
    min_filter: u32,
    mag_filter: u32,
) {
    publish_small(MSG_SET_SAMPLER, &[frame_seq, wrap_s, wrap_t, min_filter, mag_filter]);
}

pub fn publish_set_scissor(frame_seq: u32, x: u32, y: u32, width: u32, height: u32) {
    publish_small(MSG_SET_SCISSOR, &[frame_seq, x, y, width, height]);
}

pub fn publish_clear_scissor(frame_seq: u32) {
    publish_small(MSG_CLEAR_SCISSOR, &[frame_seq]);
}

pub fn publish_set_render_target(frame_seq: u32, tex_id: u32) {
    publish_small(MSG_SET_RENDER_TARGET, &[frame_seq, tex_id]);
}

pub fn publish_clear_render_target(frame_seq: u32) {
    publish_small(MSG_CLEAR_RENDER_TARGET, &[frame_seq]);
}

pub fn publish_clear_rect(frame_seq: u32, rgb: u32, x: u32, y: u32, width: u32, height: u32) {
    publish_small(MSG_CLEAR_RECT, &[frame_seq, rgb & 0x00FF_FFFF, x, y, width, height]);
}

pub fn publish_texture_rgba(
    tex_id: u32,
    width: u32,
    height: u32,
    flags: u32,
    region: Option<(u32, u32, u32, u32)>,
    rgba: &[u8],
) {
    let (rx, ry, rw, rh) = region.unwrap_or((0, 0, 0, 0));
    let fields = [
        tex_id,
        width,
        height,
        flags,
        rx,
        ry,
        rw,
        rh,
        rgba.len().min(u32::MAX as usize) as u32,
    ];
    if let Some(asset) = crate::r::resource_monitor::encoded_texture(tex_id) {
        let encoded_fields = encoded_fields(&asset);
        let msg = encoded_msg(asset.kind);
        update_cached_texture(tex_id, msg, &encoded_fields, asset.bytes.as_slice());
        publish_bytes(msg, &encoded_fields, asset.bytes.as_slice());
        return;
    }
    update_cached_texture(tex_id, MSG_TEXTURE_RGBA, &fields, rgba);
    publish_bytes(MSG_TEXTURE_RGBA, &fields, rgba);
}

pub fn publish_texture_png(tex_id: u32, flags: u32, data: &[u8]) {
    let fields = [tex_id, flags, data.len().min(u32::MAX as usize) as u32];
    update_cached_texture(tex_id, MSG_TEXTURE_PNG, &fields, data);
    publish_bytes(MSG_TEXTURE_PNG, &fields, data);
}

pub fn publish_texture_jpeg(tex_id: u32, flags: u32, data: &[u8]) {
    let fields = [tex_id, flags, data.len().min(u32::MAX as usize) as u32];
    update_cached_texture(tex_id, MSG_TEXTURE_JPEG, &fields, data);
    publish_bytes(MSG_TEXTURE_JPEG, &fields, data);
}

pub fn publish_texture_svg(tex_id: u32, flags: u32, data: &[u8]) {
    let fields = [tex_id, flags, data.len().min(u32::MAX as usize) as u32];
    update_cached_texture(tex_id, MSG_TEXTURE_SVG, &fields, data);
    publish_bytes(MSG_TEXTURE_SVG, &fields, data);
}

pub fn publish_draw_rgb_triangles(frame_seq: u32, vcount: u32, vertices: &[u8]) {
    publish_bytes(
        MSG_DRAW_RGB_TRIANGLES,
        &[
            frame_seq,
            vcount,
            vertices.len().min(u32::MAX as usize) as u32,
        ],
        vertices,
    );
}

pub fn publish_draw_tex_triangles(
    frame_seq: u32,
    tex_id: u32,
    vcount: u32,
    sampler_flags: u32,
    sample_kind: u32,
    vertices: &[u8],
) {
    publish_bytes(
        MSG_DRAW_TEX_TRIANGLES,
        &[
            frame_seq,
            tex_id,
            vcount,
            sampler_flags,
            sample_kind,
            vertices.len().min(u32::MAX as usize) as u32,
        ],
        vertices,
    );
}

fn open_listener(cmds: &NetQueue<NetCommand>) {
    if cmds
        .push(NetCommand::OpenTcpListen {
            port: TRUEOS_RDP_PORT,
        })
        .is_err()
    {
        crate::log!("trueos-rdp: listen command queue full port={}\n", TRUEOS_RDP_PORT);
    }
}

async fn send_hello(cmds: &NetQueue<NetCommand>, handle: NetHandle) {
    send_frame_to(cmds, handle, hello_frame(), "hello");
    send_resource_snapshot(cmds, handle).await;
}

#[task]
pub async fn trueos_rdp_task() {
    if TRUEOS_RDP_STARTED.swap(true, Ordering::AcqRel) {
        return;
    }

    crate::r::readiness::wait_for(crate::r::readiness::NET_ANY_CONFIGURED).await;

    let cmds = NetQueue::new_leaked("trueos-rdp-cmd", 8192);
    let events = NetQueue::new_leaked("trueos-rdp-evt", 1024);
    register_app_queues(OWNER, cmds, events);
    *TRUEOS_RDP_COMMAND_QUEUE.lock() = Some(cmds);

    open_listener(cmds);
    crate::log!("trueos-rdp: listening tcp {} owner={}\n", TRUEOS_RDP_PORT, OWNER);

    let mut listener: Option<NetHandle> = None;
    let mut clients: Vec<NetHandle> = Vec::new();
    let mut input_buffers: Vec<ClientInputBuffer> = Vec::new();
    let mut ticks: u32 = 0;

    loop {
        for ev in events.drain(64) {
            match ev {
                NetEvent::Opened { handle, kind } if kind == SocketKind::Tcp => {
                    if listener.is_none() {
                        listener = Some(handle);
                        crate::log!("trueos-rdp: listener opened handle={}\n", handle.0);
                    }
                }
                NetEvent::TcpEstablished {
                    handle,
                    peer,
                    peer6,
                } => {
                    if listener == Some(handle) {
                        listener = None;
                        open_listener(cmds);
                    }

                    if !clients.contains(&handle) {
                        clients.push(handle);
                        publish_client_handles(clients.as_slice());
                    }

                    match peer {
                        Some(p) => crate::log!(
                            "trueos-rdp: client handle={} peer={}.{}.{}.{}:{} clients={}\n",
                            handle.0,
                            p.addr[0],
                            p.addr[1],
                            p.addr[2],
                            p.addr[3],
                            p.port,
                            clients.len()
                        ),
                        None => {
                            let port = peer6.map(|p| p.port).unwrap_or(0);
                            crate::log!(
                                "trueos-rdp: client handle={} peer6={} clients={}\n",
                                handle.0,
                                port,
                                clients.len()
                            );
                        }
                    }

                    send_hello(cmds, handle).await;
                }
                NetEvent::Closed { handle } => {
                    if listener == Some(handle) {
                        listener = None;
                        open_listener(cmds);
                        crate::log!("trueos-rdp: listener closed handle={} relisten=1\n", handle.0);
                    }

                    if remove_client_handle(&mut clients, &mut input_buffers, handle) {
                        crate::log!(
                            "trueos-rdp: client closed handle={} clients={}\n",
                            handle.0,
                            clients.len()
                        );
                    }
                }
                NetEvent::TcpData { handle, data } => {
                    handle_client_data(handle, data, &mut input_buffers);
                }
                NetEvent::TcpSent { .. } => {}
                NetEvent::Error { msg } => {
                    if msg == "bad handle" {
                        let cleared = clear_client_handles(&mut clients, &mut input_buffers);
                        if cleared != 0 {
                            crate::log!(
                                "trueos-rdp: clients cleared reason=bad-handle count={}\n",
                                cleared
                            );
                        }
                    } else if ticks.is_multiple_of(100) {
                        crate::log!("trueos-rdp: net error {}\n", msg);
                    }
                }
                NetEvent::Opened { .. }
                | NetEvent::UdpPacket { .. }
                | NetEvent::UdpPacketV6 { .. }
                | NetEvent::IcmpReply { .. }
                | NetEvent::IcmpReplyV6 { .. } => {}
            }
        }

        ticks = ticks.wrapping_add(1);
        Timer::after(EmbassyDuration::from_millis(10)).await;
    }
}
