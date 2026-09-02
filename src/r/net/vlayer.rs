//! Kernel-side vlayer helpers used by narrow runtime ABI shims.

extern crate alloc;

use alloc::string::{String, ToString};
use core::fmt::Write;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DnsResolveError {
    Runtime,
    BadName,
    NoNic,
    Timeout,
    NoAnswer,
}

impl From<super::dns::DnsError> for DnsResolveError {
    fn from(err: super::dns::DnsError) -> Self {
        match err {
            super::dns::DnsError::BadName => Self::BadName,
            super::dns::DnsError::NoNic => Self::NoNic,
            super::dns::DnsError::Timeout => Self::Timeout,
            super::dns::DnsError::NoAnswer => Self::NoAnswer,
            super::dns::DnsError::Runtime => Self::Runtime,
        }
    }
}

pub fn dns_resolve_error_code(err: DnsResolveError) -> u64 {
    match err {
        DnsResolveError::Runtime => 1,
        DnsResolveError::BadName => 2,
        DnsResolveError::NoNic => 3,
        DnsResolveError::Timeout => 4,
        DnsResolveError::NoAnswer => 5,
    }
}

pub fn dns_resolve_error_from_code(code: u64) -> DnsResolveError {
    match code {
        2 => DnsResolveError::BadName,
        3 => DnsResolveError::NoNic,
        4 => DnsResolveError::Timeout,
        5 => DnsResolveError::NoAnswer,
        _ => DnsResolveError::Runtime,
    }
}

pub fn resolve_ipv4_for_sync_abi(host: &str) -> Result<[u8; 4], DnsResolveError> {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return resolve_ipv4_for_sync_abi_guest_vmcall(host);
    }
    resolve_ipv4_for_sync_abi_host(host)
}

pub fn resolve_ipv4_for_sync_abi_host(host: &str) -> Result<[u8; 4], DnsResolveError> {
    let profile = crate::r::net::NetProfile::default();
    let dev_idx = profile
        .resolve_device_index()
        .ok_or(DnsResolveError::NoNic)?;
    let host = String::from(host);
    // This function is synchronous only because the current Blueprint C ABI
    // and guest vmcall ABI promise that shape. The carrier lane parks while a
    // BSP task owns and polls the actual network future; never restore a local
    // executor-polling bridge here.
    match super::dns_request_broker::resolve_ipv4(dev_idx, host) {
        Ok(result) => result,
        Err(error) => {
            crate::log_error!(target: "net";
                "dns sync ABI: request rejected reason={:?} cpu={} executor_poll={}\n",
                error,
                crate::percpu::this_cpu().cpu_index(),
                crate::percpu::in_executor_poll(),
            );
            Err(DnsResolveError::Runtime)
        }
    }
}

fn resolve_ipv4_for_sync_abi_guest_vmcall(host: &str) -> Result<[u8; 4], DnsResolveError> {
    if host.is_empty() || host.len() > trueos_vm::vmcall::PAYLOAD_CAP {
        return Err(DnsResolveError::BadName);
    }
    let mut out = [0u8; 4];
    let (status, data) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_DNS_RESOLVE_IPV4,
        0,
        0,
        host.as_bytes(),
        &mut out,
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return Err(DnsResolveError::Runtime);
    }
    if data != 0 {
        return Err(dns_resolve_error_from_code(data));
    }
    Ok(out)
}

pub fn rapl_snapshot_len_host() -> usize {
    crate::power::rapl::latest_snapshot_text().len()
}

pub fn rapl_snapshot_read_host(offset: usize, out: &mut [u8]) -> usize {
    if out.is_empty() {
        return 0;
    }

    let text = crate::power::rapl::latest_snapshot_text();
    let bytes = text.as_bytes();
    if offset >= bytes.len() {
        return 0;
    }

    let n = core::cmp::min(out.len(), bytes.len() - offset);
    out[..n].copy_from_slice(&bytes[offset..offset + n]);
    n
}

pub fn rapl_history_len_host() -> usize {
    crate::power::rapl::history_len()
}

