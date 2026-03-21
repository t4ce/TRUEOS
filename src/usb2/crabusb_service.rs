use alloc::vec::Vec;
use core::alloc::Layout;
use core::cmp::min;
use core::future::Future;
use core::f32::consts::TAU;
use core::num::NonZeroUsize;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicBool, Ordering};
use core::task::Poll;
use core::time::Duration;

use crab_usb::{EndpointKind, Event, EventHandler, KernelOp, USBHost, usb_if};
use dma_api::{DmaAddr, DmaDirection, DmaError, DmaHandle, DmaMapHandle, DmaOp};
use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};
use libm::sinf;
use spin::Mutex;

use super::api::{InterfaceEndpointError, claim_interface};

pub(super) struct TrueosCrabUsbKernel;

pub(super) static CRABUSB_KERNEL: TrueosCrabUsbKernel = TrueosCrabUsbKernel;
static INITIAL_SNAPSHOT_LOGGED: AtomicBool = AtomicBool::new(false);
static EVENT_HANDLER_READY: AtomicBool = AtomicBool::new(false);
static EVENT_HANDLER: Mutex<Option<EventHandler>> = Mutex::new(None);
static PROBE_REQUESTED: AtomicBool = AtomicBool::new(false);
static ROOT_PORT_CHANGE_SEEN: AtomicBool = AtomicBool::new(false);
static NO_PORT_CHANGE_HINT_LOGGED: AtomicBool = AtomicBool::new(false);
static AUDIO_STREAM_REQUESTED: AtomicBool = AtomicBool::new(false);
static AUDIO_STREAM_ACTIVE: AtomicBool = AtomicBool::new(false);
static TRUEKEY_STREAM_REQUESTED: AtomicBool = AtomicBool::new(false);
static TRUEKEY_STREAM_ACTIVE: AtomicBool = AtomicBool::new(false);
static HID_STREAMS_ACTIVE: Mutex<Vec<ActiveHidStream>> = Mutex::new(Vec::new());

const DEMO_WAV_EMBEDDED: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/demo.wav"));
const AUDIO_FRAME_BYTES: usize = 4; // s16le stereo
const TRUEKEY_VENDOR_ID: u16 = 0x303A;
const TRUEKEY_PRODUCT_ID: u16 = 0x1001;
const TRUEKEY_STREAM_CHUNK: usize = 512;
const HID_INTERRUPT_TIMEOUT_MS: u64 = 1000;

impl DmaOp for TrueosCrabUsbKernel {
    fn page_size(&self) -> usize {
        4096
    }

    unsafe fn map_single(
        &self,
        dma_mask: u64,
        addr: NonNull<u8>,
        size: NonZeroUsize,
        align: usize,
        _direction: DmaDirection,
    ) -> Result<DmaMapHandle, DmaError> {
        let required_align = align.max(1);
        let layout =
            Layout::from_size_align(size.get(), required_align).map_err(DmaError::LayoutError)?;
        let phys = crate::phys::virt_to_phys_checked(addr.as_ptr()).ok_or(DmaError::NoMemory)?;

        let aligned = phys.is_multiple_of(required_align as u64);
        let in_mask = phys
            .checked_add(size.get().saturating_sub(1) as u64)
            .map(|end| end <= dma_mask)
            .unwrap_or(false);

        if aligned && in_mask {
            return Ok(unsafe { DmaMapHandle::new(addr, DmaAddr::from(phys), layout, None) });
        }

        let max_phys_exclusive = if dma_mask == u64::MAX {
            None
        } else {
            dma_mask.checked_add(1)
        };
        let (bounce_phys, bounce_virt) =
            crate::dma::alloc_with_max(layout.size(), layout.align(), max_phys_exclusive)
                .ok_or(DmaError::NoMemory)?;
        let bounce_virt = NonNull::new(bounce_virt).ok_or(DmaError::NoMemory)?;

        Ok(unsafe {
            DmaMapHandle::new(addr, DmaAddr::from(bounce_phys), layout, Some(bounce_virt))
        })
    }

    unsafe fn unmap_single(&self, handle: DmaMapHandle) {
        if let Some(alloc_virt) = handle.alloc_virt() {
            crate::dma::dealloc(alloc_virt.as_ptr(), handle.size());
        }
    }

    unsafe fn alloc_coherent(&self, dma_mask: u64, layout: Layout) -> Option<DmaHandle> {
        let max_phys_exclusive = if dma_mask == u64::MAX {
            None
        } else {
            dma_mask.checked_add(1)
        };
        let (phys, virt) =
            crate::dma::alloc_with_max(layout.size(), layout.align(), max_phys_exclusive)?;
        let virt = NonNull::new(virt)?;
        Some(unsafe { DmaHandle::new(virt, DmaAddr::from(phys), layout) })
    }

    unsafe fn dealloc_coherent(&self, handle: DmaHandle) {
        crate::dma::dealloc(handle.as_ptr().as_ptr(), handle.size());
    }
}

impl KernelOp for TrueosCrabUsbKernel {
    fn delay(&self, duration: Duration) {
        let millis = duration.as_millis();
        if millis == 0 {
            return;
        }
        let timeout_ms = millis.min(u128::from(u64::MAX)) as u64;
        let _ = crate::wait::spin_until_timeout(timeout_ms, || false);
    }
}

#[derive(Copy, Clone)]
struct PreferredAlt {
    interface_number: u8,
    alternate_setting: u8,
    class: u8,
    subclass: u8,
    protocol: u8,
    has_iso_out: bool,
    endpoint_count: usize,
}

#[derive(Copy, Clone)]
struct IsoOutEndpoint {
    address: u8,
    max_packet_size: u16,
}

#[derive(Copy, Clone)]
struct TruekeyTarget {
    interface_number: u8,
    alternate_setting: u8,
    out_endpoint: u8,
    out_max_packet_size: u16,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum HidBootKind {
    Keyboard,
    Mouse,
    Tablet,
}

impl HidBootKind {
    #[inline]
    fn as_str(self) -> &'static str {
        match self {
            HidBootKind::Keyboard => "keyboard",
            HidBootKind::Mouse => "mouse",
            HidBootKind::Tablet => "tablet",
        }
    }

