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
const CAP_ONE_WAY_MONITOR: u32 = 1;
const CAP_GFX_COMMAND_STREAM: u32 = 1 << 1;
const CAP_RESOURCE_SNAPSHOT: u32 = 1 << 2;
const RDP_TEXTURE_CACHE_CAP: usize = 512;

#[derive(Clone)]
struct CachedTexture {
    tex_id: u32,
    msg: u16,
    fields: Vec<u32>,
    data: Vec<u8>,
    seq: u32,
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
    let caps = CAP_ONE_WAY_MONITOR | CAP_GFX_COMMAND_STREAM | CAP_RESOURCE_SNAPSHOT;

    let mut payload = begin_payload(MSG_HELLO, 16);
    push_u32(&mut payload, view_w);
    push_u32(&mut payload, view_h);
    push_u32(&mut payload, caps);

    frame_from_payload(payload)
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

fn send_resource_snapshot(cmds: &NetQueue<NetCommand>, handle: NetHandle) {
    let textures = texture_cache_snapshot();
    let bytes = textures
        .iter()
        .fold(0usize, |acc, entry| acc.saturating_add(entry.data.len()))
        .min(u32::MAX as usize) as u32;
    let latest_seq = textures.last().map(|entry| entry.seq).unwrap_or(0);

    send_frame_to(
        cmds,
        handle,
        protocol_frame(
            MSG_RESOURCE_SNAPSHOT_BEGIN,
            &[
                textures.len().min(u32::MAX as usize) as u32,
                bytes,
                latest_seq,
            ],
            &[],
        ),
        "snapshot-begin",
    );

    for texture in textures {
        send_frame_to(
            cmds,
            handle,
            protocol_frame(texture.msg, texture.fields.as_slice(), texture.data.as_slice()),
            "snapshot-texture",
        );
    }

    send_frame_to(
        cmds,
        handle,
        protocol_frame(
            MSG_RESOURCE_SNAPSHOT_END,
            &[
                cached_texture_count(),
                cached_texture_bytes(),
                TRUEOS_RDP_RESOURCE_SEQ.load(Ordering::Acquire),
            ],
            &[],
        ),
        "snapshot-end",
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

fn send_hello(cmds: &NetQueue<NetCommand>, handle: NetHandle) {
    send_frame_to(cmds, handle, hello_frame(), "hello");
    send_resource_snapshot(cmds, handle);
}

#[task]
pub async fn trueos_rdp_task() {
    if TRUEOS_RDP_STARTED.swap(true, Ordering::AcqRel) {
        return;
    }

    crate::r::readiness::wait_for(crate::r::readiness::NET_ANY_CONFIGURED).await;

    let cmds = NetQueue::new_leaked("trueos-rdp-cmd", 512);
    let events = NetQueue::new_leaked("trueos-rdp-evt", 512);
    register_app_queues(OWNER, cmds, events);
    *TRUEOS_RDP_COMMAND_QUEUE.lock() = Some(cmds);

    open_listener(cmds);
    crate::log!("trueos-rdp: listening tcp {} owner={}\n", TRUEOS_RDP_PORT, OWNER);

    let mut listener: Option<NetHandle> = None;
    let mut clients: Vec<NetHandle> = Vec::new();
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
                        TRUEOS_RDP_CLIENTS.store(clients.len() as u32, Ordering::Release);
                        *TRUEOS_RDP_CLIENT_HANDLES.lock() = clients.clone();
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

                    send_hello(cmds, handle);
                }
                NetEvent::Closed { handle } => {
                    if listener == Some(handle) {
                        listener = None;
                        open_listener(cmds);
                        crate::log!("trueos-rdp: listener closed handle={} relisten=1\n", handle.0);
                    }

                    let before = clients.len();
                    clients.retain(|client| *client != handle);
                    if clients.len() != before {
                        TRUEOS_RDP_CLIENTS.store(clients.len() as u32, Ordering::Release);
                        *TRUEOS_RDP_CLIENT_HANDLES.lock() = clients.clone();
                        crate::log!(
                            "trueos-rdp: client closed handle={} clients={}\n",
                            handle.0,
                            clients.len()
                        );
                    }
                }
                NetEvent::TcpSent { .. } | NetEvent::TcpData { .. } => {}
                NetEvent::Error { msg } => {
                    if ticks.is_multiple_of(100) {
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