pub fn rapl_history_read_host(offset: usize, out: &mut [u8]) -> usize {
    crate::power::rapl::copy_history_slice(offset, out)
}

pub fn thermal_snapshot_len_host() -> usize {
    crate::power::thermal::latest_snapshot_text().len()
}

pub fn thermal_snapshot_read_host(offset: usize, out: &mut [u8]) -> usize {
    if out.is_empty() {
        return 0;
    }

    let text = crate::power::thermal::latest_snapshot_text();
    let bytes = text.as_bytes();
    if offset >= bytes.len() {
        return 0;
    }

    let n = core::cmp::min(out.len(), bytes.len() - offset);
    out[..n].copy_from_slice(&bytes[offset..offset + n]);
    n
}

pub fn vram_snapshot_len_host() -> usize {
    crate::gpu::vram::latest_snapshot_text().len()
}

pub fn vram_snapshot_read_host(offset: usize, out: &mut [u8]) -> usize {
    if out.is_empty() {
        return 0;
    }

    let text = crate::gpu::vram::latest_snapshot_text();
    let bytes = text.as_bytes();
    if offset >= bytes.len() {
        return 0;
    }

    let n = core::cmp::min(out.len(), bytes.len() - offset);
    out[..n].copy_from_slice(&bytes[offset..offset + n]);
    n
}

pub fn system_services_snapshot_len_host() -> usize {
    crate::r::services::spawn_service::latest_system_service_snapshot_text().len()
}

pub fn system_services_snapshot_read_host(offset: usize, out: &mut [u8]) -> usize {
    if out.is_empty() {
        return 0;
    }

    let text = crate::r::services::spawn_service::latest_system_service_snapshot_text();
    let bytes = text.as_bytes();
    if offset >= bytes.len() {
        return 0;
    }

    let n = core::cmp::min(out.len(), bytes.len() - offset);
    out[..n].copy_from_slice(&bytes[offset..offset + n]);
    n
}

pub fn bios_schema_snapshot_len_host() -> usize {
    crate::shell2::cmds::bios_blueprint::snapshot_len()
}

pub fn bios_schema_snapshot_read_host(offset: usize, out: &mut [u8]) -> usize {
    crate::shell2::cmds::bios_blueprint::snapshot_read(offset, out)
}

pub fn printer_snapshot_len_host() -> usize {
    crate::r::net::printer::snapshot_text().len()
}

pub fn printer_snapshot_read_host(offset: usize, out: &mut [u8]) -> usize {
    if out.is_empty() {
        return 0;
    }

    let text = crate::r::net::printer::snapshot_text();
    let bytes = text.as_bytes();
    if offset >= bytes.len() {
        return 0;
    }

    let n = core::cmp::min(out.len(), bytes.len() - offset);
    out[..n].copy_from_slice(&bytes[offset..offset + n]);
    n
}