    #[inline]
    fn protocol(self) -> u8 {
        match self {
            HidBootKind::Keyboard => 0x01,
            HidBootKind::Mouse => 0x02,
            HidBootKind::Tablet => 0x00,
        }
    }
}

#[derive(Copy, Clone, Debug)]
struct HidBootTarget {
    configuration_value: u8,
    interface_number: u8,
    alternate_setting: u8,
    protocol: u8,
    in_endpoint: u8,
    in_max_packet_size: u16,
    report_len: usize,
    kind: HidBootKind,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct ActiveHidStream {
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    kind: HidBootKind,
}

#[inline]
fn endpoint_target_from_address(address: u8) -> u32 {
    let ep_num = u32::from(address & 0x0F);
    if ep_num == 0 {
        1
    } else if (address & 0x80) != 0 {
        (ep_num * 2) + 1
    } else {
        ep_num * 2
    }
}

fn pick_hid_boot_targets(
    configs: &[usb_if::descriptor::ConfigurationDescriptor],
) -> Vec<HidBootTarget> {
    let mut out = Vec::new();

    for config in configs.iter() {
        for interface in config.interfaces.iter() {
            for alt in interface.alt_settings.iter() {
                let kind = match (alt.class, alt.subclass, alt.protocol) {
                    (0x03, 0x01, 0x01) => HidBootKind::Keyboard,
                    (0x03, 0x01, 0x02) => HidBootKind::Mouse,
                    _ if super::hid::tablet::matches_interface(
                        alt.class,
                        alt.subclass,
                        alt.protocol,
                    ) =>
                    {
                        HidBootKind::Tablet
                    }
                    _ => continue,
                };

                let Some(endpoint) = alt.endpoints.iter().find(|ep| {
                    ep.transfer_type == usb_if::descriptor::EndpointType::Interrupt
                        && ep.direction == usb_if::transfer::Direction::In
                }) else {
                    continue;
                };

                let report_len = match kind {
                    HidBootKind::Keyboard => 8,
                    HidBootKind::Mouse => 4,
                    HidBootKind::Tablet => {
                        super::hid::tablet::report_len(endpoint.max_packet_size)
                    }
                };
                out.push(HidBootTarget {
                    configuration_value: config.configuration_value,
                    interface_number: alt.interface_number,
                    alternate_setting: alt.alternate_setting,
                    protocol: alt.protocol,
                    in_endpoint: endpoint.address,
                    in_max_packet_size: endpoint.max_packet_size,
                    report_len,
                    kind,
                });
            }
        }
    }

    out
}

fn descriptor_has_audio_candidate(dev_info: &crab_usb::DeviceInfo) -> bool {
    dev_info.interface_descriptors().any(|iface| {
        iface.class == 0x01
            || iface.endpoints.iter().any(|ep| {
                ep.transfer_type == usb_if::descriptor::EndpointType::Isochronous
                    && ep.direction == usb_if::transfer::Direction::Out
            })
    })
}

fn register_active_hid_stream(stream: ActiveHidStream) -> bool {
    let mut streams = HID_STREAMS_ACTIVE.lock();
    if streams.iter().any(|active| *active == stream) {
        return false;
    }
    streams.push(stream);
    true
}

fn unregister_active_hid_stream(stream: ActiveHidStream) -> bool {
    let mut streams = HID_STREAMS_ACTIVE.lock();
    if let Some(idx) = streams.iter().position(|active| *active == stream) {
        streams.remove(idx);
    }
    !streams.iter().any(|active| {
        active.controller_id == stream.controller_id && active.slot_id == stream.slot_id
    })
}

async fn with_timeout_or_none<F: Future>(fut: F, timeout_ms: u64) -> Option<F::Output> {
    let mut fut = core::pin::pin!(fut);
    let mut timeout = core::pin::pin!(Timer::after(EmbassyDuration::from_millis(timeout_ms)));

    core::future::poll_fn(|cx| {
        if let Poll::Ready(out) = fut.as_mut().poll(cx) {
            return Poll::Ready(Some(out));
        }
        if timeout.as_mut().poll(cx).is_ready() {
            return Poll::Ready(None);
        }
        Poll::Pending
    })
    .await
}

#[embassy_executor::task(pool_size = 8)]
async fn hid_boot_stream_task(
    mut device: crab_usb::Device,
    controller_id: u32,
    target: HidBootTarget,
) {
    let desc = device.descriptor();
    let vendor_id = desc.vendor_id;
    let product_id = desc.product_id;
    let slot_id = u32::from(device.slot_id());
    let ep_target = endpoint_target_from_address(target.in_endpoint);
    let active_stream = ActiveHidStream {
        controller_id,
        slot_id,
        ep_target,
        kind: target.kind,
    };
    let mut boot_protocol_ok = false;
    let mut set_idle_ok = false;

    if let Err(err) = device
        .ep_ctrl()
        .set_configuration(target.configuration_value)
        .await
    {
        crate::log!(
            "crabusb: hid {} {:04X}:{:04X} set cfg={} failed: {:?}\n",
            target.kind.as_str(),
            vendor_id,
            product_id,
            target.configuration_value,
            err
        );
    }

    let mut interface = match claim_interface(
        &mut device,
        target.interface_number,
        target.alternate_setting,
    )
    .await
    {
        Ok(interface) => interface,
        Err(err) => {
            crate::log!(
                "crabusb: hid {} {:04X}:{:04X} claim failed if#{} alt={}: {:?}\n",
                target.kind.as_str(),
                vendor_id,
                product_id,
                target.interface_number,
                target.alternate_setting,
                err
            );
            let _ = unregister_active_hid_stream(active_stream);
            return;
        }
    };

    if matches!(target.kind, HidBootKind::Mouse | HidBootKind::Keyboard) {
        match interface
            .device()
            .control_out(
                usb_if::host::ControlSetup {
                    request_type: usb_if::transfer::RequestType::Class,
                    recipient: usb_if::transfer::Recipient::Interface,
                    request: usb_if::transfer::Request::Other(0x0B),
                    value: 0,
                    index: u16::from(target.interface_number),
                },
                &[],
            )
            .await
        {
            Ok(_) => {
                boot_protocol_ok = true;
            }
            Err(err) => {
                crate::log!(
                    "crabusb: hid {} {:04X}:{:04X} boot protocol if#{} failed: {:?}\n",
                    target.kind.as_str(),
                    vendor_id,
                    product_id,
                    target.interface_number,
                    err
                );
            }
        }

        match interface
            .device()
            .control_out(
                usb_if::host::ControlSetup {
                    request_type: usb_if::transfer::RequestType::Class,
                    recipient: usb_if::transfer::Recipient::Interface,
                    request: usb_if::transfer::Request::Other(0x0A),
                    value: 1 << 8,
                    index: u16::from(target.interface_number),
                },
                &[],
            )
            .await
        {
            Ok(_) => {
                set_idle_ok = true;
            }
            Err(err) => {
                crate::log!(
                    "crabusb: hid {} {:04X}:{:04X} set idle if#{} duration=1 failed: {:?}\n",
                    target.kind.as_str(),
                    vendor_id,
                    product_id,
                    target.interface_number,
                    err
                );
            }
        }
    }

    if matches!(target.kind, HidBootKind::Tablet) {
        match interface
            .device()
            .control_out(
                usb_if::host::ControlSetup {
                    request_type: usb_if::transfer::RequestType::Class,
                    recipient: usb_if::transfer::Recipient::Interface,
                    request: usb_if::transfer::Request::Other(0x0A),
                    value: 1 << 8,
                    index: u16::from(target.interface_number),
                },
                &[],
            )
            .await
        {
            Ok(_) => {
                set_idle_ok = true;
            }
            Err(err) => {
                crate::log!(
                    "crabusb: hid {} {:04X}:{:04X} set idle if#{} duration=1 failed: {:?}\n",
                    target.kind.as_str(),
                    vendor_id,
                    product_id,
                    target.interface_number,
                    err
                );
            }
        }
    }

    let mut interrupt_in = match interface.endpoint_interrupt_in(target.in_endpoint).await {
        Ok(endpoint) => endpoint,
        Err(InterfaceEndpointError::WrongKind { .. }) => {
            crate::log!(
                "crabusb: hid {} {:04X}:{:04X} interrupt endpoint kind mismatch ep=0x{:02X}\n",
                target.kind.as_str(),
                vendor_id,
                product_id,
                target.in_endpoint
            );
            let _ = unregister_active_hid_stream(active_stream);
            return;
        }
        Err(InterfaceEndpointError::Usb(err)) => {
            crate::log!(
                "crabusb: hid {} {:04X}:{:04X} interrupt open failed ep=0x{:02X}: {:?}\n",
                target.kind.as_str(),
                vendor_id,
                product_id,
                target.in_endpoint,
                err
            );
            let _ = unregister_active_hid_stream(active_stream);
            return;
        }
    };

    crate::log!(
        "crabusb: hid {} {:04X}:{:04X} ready slot={} if#{} alt={} cfg={} int_in=0x{:02X} mps={} ep_target={} proto={:02X} boot={} idle={}\n",
        target.kind.as_str(),
        vendor_id,
        product_id,
        slot_id,
        target.interface_number,
        target.alternate_setting,
        target.configuration_value,
        target.in_endpoint,
        target.in_max_packet_size,
        ep_target,
        target.protocol,
        boot_protocol_ok,
        set_idle_ok
    );

    let mut report = Vec::from_iter(core::iter::repeat_n(
        0u8,
        usize::from(target.in_max_packet_size.max(target.report_len as u16)),
    ));
    let mut timeout_logs = 0u32;

    loop {
        match with_timeout_or_none(
            interrupt_in.submit_and_wait(report.as_mut_slice()),
            HID_INTERRUPT_TIMEOUT_MS,
        )
        .await
        {
            None => {
                timeout_logs = timeout_logs.wrapping_add(1);
                if timeout_logs <= 8 || timeout_logs.is_multiple_of(32) {
                    crate::log!(
                        "crabusb: hid {} {:04X}:{:04X} interrupt timeout ep=0x{:02X} count={}\n",
                        target.kind.as_str(),
                        vendor_id,
                        product_id,
                        target.in_endpoint,
                        timeout_logs
                    );
                }
                continue;
            }
            Some(result) => match result {
            Ok(read) => {
                timeout_logs = 0;
                if read == 0 {
                    Timer::after(EmbassyDuration::from_millis(1)).await;
                    continue;
                }

                let sample = &report[..read.min(report.len())];
                match target.kind {
                    HidBootKind::Keyboard => super::handle_keyboard_boot_report(
                        controller_id,
                        slot_id,
                        ep_target,
                        sample,
                    ),
                    HidBootKind::Mouse => {
                        super::handle_mouse_boot_report(controller_id, slot_id, ep_target, sample)
                    }
                    HidBootKind::Tablet => {
                        super::hid::tablet::handle_packet(
                            vendor_id,
                            product_id,
                            target.in_endpoint,
                            sample,
                        );
                    }
                }
            }
            Err(err) => {
                crate::log!(
                    "crabusb: hid {} {:04X}:{:04X} stream stop ep=0x{:02X} err={:?}\n",
                    target.kind.as_str(),
                    vendor_id,
                    product_id,
                    target.in_endpoint,
                    err
                );
                break;
            }
        },
        }
    }

    if unregister_active_hid_stream(active_stream) {
        super::remove_hid_slot(controller_id, slot_id);
    }
}

async fn maybe_start_hid_boot_streams(
    host: &mut USBHost,
    dev_info: &crab_usb::DeviceInfo,
    spawner: &Spawner,
    controller_id: u32,
) -> bool {
    let desc = dev_info.descriptor();
    let vendor_id = desc.vendor_id;
    let product_id = desc.product_id;
    let targets = pick_hid_boot_targets(dev_info.configurations());
    if targets.is_empty() {
        return false;
    }

    let mut started_any = false;

    for target in targets {
        let device = match host.open_device(dev_info).await {
            Ok(device) => device,
            Err(err) => {
                crate::log!(
                    "crabusb: hid {} {:04X}:{:04X} open failed: {:?}\n",
                    target.kind.as_str(),
                    vendor_id,
                    product_id,
                    err
                );
                break;
            }
        };

        let slot_id = u32::from(device.slot_id());
        let ep_target = endpoint_target_from_address(target.in_endpoint);
        let active_stream = ActiveHidStream {
            controller_id,
            slot_id,
            ep_target,
            kind: target.kind,
        };
        if !register_active_hid_stream(active_stream) {
            continue;
        }

        match spawner.spawn(hid_boot_stream_task(device, controller_id, target)) {
            Ok(()) => {
                started_any = true;
                crate::log!(
                    "crabusb: hid {} {:04X}:{:04X} handoff if#{} alt={} cfg={} int_in=0x{:02X} mps={} proto={:02X}\n",
                    target.kind.as_str(),
                    vendor_id,
                    product_id,
                    target.interface_number,
                    target.alternate_setting,
                    target.configuration_value,
                    target.in_endpoint,
                    target.in_max_packet_size,
                    target.protocol
                );
            }
            Err(err) => {
                let _ = unregister_active_hid_stream(active_stream);
                crate::log!(
                    "crabusb: hid {} {:04X}:{:04X} spawn failed if#{} alt={} ep=0x{:02X}: {:?}\n",
                    target.kind.as_str(),
                    vendor_id,
                    product_id,
                    target.interface_number,
                    target.alternate_setting,
                    target.in_endpoint,
                    err
                );
            }
        }
    }

    started_any
}

fn parse_wav_pcm_s16_stereo_48k(bytes: &[u8]) -> Option<(usize, usize)> {
    fn le_u16(s: &[u8]) -> Option<u16> {
        if s.len() < 2 {
            return None;
        }
        Some(u16::from_le_bytes([s[0], s[1]]))
    }

    fn le_u32(s: &[u8]) -> Option<u32> {
        if s.len() < 4 {
            return None;
        }
        Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }

    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }

