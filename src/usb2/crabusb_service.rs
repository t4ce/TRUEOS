use alloc::vec::Vec;
use core::alloc::Layout;
use core::cmp::min;
use core::f32::consts::TAU;
use core::num::NonZeroUsize;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicBool, Ordering};
use core::time::Duration;

use crab_usb::{
    DmaAddr, DmaDirection, DmaError, DmaHandle, DmaMapHandle, DmaOp, EndpointKind, Event,
    EventHandler, KernelOp, USBHost, usb_if,
};
use embassy_time::{Duration as EmbassyDuration, Timer};
use libm::sinf;
use spin::Mutex;
use usb_if::host::ControlSetup;
use usb_if::transfer::{Recipient, Request, RequestType};

struct TrueosCrabUsbKernel;

static CRABUSB_KERNEL: TrueosCrabUsbKernel = TrueosCrabUsbKernel;
static INITIAL_SNAPSHOT_LOGGED: AtomicBool = AtomicBool::new(false);
static EVENT_HANDLER_READY: AtomicBool = AtomicBool::new(false);
static EVENT_HANDLER: Mutex<Option<EventHandler>> = Mutex::new(None);
static PROBE_REQUESTED: AtomicBool = AtomicBool::new(false);
static AUDIO_STREAM_REQUESTED: AtomicBool = AtomicBool::new(false);
static AUDIO_STREAM_ACTIVE: AtomicBool = AtomicBool::new(false);
static TRUEKEY_STREAM_REQUESTED: AtomicBool = AtomicBool::new(false);
static TRUEKEY_STREAM_ACTIVE: AtomicBool = AtomicBool::new(false);

const DEMO_WAV_EMBEDDED: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/demo.wav"));
const AUDIO_FRAME_BYTES: usize = 4; // s16le stereo
const TRUEKEY_VENDOR_ID: u16 = 0x303A;
const TRUEKEY_PRODUCT_ID: u16 = 0x1001;
const TRUEKEY_STREAM_CHUNK: usize = 512;

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
            crate::pci::dma::alloc_with_max(layout.size(), layout.align(), max_phys_exclusive)
                .ok_or(DmaError::NoMemory)?;
        let bounce_virt = NonNull::new(bounce_virt).ok_or(DmaError::NoMemory)?;

        Ok(unsafe {
            DmaMapHandle::new(addr, DmaAddr::from(bounce_phys), layout, Some(bounce_virt))
        })
    }

    unsafe fn unmap_single(&self, handle: DmaMapHandle) {
        if let Some(alloc_virt) = handle.alloc_virt() {
            crate::pci::dma::dealloc(alloc_virt.as_ptr(), handle.size());
        }
    }

    unsafe fn alloc_coherent(&self, dma_mask: u64, layout: Layout) -> Option<DmaHandle> {
        let max_phys_exclusive = if dma_mask == u64::MAX {
            None
        } else {
            dma_mask.checked_add(1)
        };
        let (phys, virt) =
            crate::pci::dma::alloc_with_max(layout.size(), layout.align(), max_phys_exclusive)?;
        let virt = NonNull::new(virt)?;
        Some(unsafe { DmaHandle::new(virt, DmaAddr::from(phys), layout) })
    }

    unsafe fn dealloc_coherent(&self, handle: DmaHandle) {
        crate::pci::dma::dealloc(handle.as_ptr().as_ptr(), handle.size());
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

fn endpoint_kind_name(kind: &EndpointKind) -> &'static str {
    match kind {
        EndpointKind::Control(_) => "control",
        EndpointKind::IsochronousIn(_) => "iso-in",
        EndpointKind::IsochronousOut(_) => "iso-out",
        EndpointKind::BulkIn(_) => "bulk-in",
        EndpointKind::BulkOut(_) => "bulk-out",
        EndpointKind::InterruptIn(_) => "intr-in",
        EndpointKind::InterruptOut(_) => "intr-out",
    }
}