pub fn pci_snapshot_text_host() -> String {
    ensure_pci_devices_enumerated();

    let mut out = String::new();
    let mut count = 0usize;
    crate::pci::with_devices(|list| {
        count = list.len();
    });

    let _ = writeln!(out, "trueos pci snapshot v1");
    let _ = writeln!(out, "device_count={}", count);
    let _ = writeln!(
        out,
        "dev,bdf,vendor_id,device_id,class,subclass,prog_if,class_name,role,command,status,name"
    );

    crate::pci::with_devices(|list| {
        for dev in list {
            let bdf = alloc::format!("{:02X}:{:02X}.{}", dev.bus, dev.slot, dev.function);
            let class_name = pci_class_name(dev.class, dev.subclass);
            let role = pci_role(dev.class, dev.subclass);
            let command = crate::pci::config_read_u16(dev.bus, dev.slot, dev.function, 0x04);
            let status = crate::pci::config_read_u16(dev.bus, dev.slot, dev.function, 0x06);
            let name = alloc::format!("{} {:04X}:{:04X}", class_name, dev.vendor, dev.device);
            let _ = writeln!(
                out,
                "dev,{},{:04X},{:04X},{:02X},{:02X},{:02X},{},{},0x{:04X},0x{:04X},{}",
                bdf,
                dev.vendor,
                dev.device,
                dev.class,
                dev.subclass,
                dev.prog_if,
                class_name,
                role,
                command,
                status,
                name
            );

            let mut bar_idx = 0u8;
            while bar_idx < 6 {
                let (bar_lo, bar_hi) =
                    crate::pci::read_bar_raw(dev.bus, dev.slot, dev.function, bar_idx);
                let decoded = decode_pci_bar(bar_lo, bar_hi);
                if decoded.present {
                    let size = crate::pci::bar_size_bytes(dev.bus, dev.slot, dev.function, bar_idx)
                        .map(|bytes| alloc::format!("0x{:X}", bytes))
                        .unwrap_or_else(|| String::from("-"));
                    let _ = writeln!(
                        out,
                        "bar,{},{},{},{},{},0x{:016X},{},{}",
                        bdf,
                        bar_idx,
                        decoded.kind,
                        decoded.width,
                        if decoded.prefetchable { 1 } else { 0 },
                        decoded.base,
                        size,
                        format_bar_raw(bar_lo, bar_hi)
                    );
                }
                bar_idx += if decoded.is_64 { 2 } else { 1 };
            }
        }
    });

    out
}

fn ensure_pci_devices_enumerated() {
    let mut len = 0usize;
    crate::pci::with_devices(|list| {
        len = list.len();
    });
    if len == 0 {
        crate::pci::enumerate_impl();
    }
}

pub fn pci_snapshot_len_host() -> usize {
    pci_snapshot_text_host().len()
}

pub fn pci_snapshot_read_host(offset: usize, out: &mut [u8]) -> usize {
    if out.is_empty() {
        return 0;
    }

    let text = pci_snapshot_text_host();
    let bytes = text.as_bytes();
    if offset >= bytes.len() {
        return 0;
    }

    let n = core::cmp::min(out.len(), bytes.len() - offset);
    out[..n].copy_from_slice(&bytes[offset..offset + n]);
    n
}