    let mut off = 12usize;
    let mut fmt_ok = false;
    let mut data: Option<(usize, usize)> = None;
    while off + 8 <= bytes.len() {
        let id = &bytes[off..off + 4];
        let sz = le_u32(&bytes[off + 4..off + 8])? as usize;
        let payload = off + 8;
        let end = payload.saturating_add(sz);
        if end > bytes.len() {
            return None;
        }

        if id == b"fmt " {
            if sz < 16 {
                return None;
            }
            let fmt = &bytes[payload..payload + sz];
            let audio_fmt = le_u16(&fmt[0..2])?;
            let channels = le_u16(&fmt[2..4])?;
            let rate = le_u32(&fmt[4..8])?;
            let bits = le_u16(&fmt[14..16])?;
            if audio_fmt == 1 && channels == 2 && rate == 48_000 && bits == 16 {
                fmt_ok = true;
            } else {
                return None;
            }
        } else if id == b"data" {
            data = Some((payload, sz));
            if fmt_ok {
                break;
            }
        }

        off = end + (sz & 1);
    }

    if !fmt_ok {
        return None;
    }
    data
}

fn pick_preferred_alt(
    configs: &[usb_if::descriptor::ConfigurationDescriptor],
) -> Option<PreferredAlt> {
    let mut best: Option<PreferredAlt> = None;

    for config in configs.iter() {
        for interface in config.interfaces.iter() {
            for alt in interface.alt_settings.iter() {
                let has_iso_out = alt.endpoints.iter().any(|ep| {
                    ep.transfer_type == usb_if::descriptor::EndpointType::Isochronous
                        && ep.direction == usb_if::transfer::Direction::Out
                });

                let score = if has_iso_out {
                    100
                } else if alt.class == 0x01 && alt.subclass == 0x02 {
                    50
                } else if alt.class == 0x01 {
                    25
                } else {
                    0
                };

                let candidate = PreferredAlt {
                    interface_number: alt.interface_number,
                    alternate_setting: alt.alternate_setting,
                    class: alt.class,
                    subclass: alt.subclass,
                    protocol: alt.protocol,
                    has_iso_out,
                    endpoint_count: alt.endpoints.len(),
                };

                let replace = match best {
                    None => true,
                    Some(current) => {
                        let current_score = if current.has_iso_out {
                            100
                        } else if current.class == 0x01 && current.subclass == 0x02 {
                            50
                        } else if current.class == 0x01 {
                            25
                        } else {
                            0
                        };

                        score > current_score
                            || (score == current_score
                                && candidate.endpoint_count > current.endpoint_count)
                            || (score == current_score
                                && candidate.endpoint_count == current.endpoint_count
                                && candidate.alternate_setting > current.alternate_setting)
                    }
                };

                if replace {
                    best = Some(candidate);
                }
            }
        }
    }

    best
}