const HYPERX_VENDOR_ID: u16 = 0x0951;
const HYPERX_PRODUCT_ID: u16 = 0x16A4;

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
struct SerialTarget {
    control_interface: Option<u8>,
    data_interface: u8,
    alternate_setting: u8,
    out_endpoint: u8,
    out_max_packet_size: u16,
    class: u8,
    subclass: u8,
    protocol: u8,
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

fn pick_serial_target(
    configs: &[usb_if::descriptor::ConfigurationDescriptor],
) -> Option<SerialTarget> {
    let mut best: Option<(u32, SerialTarget)> = None;

    for config in configs.iter() {
        let mut control_interface = None;
        for interface in config.interfaces.iter() {
            for alt in interface.alt_settings.iter() {
                if alt.class == 0x02 {
                    control_interface = Some(alt.interface_number);
                    break;
                }
            }
            if control_interface.is_some() {
                break;
            }
        }

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
                if control_interface.is_some() {
                    score += 10;
                }
                score += alt.endpoints.len() as u32;
                score += u32::from(alt.alternate_setting);

                let target = SerialTarget {
                    control_interface,
                    data_interface: alt.interface_number,
                    alternate_setting: alt.alternate_setting,
                    out_endpoint: out_ep.address,
                    out_max_packet_size: out_ep.max_packet_size,
                    class: alt.class,
                    subclass: alt.subclass,
                    protocol: alt.protocol,
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

async fn configure_cdc_acm_bridge(
    device: &mut crab_usb::Device,
    control_interface: u8,
) -> crab_usb::err::Result {
    let line_coding = [
        0x00, 0x10, 0x0E, 0x00, // 921600 baud, LE
        0x00, // 1 stop bit
        0x00, // no parity
        0x08, // 8 data bits
    ];

    device
        .ep_ctrl()
        .control_out(
            ControlSetup {
                request_type: RequestType::Class,
                recipient: Recipient::Interface,
                request: Request::Other(0x20), // SET_LINE_CODING
                value: 0,
                index: control_interface as u16,
            },
            &line_coding,
        )
        .await?;

    device
        .ep_ctrl()
        .control_out(
            ControlSetup {
                request_type: RequestType::Class,
                recipient: Recipient::Interface,
                request: Request::Other(0x22), // SET_CONTROL_LINE_STATE
                value: 0x0003,                 // DTR | RTS
                index: control_interface as u16,
            },
            &[],
        )
        .await?;

    Ok(())
}

async fn stream_truekey_logs(
    device: &mut crab_usb::Device,
    vendor_id: u16,
    product_id: u16,
    target: SerialTarget,
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
        "crabusb: truekey streaming start {:04X}:{:04X} if#{} alt={} ep=0x{:02X} mps={} class={:02X} subclass={:02X} proto={:02X}\n",
        vendor_id,
        product_id,
        target.data_interface,
        target.alternate_setting,
        target.out_endpoint,
        target.out_max_packet_size,
        target.class,
        target.subclass,
        target.protocol
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
    let Some(target) = pick_serial_target(&configs) else {
        crate::log!(
            "crabusb: truekey {:04X}:{:04X} no serial target found\n",
            vendor_id,
            product_id
        );
        return;
    };

    crate::log!(
        "crabusb: truekey {:04X}:{:04X} selected if#{} alt={} ep=0x{:02X} ctrl_if={:?} class={:02X} subclass={:02X} proto={:02X}\n",
        vendor_id,
        product_id,
        target.data_interface,
        target.alternate_setting,
        target.out_endpoint,
        target.control_interface,
        target.class,
        target.subclass,
        target.protocol
    );

    crate::log!(
        "crabusb: truekey {:04X}:{:04X} data-only probe ctrl_if={:?}\n",
        vendor_id,
        product_id,
        target.control_interface
    );

    crate::log!(
        "crabusb: truekey {:04X}:{:04X} data claim begin if#{} alt={}\n",
        vendor_id,
        product_id,
        target.data_interface,
        target.alternate_setting
    );
    match device
        .claim_interface(target.data_interface, target.alternate_setting)
        .await
    {
        Ok(()) => {
            crate::log!(
                "crabusb: truekey {:04X}:{:04X} ownership if#{} alt={} ep=0x{:02X}\n",
                vendor_id,
                product_id,
                target.data_interface,
                target.alternate_setting,
                target.out_endpoint
            );
            stream_truekey_logs(&mut device, vendor_id, product_id, target).await;
        }
        Err(err) => crate::log!(
            "crabusb: truekey {:04X}:{:04X} data if#{} alt={} claim failed: {:?}\n",
            vendor_id,
            product_id,
            target.data_interface,
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
    if vendor_id != HYPERX_VENDOR_ID || product_id != HYPERX_PRODUCT_ID {
        return;
    }

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
        crate::log!(
            "crabusb: target {:04X}:{:04X} no preferred interface found\n",
            vendor_id,
            product_id
        );
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
    let mut device = match host.open_device(dev_info).await {
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
    let descriptor_only = vendor_id == TRUEKEY_VENDOR_ID && product_id == TRUEKEY_PRODUCT_ID;
    if vendor_id == HYPERX_VENDOR_ID
        && product_id == HYPERX_PRODUCT_ID
        && let Some(preferred) = pick_preferred_alt(&configs)
    {
        crate::log!(
            "crabusb: target {:04X}:{:04X} preferred if#{} alt={} class={:02X} subclass={:02X} proto={:02X} iso_out={}\n",
            vendor_id,
            product_id,
            preferred.interface_number,
            preferred.alternate_setting,
            preferred.class,
            preferred.subclass,
            preferred.protocol,
            preferred.has_iso_out
        );

        match device
            .claim_interface(preferred.interface_number, preferred.alternate_setting)
            .await
        {
            Ok(()) => crate::log!(
                "crabusb: target {:04X}:{:04X} selected if#{} alt={}\n",
                vendor_id,
                product_id,
                preferred.interface_number,
                preferred.alternate_setting
            ),
            Err(err) => crate::log!(
                "crabusb: target {:04X}:{:04X} preferred if#{} alt={} claim failed: {:?}\n",
                vendor_id,
                product_id,
                preferred.interface_number,
                preferred.alternate_setting,
                err
            ),
        }
    }

    for config in configs.iter() {
        for interface in config.interfaces.iter() {
            for alt in interface.alt_settings.iter() {
                if descriptor_only {
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
                    continue;
                }

                match device
                    .claim_interface(interface.interface_number, alt.alternate_setting)
                    .await
                {
                    Ok(()) => {
                        crate::log!(
                            "crabusb: open dev#{} if#{} alt={} claim ok class={:02X} subclass={:02X} proto={:02X}\n",
                            dev_idx,
                            alt.interface_number,
                            alt.alternate_setting,
                            alt.class,
                            alt.subclass,
                            alt.protocol
                        );
                    }
                    Err(err) => {
                        crate::log!(
                            "crabusb: open dev#{} if#{} alt={} claim failed: {:?}\n",
                            dev_idx,
                            alt.interface_number,
                            alt.alternate_setting,
                            err
                        );
                        continue;
                    }
                }

                for ep in alt.endpoints.iter() {
                    let ep_num = ep.address & 0x0F;
                    match device.get_endpoint(ep.address).await {
                        Ok(kind) => {
                            crate::log!(
                                "crabusb: open dev#{} if#{} alt={} ep=0x{:02X} num={} kind={} mps={} interval={}\n",
                                dev_idx,
                                alt.interface_number,
                                alt.alternate_setting,
                                ep.address,
                                ep_num,
                                endpoint_kind_name(&kind),
                                ep.max_packet_size,
                                ep.interval
                            );
                        }
                        Err(err) => {
                            crate::log!(
                                "crabusb: open dev#{} if#{} alt={} ep=0x{:02X} get failed: {:?}\n",
                                dev_idx,
                                alt.interface_number,
                                alt.alternate_setting,
                                ep.address,
                                err
                            );
                        }
                    }
                }
            }
        }
    }
}

async fn probe_and_log(host: &mut USBHost) {
    match host.probe_devices().await {
        Ok(devices) => {
            if devices.is_empty() {
                crate::log!("crabusb: no newly discovered devices\n");
            } else {
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
                    maybe_start_target_audio(host, dev).await;
                }
            }
        }
        Err(err) => crate::log!("crabusb: probe failed: {:?}\n", err),
    }
}

async fn crab_scout_once(host: &mut USBHost, info: super::super::xhci::XhcInfo) {
    if INITIAL_SNAPSHOT_LOGGED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }

    crate::log!(
        "crabusb: one-time snapshot controller={} bdf={:02X}:{:02X}.{}\n",
        info.controller_id,
        info.bus,
        info.slot,
        info.function
    );

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
                if desc.vendor_id == HYPERX_VENDOR_ID && desc.product_id == HYPERX_PRODUCT_ID {
                    maybe_start_target_audio(host, dev).await;
                    continue;
                }
                log_opened_device_graph(host, dev_idx, dev).await;
            }
        }
        Err(err) => crate::log!("crabusb: scout probe failed: {:?}\n", err),
    }
    crate::log!("crabusb: scout end\n");
}

fn discover_first_controller() -> Option<super::super::xhci::XhcInfo> {
    crate::pci::enumerate_impl();
    super::super::xhci::init_once();
    super::super::xhci::xhc_list().iter().copied().next()
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
pub async fn bsp_service() {
    const OFFLINE_RETRY_MS: u64 = 1000;

    // The BSP host owner performs the one-time initial scout immediately after
    // init. Mark services as requested here so boot-present devices are
    // eligible for handoff even if the separate "armed" tasks have not run yet.
    AUDIO_STREAM_REQUESTED.store(true, Ordering::Release);
    TRUEKEY_STREAM_REQUESTED.store(true, Ordering::Release);

    loop {
        let Some(info) = discover_first_controller() else {
            crate::log!("crabusb: no xhci controller available yet; retrying on BSP\n");
            Timer::after(EmbassyDuration::from_millis(OFFLINE_RETRY_MS)).await;
            continue;
        };

        crate::log!(
            "crabusb: BSP service binding controller {} at {:02X}:{:02X}.{}\n",
            info.controller_id,
            info.bus,
            info.slot,
            info.function
        );

        let mut host = match USBHost::new_xhci(info.mmio_base, &CRABUSB_KERNEL) {
            Ok(host) => host,
            Err(err) => {
                crate::log!(
                    "crabusb: failed to create host for controller {}: {:?}\n",
                    info.controller_id,
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
                info.controller_id,
                err
            );
            uninstall_event_handler();
            Timer::after(EmbassyDuration::from_millis(OFFLINE_RETRY_MS)).await;
            continue;
        }

        crate::log!(
            "crabusb: host init complete for controller {}\n",
            info.controller_id
        );
        PROBE_REQUESTED.store(true, Ordering::Release);
        crab_scout_once(&mut host, info).await;
        probe_and_log(&mut host).await;

        let mut idle_ticks = 0u32;
        loop {
            if !EVENT_HANDLER_READY.load(Ordering::Acquire) {
                crate::log!("crabusb: event handler stopped; rediscovering controller\n");
                break;
            }

            if PROBE_REQUESTED.swap(false, Ordering::AcqRel) {
                crate::log!(
                    "crabusb: servicing pending probe on controller {}\n",
                    info.controller_id
                );
                idle_ticks = 0;
                probe_and_log(&mut host).await;
                continue;
            }

            idle_ticks = idle_ticks.wrapping_add(1);
            if idle_ticks >= 300 {
                idle_ticks = 0;
                probe_and_log(&mut host).await;
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
            crate::log!("crabusb: truekey service waiting for target serial device\n");
        }
    }
}