pub fn usb_snapshot_text_host() -> String {
    let snapshot = crate::usb2::tlb_usb_snapshot();
    let mut out = String::new();
    let _ = writeln!(out, "trueos usb snapshot v1");
    let _ = writeln!(
        out,
        "summary\t{}\t{}\t{}",
        snapshot.devices.len(),
        snapshot
            .probe_device_count
            .map(|count| count.to_string())
            .unwrap_or_else(|| String::from("-")),
        snapshot.probe_error.unwrap_or("-")
    );

    for controller in &snapshot.controllers {
        let runtime = crate::usb2::crabusb_runtime_diag(controller.index);
        let _ = writeln!(
            out,
            "controller\t{}\t{:02X}:{:02X}.{}\t{:04X}\t{:04X}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            controller.index,
            controller.bus,
            controller.slot,
            controller.function,
            controller.vendor_id,
            controller.device_id,
            controller.controller_phase,
            controller.root_hub_lifecycle,
            u8::from(controller.event_ready),
            u8::from(controller.root_port_change_seen),
            controller.empty_probe_streak,
            runtime.last_probe_state,
            runtime.last_probe_device_count,
        );

        if let Some(mmio) = crate::usb2::controller_mmio_diag(controller.index) {
            for port in mmio.ports {
                let speed = usb_port_speed_name(port.portsc);
                let link_state = usb_port_link_state_name(port.portsc);
                let _ = writeln!(
                    out,
                    "port\t{}\t{}\t{}\t{}\t{}\t{}\t0x{:08X}\t0x{:08X}\t0x{:08X}",
                    controller.index,
                    port.port_id,
                    u8::from((port.portsc & 1) != 0),
                    u8::from((port.portsc & 2) != 0),
                    speed,
                    link_state,
                    port.portsc,
                    port.portpmsc,
                    port.portli,
                );
            }
        }
    }

    for device in &snapshot.devices {
        let parent = device
            .parent_hub_slot_id
            .map(|slot| slot.to_string())
            .unwrap_or_else(|| String::from("-"));
        let path = join_usb_path(&device.path);
        let _ = writeln!(
            out,
            "device\t{:08X}\t0\t{}\t{}\t{}\t0x{:05X}\t{}\t{:04X}\t{:04X}\t{:02X}\t{:02X}\t{:02X}\t{:04X}\t{:04X}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            device.stable_id,
            device.slot_id,
            device.root_port_id,
            device.port_id,
            device.route_string,
            device.speed,
            device.vendor_id,
            device.product_id,
            device.class,
            device.subclass,
            device.protocol,
            device.usb_version,
            device.device_version,
            device.num_configurations,
            device.max_packet_size_0,
            parent,
            sanitize_usb_field(device.manufacturer.as_deref().unwrap_or("")),
            sanitize_usb_field(device.product.as_deref().unwrap_or("")),
            sanitize_usb_field(device.serial.as_deref().unwrap_or("")),
            path,
        );
        for configuration in &device.configurations {
            let _ = writeln!(
                out,
                "config\t{:08X}\t{}\t0x{:02X}\t{}",
                device.stable_id,
                configuration.configuration_value,
                configuration.attributes,
                configuration.max_power,
            );
            for interface in &configuration.interfaces {
                let _ = writeln!(
                    out,
                    "interface\t{:08X}\t{}\t{}\t{}\t{:02X}\t{:02X}\t{:02X}",
                    device.stable_id,
                    configuration.configuration_value,
                    interface.interface_number,
                    interface.alternate_setting,
                    interface.class,
                    interface.subclass,
                    interface.protocol,
                );
                for endpoint in &interface.endpoints {
                    let _ = writeln!(
                        out,
                        "endpoint\t{:08X}\t{}\t{}\t{}\t0x{:02X}\t{}\t{}\t{}",
                        device.stable_id,
                        configuration.configuration_value,
                        interface.interface_number,
                        interface.alternate_setting,
                        endpoint.address,
                        endpoint.transfer_type,
                        endpoint.max_packet_size,
                        endpoint.interval,
                    );
                }
            }
        }
        for hop in &device.hub_path {
            let _ = writeln!(
                out,
                "hop\t{:08X}\t{}\t{}\t{}\t{}",
                device.stable_id, hop.slot_id, hop.port_id, hop.hub_depth, hop.speed,
            );
        }
    }

    for node in &snapshot.topology {
        let kind = match node.kind {
            crate::usb2::TlbUsbTopologyNodeKind::RootPort => "root",
            crate::usb2::TlbUsbTopologyNodeKind::Hub => "hub",
            crate::usb2::TlbUsbTopologyNodeKind::Device => "device",
        };
        let _ = writeln!(
            out,
            "topology\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            kind,
            node.controller_index,
            node.root_port_id,
            node.port_id,
            node.depth,
            optional_usb_u8(node.slot_id),
            optional_usb_u8(node.parent_slot_id),
            node.speed,
            optional_usb_u16_hex(node.vendor_id),
            optional_usb_u16_hex(node.product_id),
            optional_usb_u8_hex(node.class),
            optional_usb_u8_hex(node.subclass),
            optional_usb_u8_hex(node.protocol),
        );
    }

    out
}

pub fn usb_snapshot_len_host() -> usize {
    usb_snapshot_text_host().len()
}

pub fn usb_snapshot_read_host(offset: usize, out: &mut [u8]) -> usize {
    if out.is_empty() {
        return 0;
    }
    let text = usb_snapshot_text_host();
    let bytes = text.as_bytes();
    if offset >= bytes.len() {
        return 0;
    }
    let n = core::cmp::min(out.len(), bytes.len() - offset);
    out[..n].copy_from_slice(&bytes[offset..offset + n]);
    n
}

fn sanitize_usb_field(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if matches!(ch, '\t' | '\r' | '\n') {
                ' '
            } else {
                ch
            }
        })
        .collect()
}