fn find_iso_out_endpoint(
    configs: &[usb_if::descriptor::ConfigurationDescriptor],
    interface_number: u8,
    alternate_setting: u8,
) -> Option<IsoOutEndpoint> {
    for config in configs.iter() {
        for interface in config.interfaces.iter() {
            if interface.interface_number != interface_number {
                continue;
            }
            for alt in interface.alt_settings.iter() {
                if alt.alternate_setting != alternate_setting {
                    continue;
                }
                for ep in alt.endpoints.iter() {
                    if ep.transfer_type == usb_if::descriptor::EndpointType::Isochronous
                        && ep.direction == usb_if::transfer::Direction::Out
                    {
                        return Some(IsoOutEndpoint {
                            address: ep.address,
                            max_packet_size: ep.max_packet_size,
                        });
                    }
                }
            }
        }
    }
    None
}

fn fill_audio_packet(
    out: &mut [u8],
    wav: Option<&[u8]>,
    wav_cursor: &mut usize,
    sine_phase: &mut f32,
) {
    if let Some(wav_bytes) = wav {
        let mut copied = 0usize;
        while copied + AUDIO_FRAME_BYTES <= out.len() {
            if *wav_cursor + AUDIO_FRAME_BYTES > wav_bytes.len() {
                *wav_cursor = 0;
            }
            out[copied..copied + AUDIO_FRAME_BYTES]
                .copy_from_slice(&wav_bytes[*wav_cursor..*wav_cursor + AUDIO_FRAME_BYTES]);
            *wav_cursor += AUDIO_FRAME_BYTES;
            copied += AUDIO_FRAME_BYTES;
        }
        out[copied..].fill(0);
        return;
    }

    for frame in out.chunks_exact_mut(AUDIO_FRAME_BYTES) {
        let sample = (sinf(*sine_phase) * 0.18 * i16::MAX as f32) as i16;
        let bytes = sample.to_le_bytes();
        frame[0] = bytes[0];
        frame[1] = bytes[1];
        frame[2] = bytes[0];
        frame[3] = bytes[1];
        *sine_phase += TAU * 440.0 / 48_000.0;
        if *sine_phase >= TAU {
            *sine_phase -= TAU;
        }
    }
}

