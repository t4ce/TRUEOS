//! Kernel-side vlayer helpers used by narrow runtime ABI shims.

extern crate alloc;

use alloc::string::String;
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
    crate::wait::spawn_and_wait_local(async move {
        super::dns::resolve_ipv4_for_device(
            dev_idx,
            host.as_str(),
            super::dns::DnsConfig::for_profile(profile),
        )
        .await
    })
    .map_err(DnsResolveError::from)
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