fn join_usb_path(path: &[u8]) -> String {
    let mut out = String::new();
    for (index, port) in path.iter().enumerate() {
        if index != 0 {
            out.push('.');
        }
        let _ = write!(out, "{port}");
    }
    out
}

fn optional_usb_u8(value: Option<u8>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| String::from("-"))
}

fn optional_usb_u8_hex(value: Option<u8>) -> String {
    value
        .map(|value| alloc::format!("{value:02X}"))
        .unwrap_or_else(|| String::from("-"))
}

fn optional_usb_u16_hex(value: Option<u16>) -> String {
    value
        .map(|value| alloc::format!("{value:04X}"))
        .unwrap_or_else(|| String::from("-"))
}

fn usb_port_speed_name(portsc: u32) -> &'static str {
    match (portsc >> 10) & 0x0f {
        1 => "full",
        2 => "low",
        3 => "high",
        4 => "super",
        5 => "super+",
        _ => "unknown",
    }
}

fn usb_port_link_state_name(portsc: u32) -> &'static str {
    match (portsc >> 5) & 0x0f {
        0 => "u0",
        1 => "u1",
        2 => "u2",
        3 => "u3",
        4 => "disabled",
        5 => "rx-detect",
        6 => "inactive",
        7 => "polling",
        8 => "recovery",
        9 => "hot-reset",
        10 => "compliance",
        11 => "test",
        15 => "resume",
        _ => "reserved",
    }
}

pub unsafe extern "C" fn trueos_vlayer_rapl_snapshot_read(
    offset: usize,
    out_ptr: *mut u8,
    out_cap: usize,
) -> isize {
    vlayer_read_runtime(
        trueos_vm::vmcall::OP_BP_RAPL_SNAPSHOT_READ,
        rapl_snapshot_len_host,
        rapl_snapshot_read_host,
        offset,
        out_ptr,
        out_cap,
    )
}

pub unsafe extern "C" fn trueos_vlayer_rapl_history_read(
    offset: usize,
    out_ptr: *mut u8,
    out_cap: usize,
) -> isize {
    vlayer_read_runtime(
        trueos_vm::vmcall::OP_BP_RAPL_HISTORY_READ,
        rapl_history_len_host,
        rapl_history_read_host,
        offset,
        out_ptr,
        out_cap,
    )
}

pub unsafe extern "C" fn trueos_vlayer_pci_snapshot_read(
    offset: usize,
    out_ptr: *mut u8,
    out_cap: usize,
) -> isize {
    vlayer_read_runtime(
        trueos_vm::vmcall::OP_BP_PCI_SNAPSHOT_READ,
        pci_snapshot_len_host,
        pci_snapshot_read_host,
        offset,
        out_ptr,
        out_cap,
    )
}

pub unsafe extern "C" fn trueos_vlayer_usb_snapshot_read(
    offset: usize,
    out_ptr: *mut u8,
    out_cap: usize,
) -> isize {
    vlayer_read_runtime(
        trueos_vm::vmcall::OP_BP_USB_SNAPSHOT_READ,
        usb_snapshot_len_host,
        usb_snapshot_read_host,
        offset,
        out_ptr,
        out_cap,
    )
}

pub unsafe extern "C" fn trueos_vlayer_thermal_snapshot_read(
    offset: usize,
    out_ptr: *mut u8,
    out_cap: usize,
) -> isize {
    vlayer_read_runtime(
        trueos_vm::vmcall::OP_BP_THERMAL_SNAPSHOT_READ,
        thermal_snapshot_len_host,
        thermal_snapshot_read_host,
        offset,
        out_ptr,
        out_cap,
    )
}

pub unsafe extern "C" fn trueos_vlayer_vram_snapshot_read(
    offset: usize,
    out_ptr: *mut u8,
    out_cap: usize,
) -> isize {
    vlayer_read_runtime(
        trueos_vm::vmcall::OP_BP_VRAM_SNAPSHOT_READ,
        vram_snapshot_len_host,
        vram_snapshot_read_host,
        offset,
        out_ptr,
        out_cap,
    )
}