fn pick_truekey_target(
    configs: &[usb_if::descriptor::ConfigurationDescriptor],
) -> Option<TruekeyTarget> {
    let mut best: Option<(u32, TruekeyTarget)> = None;

    for config in configs.iter() {
        for interface in config.interfaces.iter() {
            for alt in interface.alt_settings.iter() {
                let Some(out_ep) = alt.endpoints.iter().find(|ep| {
                    ep.direction == usb_if::transfer::Direction::Out
                        && ep.transfer_type == usb_if::descriptor::EndpointType::Bulk
                }) else {
                    continue;
                };

                let mut score = 10u32;
                if alt.class == 0x0A {
                    score += 100;
                } else if alt.class == 0x02 {
                    score += 70;
                } else if alt.class == 0xFF {
                    score += 40;
                }
                score += alt.endpoints.len() as u32;
                score += u32::from(alt.alternate_setting);

                let target = TruekeyTarget {
                    interface_number: alt.interface_number,
                    alternate_setting: alt.alternate_setting,
                    out_endpoint: out_ep.address,
                    out_max_packet_size: out_ep.max_packet_size,
                };

                match best {
                    Some((best_score, _)) if best_score >= score => {}
                    _ => best = Some((score, target)),
                }
            }
        }
    }

    best.map(|(_, target)| target)
}

async fn stream_truekey_logs(
    device: &mut crab_usb::Device,
    vendor_id: u16,
    product_id: u16,
    target: TruekeyTarget,
) {
    let endpoint_kind = match device.get_endpoint(target.out_endpoint).await {
        Ok(kind) => kind,
        Err(err) => {
            crate::log!(
                "crabusb: truekey {:04X}:{:04X} ep=0x{:02X} open failed: {:?}\n",
                vendor_id,
                product_id,
                target.out_endpoint,
                err
            );
            return;
        }
    };

    let crab_usb::EndpointKind::BulkOut(mut bulk_out) = endpoint_kind else {
        crate::log!(
            "crabusb: truekey {:04X}:{:04X} ep=0x{:02X} is not bulk-out\n",
            vendor_id,
            product_id,
            target.out_endpoint
        );
        return;
    };

    TRUEKEY_STREAM_ACTIVE.store(true, Ordering::Release);
    crate::log!(
        "crabusb: truekey streaming start {:04X}:{:04X} if#{} alt={} ep=0x{:02X} mps={}\n",
        vendor_id,
        product_id,
        target.interface_number,
        target.alternate_setting,
        target.out_endpoint,
        target.out_max_packet_size
    );

    let chunk_limit = min(
        TRUEKEY_STREAM_CHUNK,
        usize::from(target.out_max_packet_size.max(1)),
    );
    let mut cursor = 0usize;

    loop {
        let snapshot = crate::globalog::snapshot();
        if cursor > snapshot.len() {
            cursor = snapshot.len();
        }

        if cursor == snapshot.len() {
            Timer::after(EmbassyDuration::from_millis(50)).await;
            continue;
        }

        let end = min(snapshot.len(), cursor + chunk_limit);
        match bulk_out.submit_and_wait(&snapshot[cursor..end]).await {
            Ok(sent) if sent > 0 => {
                cursor += sent;
            }
            Ok(_) => {
                Timer::after(EmbassyDuration::from_millis(1)).await;
            }
            Err(err) => {
                crate::log!(
                    "crabusb: truekey streaming stopped {:04X}:{:04X} ep=0x{:02X} err={:?}\n",
                    vendor_id,
                    product_id,
                    target.out_endpoint,
                    err
                );
                break;
            }
        }
    }

    TRUEKEY_STREAM_ACTIVE.store(false, Ordering::Release);
}

async fn truekey_logdrain_task(
    device: &mut crab_usb::Device,
    vendor_id: u16,
    product_id: u16,
    target: TruekeyTarget,
) {
    crate::log!(
        "crabusb: truekey {:04X}:{:04X} handoff -> logdrain if#{} alt={} ep=0x{:02X}\n",
        vendor_id,
        product_id,
        target.interface_number,
        target.alternate_setting,
        target.out_endpoint
    );
    stream_truekey_logs(device, vendor_id, product_id, target).await;
}

async fn maybe_start_truekey_bridge(host: &mut USBHost, dev_info: &crab_usb::DeviceInfo) {
    if !TRUEKEY_STREAM_REQUESTED.load(Ordering::Acquire)
        || TRUEKEY_STREAM_ACTIVE.load(Ordering::Acquire)
    {
        return;
    }

    let desc = dev_info.descriptor();
    let vendor_id = desc.vendor_id;
    let product_id = desc.product_id;
    if vendor_id != TRUEKEY_VENDOR_ID || product_id != TRUEKEY_PRODUCT_ID {
        return;
    }

    crate::log!(
        "crabusb: truekey {:04X}:{:04X} candidate found\n",
        vendor_id,
        product_id
    );

    let mut device = match host.open_device(dev_info).await {
        Ok(device) => device,
        Err(err) => {
            crate::log!(
                "crabusb: truekey {:04X}:{:04X} open failed: {:?}\n",
                vendor_id,
                product_id,
                err
            );
            return;
        }
    };

    let configs = device.configurations().to_vec();
    crate::log!(
        "crabusb: truekey {:04X}:{:04X} inspecting {} config(s)\n",
        vendor_id,
        product_id,
        configs.len()
    );
    let Some(target) = pick_truekey_target(&configs) else {
        crate::log!(
            "crabusb: truekey {:04X}:{:04X} no bulk-out sink target found\n",
            vendor_id,
            product_id
        );
        return;
    };

    crate::log!(
        "crabusb: truekey {:04X}:{:04X} selected if#{} alt={} ep=0x{:02X}\n",
        vendor_id,
        product_id,
        target.interface_number,
        target.alternate_setting,
        target.out_endpoint
    );

    crate::log!(
        "crabusb: truekey {:04X}:{:04X} data claim begin if#{} alt={}\n",
        vendor_id,
        product_id,
        target.interface_number,
        target.alternate_setting
    );
    match device
        .claim_interface(target.interface_number, target.alternate_setting)
        .await
    {
        Ok(()) => {
            crate::log!(
                "crabusb: truekey {:04X}:{:04X} ownership if#{} alt={} ep=0x{:02X}\n",
                vendor_id,
                product_id,
                target.interface_number,
                target.alternate_setting,
                target.out_endpoint
            );
            truekey_logdrain_task(&mut device, vendor_id, product_id, target).await;
        }
        Err(err) => crate::log!(
            "crabusb: truekey {:04X}:{:04X} data if#{} alt={} claim failed: {:?}\n",
            vendor_id,
            product_id,
            target.interface_number,
            target.alternate_setting,
            err
        ),
    }
}