pub unsafe extern "C" fn trueos_vlayer_system_services_snapshot_read(
    offset: usize,
    out_ptr: *mut u8,
    out_cap: usize,
) -> isize {
    vlayer_read_runtime(
        trueos_vm::vmcall::OP_BP_SYSTEM_SERVICES_SNAPSHOT_READ,
        system_services_snapshot_len_host,
        system_services_snapshot_read_host,
        offset,
        out_ptr,
        out_cap,
    )
}

pub unsafe extern "C" fn trueos_vlayer_bios_schema_snapshot_read(
    offset: usize,
    out_ptr: *mut u8,
    out_cap: usize,
) -> isize {
    vlayer_read_runtime(
        trueos_vm::vmcall::OP_BP_BIOS_SCHEMA_SNAPSHOT_READ,
        bios_schema_snapshot_len_host,
        bios_schema_snapshot_read_host,
        offset,
        out_ptr,
        out_cap,
    )
}

pub unsafe extern "C" fn trueos_vlayer_printer_snapshot_read(
    offset: usize,
    out_ptr: *mut u8,
    out_cap: usize,
) -> isize {
    vlayer_read_runtime(
        trueos_vm::vmcall::OP_BP_PRINTER_SNAPSHOT_READ,
        printer_snapshot_len_host,
        printer_snapshot_read_host,
        offset,
        out_ptr,
        out_cap,
    )
}

pub unsafe extern "C" fn trueos_vlayer_print2d_submit(
    document_kind: u32,
    subject: u64,
    raw_ptr: *const u8,
    raw_len: usize,
) -> i64 {
    let raw = if raw_len == 0 {
        &[][..]
    } else {
        if raw_ptr.is_null() {
            return crate::r::print2d::ERROR_INVALID_DOCUMENT;
        }
        // SAFETY: the ABI caller promises `raw_len` readable bytes.
        unsafe { core::slice::from_raw_parts(raw_ptr, raw_len) }
    };

    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, data) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_PRINT2D_SUBMIT,
            u64::from(document_kind),
            subject,
            raw,
            &mut [],
        );
        return if status == trueos_vm::vmcall::STATUS_OK {
            data as i64
        } else {
            crate::r::print2d::ERROR_TRANSPORT
        };
    }

    let Some(owner) = crate::hv::current_guest_execution_context_vm_id() else {
        return crate::r::print2d::ERROR_NOT_OWNER;
    };
    crate::r::print2d::submit_for_owner(owner, document_kind, subject, raw)
}

pub extern "C" fn trueos_vlayer_print2d_status(job_id: u32) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, data) =
            trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_PRINT2D_STATUS, u64::from(job_id), 0);
        return if status == trueos_vm::vmcall::STATUS_OK {
            data as i64 as i32
        } else {
            crate::r::print2d::ERROR_TRANSPORT as i32
        };
    }

    let Some(owner) = crate::hv::current_guest_execution_context_vm_id() else {
        return crate::r::print2d::ERROR_NOT_OWNER as i32;
    };
    crate::r::print2d::status_for_owner(owner, job_id)
}

fn vlayer_read_runtime(
    vmcall_op: u32,
    host_len: fn() -> usize,
    host_read: fn(usize, &mut [u8]) -> usize,
    offset: usize,
    out_ptr: *mut u8,
    out_cap: usize,
) -> isize {
    if out_ptr.is_null() || out_cap == 0 {
        return if crate::hv::current_hull_guest_context_vm_id().is_some() {
            vlayer_len_guest_vmcall(vmcall_op)
        } else {
            host_len() as isize
        };
    }

    let out = unsafe { core::slice::from_raw_parts_mut(out_ptr, out_cap) };
    let copied = if crate::hv::current_hull_guest_context_vm_id().is_some() {
        vlayer_read_guest_vmcall(vmcall_op, offset, out)
    } else {
        host_read(offset, out) as isize
    };
    copied
}