async fn stream_target_audio(
    device: &mut crab_usb::Device,
    vendor_id: u16,
    product_id: u16,
    preferred: PreferredAlt,
    endpoint: IsoOutEndpoint,
) {
    let packet_bytes = usize::from(endpoint.max_packet_size.max(AUDIO_FRAME_BYTES as u16));
    let mut packet = Vec::from_iter(core::iter::repeat_n(0u8, packet_bytes));
    let wav = parse_wav_pcm_s16_stereo_48k(DEMO_WAV_EMBEDDED)
        .map(|(data_off, data_len)| &DEMO_WAV_EMBEDDED[data_off..data_off + data_len]);
    let mut wav_cursor = 0usize;
    let mut sine_phase = 0.0f32;

    let endpoint_kind = match device.get_endpoint(endpoint.address).await {
        Ok(kind) => kind,
        Err(err) => {
            crate::log!(
                "crabusb: target {:04X}:{:04X} ep=0x{:02X} open failed: {:?}\n",
                vendor_id,
                product_id,
                endpoint.address,
                err
            );
            return;
        }
    };

    let EndpointKind::IsochronousOut(mut iso_out) = endpoint_kind else {
        crate::log!(
            "crabusb: target {:04X}:{:04X} if#{} alt={} ep=0x{:02X} is not iso-out\n",
            vendor_id,
            product_id,
            preferred.interface_number,
            preferred.alternate_setting,
            endpoint.address
        );
        return;
    };

    AUDIO_STREAM_ACTIVE.store(true, Ordering::Release);
    crate::log!(
        "crabusb: audio streaming start {:04X}:{:04X} if#{} alt={} ep=0x{:02X} packet={} source={}\n",
        vendor_id,
        product_id,
        preferred.interface_number,
        preferred.alternate_setting,
        endpoint.address,
        packet_bytes,
        if wav.is_some() { "demo.wav" } else { "sine" }
    );

    loop {
        fill_audio_packet(packet.as_mut_slice(), wav, &mut wav_cursor, &mut sine_phase);
        match iso_out.submit_and_wait(packet.as_slice(), 1).await {
            Ok(sent) => {
                if sent == 0 {
                    Timer::after(EmbassyDuration::from_millis(1)).await;
                }
            }
            Err(err) => {
                crate::log!(
                    "crabusb: audio streaming stopped {:04X}:{:04X} ep=0x{:02X} err={:?}\n",
                    vendor_id,
                    product_id,
                    endpoint.address,
                    err
                );
                break;
            }
        }
    }

    AUDIO_STREAM_ACTIVE.store(false, Ordering::Release);
}

async fn maybe_start_target_audio(host: &mut USBHost, dev_info: &crab_usb::DeviceInfo) {
    if !AUDIO_STREAM_REQUESTED.load(Ordering::Acquire)
        || AUDIO_STREAM_ACTIVE.load(Ordering::Acquire)
    {
        return;
    }

    let desc = dev_info.descriptor();
    let vendor_id = desc.vendor_id;
    let product_id = desc.product_id;

    let mut device = match host.open_device(dev_info).await {
        Ok(device) => device,
        Err(err) => {
            crate::log!(
                "crabusb: target {:04X}:{:04X} audio open failed: {:?}\n",
                vendor_id,
                product_id,
                err
            );
            return;
        }
    };

    let configs = device.configurations().to_vec();
    let Some(preferred) = pick_preferred_alt(&configs) else {
        return;
    };
    let Some(endpoint) = find_iso_out_endpoint(
        &configs,
        preferred.interface_number,
        preferred.alternate_setting,
    ) else {
        crate::log!(
            "crabusb: target {:04X}:{:04X} preferred if#{} alt={} has no iso-out endpoint\n",
            vendor_id,
            product_id,
            preferred.interface_number,
            preferred.alternate_setting
        );
        return;
    };

    match device
        .claim_interface(preferred.interface_number, preferred.alternate_setting)
        .await
    {
        Ok(()) => {
            crate::log!(
                "crabusb: target {:04X}:{:04X} audio ownership if#{} alt={} ep=0x{:02X}\n",
                vendor_id,
                product_id,
                preferred.interface_number,
                preferred.alternate_setting,
                endpoint.address
            );
            stream_target_audio(&mut device, vendor_id, product_id, preferred, endpoint).await;
        }
        Err(err) => crate::log!(
            "crabusb: target {:04X}:{:04X} audio claim failed if#{} alt={}: {:?}\n",
            vendor_id,
            product_id,
            preferred.interface_number,
            preferred.alternate_setting,
            err
        ),
    }
}