fn vlayer_len_guest_vmcall(vmcall_op: u32) -> isize {
    let (status, len) = trueos_vm::vmcall::call(vmcall_op, 0, 0);
    if status == trueos_vm::vmcall::STATUS_OK {
        len as isize
    } else {
        -1
    }
}

fn vlayer_read_guest_vmcall(vmcall_op: u32, offset: usize, out: &mut [u8]) -> isize {
    let mut copied = 0usize;
    while copied < out.len() {
        let chunk_cap = core::cmp::min(out.len() - copied, trueos_vm::vmcall::PAYLOAD_CAP);
        let (status, count) = trueos_vm::vmcall::call_with_payload(
            vmcall_op,
            offset.saturating_add(copied) as u64,
            chunk_cap as u64,
            &[],
            &mut out[copied..copied + chunk_cap],
        );
        if status != trueos_vm::vmcall::STATUS_OK {
            return -1;
        }
        let count = core::cmp::min(count as usize, chunk_cap);
        if count == 0 {
            break;
        }
        copied = copied.saturating_add(count);
    }
    copied as isize
}

struct PciBarDecoded {
    present: bool,
    kind: &'static str,
    width: &'static str,
    prefetchable: bool,
    base: u64,
    is_64: bool,
}

fn decode_pci_bar(bar_lo: u32, bar_hi: Option<u32>) -> PciBarDecoded {
    if bar_lo == 0 || bar_lo == 0xFFFF_FFFF {
        return PciBarDecoded {
            present: false,
            kind: "none",
            width: "-",
            prefetchable: false,
            base: 0,
            is_64: false,
        };
    }

    if (bar_lo & 0x1) != 0 {
        return PciBarDecoded {
            present: true,
            kind: "io",
            width: "32",
            prefetchable: false,
            base: (bar_lo & !0x3) as u64,
            is_64: false,
        };
    }

    let is_64 = ((bar_lo >> 1) & 0x3) == 0x2;
    let base = if is_64 {
        (((bar_hi.unwrap_or(0) as u64) << 32) | (bar_lo as u64)) & !0xFu64
    } else {
        (bar_lo as u64) & !0xFu64
    };

    PciBarDecoded {
        present: true,
        kind: "mem",
        width: if is_64 { "64" } else { "32" },
        prefetchable: (bar_lo & 0x8) != 0,
        base,
        is_64,
    }
}

fn format_bar_raw(bar_lo: u32, bar_hi: Option<u32>) -> String {
    if let Some(hi) = bar_hi {
        alloc::format!("0x{:08X}:{:08X}", hi, bar_lo)
    } else {
        alloc::format!("0x{:08X}", bar_lo)
    }
}

fn pci_class_name(class: u8, subclass: u8) -> &'static str {
    match class {
        0x00 => "unclassified",
        0x01 => match subclass {
            0x06 => "sata",
            0x08 => "nvme",
            _ => "storage",
        },
        0x02 => "network",
        0x03 => "display",
        0x04 => "multimedia",
        0x05 => "memory",
        0x06 => match subclass {
            0x00 => "host bridge",
            0x01 => "isa bridge",
            0x04 => "pci bridge",
            _ => "bridge",
        },
        0x07 => "communication",
        0x08 => "system peripheral",
        0x09 => "input",
        0x0A => "dock",
        0x0B => "processor",
        0x0C => match subclass {
            0x03 => "usb",
            0x05 => "smbus",
            _ => "serial bus",
        },
        0x0D => "wireless",
        0x10 => "encryption",
        0x11 => "signal processing",
        _ => "other",
    }
}

fn pci_role(class: u8, subclass: u8) -> &'static str {
    match class {
        0x01 => "storage",
        0x02 => "network",
        0x03 => "display",
        0x04 => "media",
        0x06 => "bridge",
        0x09 => "input",
        0x0C if subclass == 0x03 => "usb",
        0x0C => "bus",
        _ => "system",
    }
}