async fn log_opened_device_graph(
    host: &mut USBHost,
    dev_idx: usize,
    dev_info: &crab_usb::DeviceInfo,
) {
    let device = match host.open_device(dev_info).await {
        Ok(device) => device,
        Err(err) => {
            let desc = dev_info.descriptor();
            crate::log!(
                "crabusb: open dev#{} {:04X}:{:04X} failed: {:?}\n",
                dev_idx,
                desc.vendor_id,
                desc.product_id,
                err
            );
            return;
        }
    };

    let desc = device.descriptor();
    let vendor_id = desc.vendor_id;
    let product_id = desc.product_id;
    crate::log!(
        "crabusb: open dev#{} slot={} vid={:04X} pid={:04X} mfg={}\n",
        dev_idx,
        device.slot_id(),
        vendor_id,
        product_id,
        device.manufacturer().unwrap_or("-")
    );

    let configs = device.configurations().to_vec();
    if let Some(target) = super::mass::pick_mass_target(&configs) {
        crate::log!(
            "crabusb: mass {:04X}:{:04X} cfg={} if#{} alt={} bulk_in=0x{:02X} bulk_out=0x{:02X} class={:02X} subclass={:02X} proto={:02X}\n",
            vendor_id,
            product_id,
            target.configuration_value,
            target.interface_number,
            target.alternate_setting,
            target.bulk_in,
            target.bulk_out,
            target.class,
            target.subclass,
            target.protocol
        );
    }
    if let Some(preferred) = pick_preferred_alt(&configs)
        && (preferred.has_iso_out || preferred.class == 0x01)
    {
        crate::log!(
            "crabusb: audio-candidate {:04X}:{:04X} if#{} alt={} class={:02X} subclass={:02X} proto={:02X} iso_out={}\n",
            vendor_id,
            product_id,
            preferred.interface_number,
            preferred.alternate_setting,
            preferred.class,
            preferred.subclass,
            preferred.protocol,
            preferred.has_iso_out
        );
    }

    for config in configs.iter() {
        for interface in config.interfaces.iter() {
            for alt in interface.alt_settings.iter() {
                crate::log!(
                    "crabusb: open dev#{} if#{} alt={} desc-only class={:02X} subclass={:02X} proto={:02X}\n",
                    dev_idx,
                    alt.interface_number,
                    alt.alternate_setting,
                    alt.class,
                    alt.subclass,
                    alt.protocol
                );
                for ep in alt.endpoints.iter() {
                    let ep_num = ep.address & 0x0F;
                    crate::log!(
                        "crabusb: open dev#{} if#{} alt={} ep=0x{:02X} num={} desc-only mps={} interval={}\n",
                        dev_idx,
                        alt.interface_number,
                        alt.alternate_setting,
                        ep.address,
                        ep_num,
                        ep.max_packet_size,
                        ep.interval
                    );
                }
            }
        }
    }
}

async fn probe_and_log(host: &mut USBHost, spawner: &Spawner, controller_id: u32) -> bool {
    match host.probe_devices().await {
        Ok(devices) => {
            if devices.is_empty() {
                if !ROOT_PORT_CHANGE_SEEN.load(Ordering::Acquire)
                    && NO_PORT_CHANGE_HINT_LOGGED
                        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                        .is_ok()
                {
                    crate::log!(
                        "crabusb: no root-port change events observed; controller may be empty or downstream devices are not handed to the guest\n"
                    );
                }
                false
            } else {
                NO_PORT_CHANGE_HINT_LOGGED.store(false, Ordering::Release);
                crate::log!("crabusb: discovered {} new device(s)\n", devices.len());
                for dev in devices.iter() {
                    let desc = dev.descriptor();
                    crate::log!(
                        "crabusb: dev {:04X}:{:04X} class={:02X} subclass={:02X} proto={:02X}\n",
                        desc.vendor_id,
                        desc.product_id,
                        desc.class,
                        desc.subclass,
                        desc.protocol
                    );
                    maybe_start_truekey_bridge(host, dev).await;
                    if descriptor_has_audio_candidate(dev) {
                        maybe_start_target_audio(host, dev).await;
                    }
                    let _ =
                        super::hid::leds::maybe_start_led_controller(host, dev, spawner, controller_id)
                            .await;
                    let _ = maybe_start_hid_boot_streams(host, dev, spawner, controller_id).await;
                    let _ = super::midi::maybe_start_midi(host, dev, spawner, controller_id).await;
                    let _ = super::pen::maybe_start_mass_storage(host, dev, spawner, controller_id)
                        .await;
                }
                true
            }
        }
        Err(err) => {
            crate::log!("crabusb: probe failed: {:?}\n", err);
            false
        }
    }
}

async fn crab_scout_once(host: &mut USBHost, info: super::TlbUsbController, spawner: &Spawner) {
    if INITIAL_SNAPSHOT_LOGGED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }

    crate::log!("crabusb: scout begin\n");
    match host.probe_devices().await {
        Ok(devices) => {
            crate::log!("crabusb: scout devices={}\n", devices.len());
            for (dev_idx, dev) in devices.iter().enumerate() {
                let desc = dev.descriptor();
                crate::log!(
                    "crabusb: scout dev#{} vid={:04X} pid={:04X} class={:02X} subclass={:02X} proto={:02X} cfgs={}\n",
                    dev_idx,
                    desc.vendor_id,
                    desc.product_id,
                    desc.class,
                    desc.subclass,
                    desc.protocol,
                    dev.configurations().len()
                );
                for iface in dev.interface_descriptors() {
                    crate::log!(
                        "crabusb: scout dev#{} if#{} alt={} class={:02X} subclass={:02X} proto={:02X} eps={}\n",
                        dev_idx,
                        iface.interface_number,
                        iface.alternate_setting,
                        iface.class,
                        iface.subclass,
                        iface.protocol,
                        iface.endpoints.len()
                    );
                }
            }
            for (dev_idx, dev) in devices.iter().enumerate() {
                let desc = dev.descriptor();
                if desc.vendor_id == TRUEKEY_VENDOR_ID && desc.product_id == TRUEKEY_PRODUCT_ID {
                    maybe_start_truekey_bridge(host, dev).await;
                    continue;
                }
                if descriptor_has_audio_candidate(dev) {
                    maybe_start_target_audio(host, dev).await;
                }
                if super::hid::leds::maybe_start_led_controller(
                    host,
                    dev,
                    spawner,
                    info.index as u32,
                )
                .await
                {
                    continue;
                }
                if maybe_start_hid_boot_streams(host, dev, spawner, info.index as u32).await {
                    continue;
                }
                if super::midi::maybe_start_midi(host, dev, spawner, info.index as u32).await {
                    continue;
                }
                if super::pen::maybe_start_mass_storage(host, dev, spawner, info.index as u32).await
                {
                    continue;
                }
                log_opened_device_graph(host, dev_idx, dev).await;
            }
        }
        Err(err) => crate::log!("crabusb: scout probe failed: {:?}\n", err),
    }
    crate::log!("crabusb: scout end\n");
}

fn install_event_handler(handler: EventHandler) {
    *EVENT_HANDLER.lock() = Some(handler);
    EVENT_HANDLER_READY.store(true, Ordering::Release);
}

fn uninstall_event_handler() {
    EVENT_HANDLER_READY.store(false, Ordering::Release);
    *EVENT_HANDLER.lock() = None;
}

#[embassy_executor::task]
pub async fn event_pump_task() {
    loop {
        if !EVENT_HANDLER_READY.load(Ordering::Acquire) {
            Timer::after(EmbassyDuration::from_millis(10)).await;
            continue;
        }

        let event = {
            let guard = EVENT_HANDLER.lock();
            guard.as_ref().map(|handler| handler.handle_event())
        };

        match event {
            Some(Event::Nothing) | None => {
                Timer::after(EmbassyDuration::from_millis(1)).await;
            }
            Some(Event::PortChange { port }) => {
                crate::log!("crabusb: pump port change on root port {}\n", port);
                ROOT_PORT_CHANGE_SEEN.store(true, Ordering::Release);
                NO_PORT_CHANGE_HINT_LOGGED.store(false, Ordering::Release);
                PROBE_REQUESTED.store(true, Ordering::Release);
                Timer::after(EmbassyDuration::from_millis(1)).await;
            }
            Some(Event::Stopped) => {
                crate::log!("crabusb: pump observed stopped event\n");
                uninstall_event_handler();
                Timer::after(EmbassyDuration::from_millis(10)).await;
            }
        }
    }
}

#[embassy_executor::task]
pub async fn bsp_service(spawner: Spawner) {
    const OFFLINE_RETRY_MS: u64 = 1000;

    // The BSP host owner performs the one-time initial scout immediately after
    // init. Mark services as requested here so boot-present devices are
    // eligible for handoff even if the separate "armed" tasks have not run yet.
    AUDIO_STREAM_REQUESTED.store(true, Ordering::Release);
    TRUEKEY_STREAM_REQUESTED.store(true, Ordering::Release);

    loop {
        let Some(info) = super::discover_first_controller() else {
            crate::log!("crabusb: no xhci controller available yet; retrying on BSP\n");
            Timer::after(EmbassyDuration::from_millis(OFFLINE_RETRY_MS)).await;
            continue;
        };

        crate::log!(
            "crabusb: BSP service binding controller {} at {:02X}:{:02X}.{} vid={:04X} pid={:04X} mmio={:p}\n",
            info.index,
            info.bus,
            info.slot,
            info.function,
            info.vendor_id,
            info.device_id,
            info.mmio_base
        );

        crate::pci::enable_mem_and_bus_master(info.bus, info.slot, info.function);

        let mmio = info.mmio_base;

        let mut host = match USBHost::new_xhci(mmio, &CRABUSB_KERNEL) {
            Ok(host) => host,
            Err(err) => {
                crate::log!(
                    "crabusb: failed to create host for controller {}: {:?}\n",
                    info.index,
                    err
                );
                Timer::after(EmbassyDuration::from_millis(OFFLINE_RETRY_MS)).await;
                continue;
            }
        };

        install_event_handler(host.create_event_handler());

        if let Err(err) = host.init().await {
            crate::log!(
                "crabusb: host init failed for controller {}: {:?}\n",
                info.index,
                err
            );
            uninstall_event_handler();
            Timer::after(EmbassyDuration::from_millis(OFFLINE_RETRY_MS)).await;
            continue;
        }

        ROOT_PORT_CHANGE_SEEN.store(false, Ordering::Release);
        NO_PORT_CHANGE_HINT_LOGGED.store(false, Ordering::Release);
        crab_scout_once(&mut host, info, &spawner).await;

        let mut idle_ticks = 0u32;
        loop {
            if !EVENT_HANDLER_READY.load(Ordering::Acquire) {
                crate::log!("crabusb: event handler stopped; rediscovering controller\n");
                break;
            }

            if PROBE_REQUESTED.swap(false, Ordering::AcqRel) {
                crate::log!(
                    "crabusb: servicing pending probe on controller {}\n",
                    info.index
                );
                idle_ticks = 0;
                let _ = probe_and_log(&mut host, &spawner, info.index as u32).await;
                continue;
            }

            idle_ticks = idle_ticks.wrapping_add(1);
            if idle_ticks >= 300 {
                idle_ticks = 0;
                let _ = probe_and_log(&mut host, &spawner, info.index as u32).await;
            }
            Timer::after(EmbassyDuration::from_millis(10)).await;
        }
        uninstall_event_handler();
        Timer::after(EmbassyDuration::from_millis(OFFLINE_RETRY_MS)).await;
    }
}

#[embassy_executor::task]
pub async fn audio_task() {
    AUDIO_STREAM_REQUESTED.store(true, Ordering::Release);
    crate::log!("crabusb: audio service armed\n");
    loop {
        Timer::after(EmbassyDuration::from_secs(5)).await;
        if AUDIO_STREAM_ACTIVE.load(Ordering::Acquire) {
            crate::log!("crabusb: audio service streaming\n");
        } else {
            crate::log!("crabusb: audio service waiting for target audio device\n");
        }
    }
}

#[embassy_executor::task]
pub async fn truekey_task() {
    TRUEKEY_STREAM_REQUESTED.store(true, Ordering::Release);
    crate::log!("crabusb: truekey service armed\n");
    loop {
        Timer::after(EmbassyDuration::from_secs(5)).await;
        if TRUEKEY_STREAM_ACTIVE.load(Ordering::Acquire) {
            crate::log!("crabusb: truekey service streaming\n");
        } else {
            crate::log!("crabusb: truekey service waiting for scout handoff\n");
        }
    }
}
