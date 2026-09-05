use alloc::{string::String, vec::Vec};
use core::mem::size_of;
use x86_64::registers::control::Cr3;

use crate::efi::EfiTableHeader;
use crate::shell2::shell2_cmd::ParseOutcome;

use super::super::{ShellBackend2, print_shell_line};

const TRPAY1_MAGIC: [u8; 8] = *b"TRPAY1\0\0";
const TRPAY1_VERSION: u16 = 1;
const TRBSR1_MAGIC: [u8; 8] = *b"TRBSR1\0\0";
const TRBSR1_VERSION: u16 = 1;
const SEC_BOOT_SERVICES: u32 = 4;
const MAX_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;
const MAX_SECTIONS: usize = 16;
const MAX_RETAINED_RANGES: usize = 512;
const MAX_RETAINED_TOTAL_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const CAPTURE_FLAG_BOOT_SERVICES_RETAINED: u32 = 1u32 << 31;
const BOOT_SERVICES_RANGE_EXECUTABLE: u32 = 1u32 << 0;
const EFI_BOOT_SERVICES_CODE: u32 = 3;
const EFI_BOOT_SERVICES_DATA: u32 = 4;
const EFI_BOOT_SERVICES_SIGNATURE: u64 = 0x5652_4553_544F_4F42; // "BOOTSERV"
const PROBE_PAYLOAD: &[u8] = b"TRUEOS/UEFI/BootServices/CalculateCrc32/v1";
const PROBE_EXPECTED_CRC32: u32 = 0xB667_632F;

const PAGE_PRESENT: u64 = 1 << 0;
const PAGE_HUGE: u64 = 1 << 7;
const PAGE_PHYS_MASK: u64 = 0x000F_FFFF_FFFF_F000;
const PAGE_2M: u64 = 2 * 1024 * 1024;
const PAGE_1G: u64 = 1024 * 1024 * 1024;
const PAGE_4K: u64 = 4096;

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct Trpay1Header {
    magic: [u8; 8],
    version: u16,
    header_bytes: u16,
    section_entry_bytes: u16,
    reserved0: u16,
    section_count: u32,
    total_bytes: u32,
    capture_flags: u32,
    reserved1: u32,
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct Trpay1Section {
    kind: u32,
    flags: u32,
    offset: u32,
    length: u32,
    crc32: u32,
    reserved: u32,
    status: u64,
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct Trbsr1Header {
    magic: [u8; 8],
    version: u16,
    header_bytes: u16,
    entry_bytes: u16,
    reserved0: u16,
    count: u32,
    flags: u32,
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct Trbsr1Entry {
    physical_start: u64,
    length: u64,
    memory_type: u32,
    flags: u32,
}

// UEFI 2.x EFI_BOOT_SERVICES layout through CalculateCrc32. We intentionally
// stop there: this experiment calls exactly one side-effect-free Boot Service.
#[repr(C)]
#[derive(Clone, Copy)]
struct EfiBootServicesThroughCrc32 {
    hdr: EfiTableHeader,
    raise_tpl: usize,
    restore_tpl: usize,
    allocate_pages: usize,
    free_pages: usize,
    get_memory_map: usize,
    allocate_pool: usize,
    free_pool: usize,
    create_event: usize,
    set_timer: usize,
    wait_for_event: usize,
    signal_event: usize,
    close_event: usize,
    check_event: usize,
    install_protocol_interface: usize,
    reinstall_protocol_interface: usize,
    uninstall_protocol_interface: usize,
    handle_protocol: usize,
    reserved: usize,
    register_protocol_notify: usize,
    locate_handle: usize,
    locate_device_path: usize,
    install_configuration_table: usize,
    load_image: usize,
    start_image: usize,
    exit: usize,
    unload_image: usize,
    exit_boot_services: usize,
    get_next_monotonic_count: usize,
    stall: usize,
    set_watchdog_timer: usize,
    connect_controller: usize,
    disconnect_controller: usize,
    open_protocol: usize,
    close_protocol: usize,
    open_protocol_information: usize,
    protocols_per_handle: usize,
    locate_handle_buffer: usize,
    locate_protocol: usize,
    install_multiple_protocol_interfaces: usize,
    uninstall_multiple_protocol_interfaces: usize,
    calculate_crc32: usize,
}

#[derive(Clone, Copy)]
struct RetainedRange {
    physical_start: u64,
    length: u64,
    executable: bool,
    memory_type: u32,
}

struct RetainedCapture {
    capture_flags: u32,
    ranges: Vec<RetainedRange>,
}

struct IdentityMapStats {
    ranges: usize,
    code_ranges: usize,
    data_ranges: usize,
    bytes: u64,
}

struct ProbeResult {
    capture_flags: u32,
    boot_services_raw: u64,
    boot_services_phys: u64,
    calculate_crc32_raw: u64,
    calculate_crc32_phys: u64,
    table_revision: u32,
    table_header_bytes: u32,
    identity: IdentityMapStats,
    firmware_status: usize,
    firmware_crc32: u32,
}

pub(crate) fn try_parse(io: &'static dyn ShellBackend2) -> ParseOutcome {
    print_shell_line(io, "=== TRUEOS Live UEFI Boot Services CRC32 Probe ===");
    print_shell_line(
        io,
        "policy=experimental single-call probe; retained BootServicesCode/Data identity map + CalculateCrc32 only; no firmware allocation, protocol mutation, device I/O, variable write, reset, or firmware write",
    );
    print_shell_line(
        io,
        alloc::format!(
            "payload=\"{}\" bytes={} expected_crc32=0x{:08X}",
            core::str::from_utf8(PROBE_PAYLOAD).unwrap_or("TRUEOS CRC32 probe"),
            PROBE_PAYLOAD.len(),
            PROBE_EXPECTED_CRC32
        )
        .as_str(),
    );

    match run_probe() {
        Ok(result) => {
            print_shell_line(
                io,
                alloc::format!(
                    "retention_marker=present capture_flags=0x{:08X}",
                    result.capture_flags
                )
                .as_str(),
            );
            print_shell_line(
                io,
                alloc::format!(
                    "firmware_identity_map ranges={} code={} data={} bytes={} active_cr3=yes",
                    result.identity.ranges,
                    result.identity.code_ranges,
                    result.identity.data_ranges,
                    result.identity.bytes
                )
                .as_str(),
            );
            print_shell_line(
                io,
                alloc::format!(
                    "boot_services raw=0x{:016X} phys=0x{:016X} identity_mapped=yes revision=0x{:08X} header_bytes=0x{:X}",
                    result.boot_services_raw,
                    result.boot_services_phys,
                    result.table_revision,
                    result.table_header_bytes
                )
                .as_str(),
            );
            print_shell_line(
                io,
                alloc::format!(
                    "CalculateCrc32 raw=0x{:016X} phys=0x{:016X} identity_mapped=yes",
                    result.calculate_crc32_raw,
                    result.calculate_crc32_phys
                )
                .as_str(),
            );
            print_shell_line(
                io,
                alloc::format!(
                    "firmware_status=0x{:016X} firmware_crc32=0x{:08X} expected_crc32=0x{:08X}",
                    result.firmware_status as u64,
                    result.firmware_crc32,
                    PROBE_EXPECTED_CRC32
                )
                .as_str(),
            );
            let pass = result.firmware_status == 0
                && result.firmware_crc32 == PROBE_EXPECTED_CRC32;
            print_shell_line(io, if pass { "match=yes result=PASS" } else { "match=no result=FAIL" });
        }
        Err(error) => {
            print_shell_line(
                io,
                alloc::format!("result=UNAVAILABLE detail=\"{}\"", error).as_str(),
            );
        }
    }

    ParseOutcome::Handled
}

fn run_probe() -> Result<ProbeResult, String> {
    let capture = retained_capture()?;
    let identity = install_retained_identity_map(&capture.ranges)?;

    let system_table = crate::efi::system_table()
        .ok_or_else(|| String::from("EFI system table unavailable"))?;
    let boot_services_raw = system_table.boot_services as u64;
    if boot_services_raw == 0 {
        return Err(String::from("EFI system table BootServices pointer is zero"));
    }

    let boot_services_phys = crate::limine::try_as_phys_addr(boot_services_raw)
        .ok_or_else(|| alloc::format!("BootServices pointer is not mappable: 0x{boot_services_raw:X}"))?;
    if !limine_range_covered(boot_services_phys, size_of::<EfiBootServicesThroughCrc32>() as u64) {
        return Err(String::from("BootServices table prefix crosses the Limine memory map"));
    }
    let mapping = crate::pci::mmio::map_limine_struct::<EfiBootServicesThroughCrc32>(
        boot_services_phys,
    )
    .map_err(|error| alloc::format!("BootServices map failed: {error:?}"))?;
    let boot_services = unsafe { core::ptr::read_unaligned(mapping.as_ptr()) };

    if boot_services.hdr.signature != EFI_BOOT_SERVICES_SIGNATURE {
        return Err(alloc::format!(
            "BootServices signature mismatch: 0x{:016X}",
            boot_services.hdr.signature
        ));
    }
    if usize::try_from(boot_services.hdr.header_size).unwrap_or(0)
        < size_of::<EfiBootServicesThroughCrc32>()
    {
        return Err(alloc::format!(
            "BootServices header too small: {} < {}",
            boot_services.hdr.header_size,
            size_of::<EfiBootServicesThroughCrc32>()
        ));
    }
    if boot_services.calculate_crc32 == 0 {
        return Err(String::from("BootServices CalculateCrc32 pointer is zero"));
    }

    let boot_services_identity = active_mapping_phys(boot_services_raw)
        .ok_or_else(|| String::from("BootServices table original address is not mapped by active CR3 after retained-range install"))?;
    if boot_services_identity != boot_services_phys {
        return Err(alloc::format!(
            "BootServices table is not identity-mapped: va=0x{boot_services_raw:X} maps_to=0x{boot_services_identity:X} expected=0x{boot_services_phys:X}"
        ));
    }

    let calculate_crc32_raw = boot_services.calculate_crc32 as u64;
    let calculate_crc32_phys = crate::limine::try_as_phys_addr(calculate_crc32_raw)
        .ok_or_else(|| {
            alloc::format!(
                "CalculateCrc32 pointer is not a known physical/HHDM address: 0x{calculate_crc32_raw:X}"
            )
        })?;
    if !limine_range_covered(calculate_crc32_phys, 1) {
        return Err(String::from("CalculateCrc32 code address is outside the Limine memory map"));
    }

    // The paired Limine handoff publishes every final EfiBootServicesCode/Data
    // descriptor before converting those pages to reserved ownership. Map the
    // exact set at its original physical addresses, then independently walk the
    // active CR3 before crossing back into firmware.
    let identity_phys = active_mapping_phys(calculate_crc32_raw)
        .ok_or_else(|| String::from("CalculateCrc32 original address is still not mapped after retained-range install"))?;
    if identity_phys != calculate_crc32_phys {
        return Err(alloc::format!(
            "CalculateCrc32 is not identity-mapped: va=0x{calculate_crc32_raw:X} maps_to=0x{identity_phys:X} expected=0x{calculate_crc32_phys:X}"
        ));
    }

    type CalculateCrc32 = unsafe extern "efiapi" fn(*const u8, usize, *mut u32) -> usize;
    let calculate: CalculateCrc32 = unsafe {
        core::mem::transmute::<usize, CalculateCrc32>(calculate_crc32_raw as usize)
    };
    let mut firmware_crc32 = 0u32;
    let firmware_status = unsafe {
        calculate(
            PROBE_PAYLOAD.as_ptr(),
            PROBE_PAYLOAD.len(),
            &mut firmware_crc32,
        )
    };

    Ok(ProbeResult {
        capture_flags: capture.capture_flags,
        boot_services_raw,
        boot_services_phys,
        calculate_crc32_raw,
        calculate_crc32_phys,
        table_revision: boot_services.hdr.revision,
        table_header_bytes: boot_services.hdr.header_size,
        identity,
        firmware_status,
        firmware_crc32,
    })
}

fn install_retained_identity_map(ranges: &[RetainedRange]) -> Result<IdentityMapStats, String> {
    if ranges.is_empty() {
        return Err(String::from("retained Boot Services range set is empty"));
    }

    let mut stats = IdentityMapStats {
        ranges: 0,
        code_ranges: 0,
        data_ranges: 0,
        bytes: 0,
    };

    for range in ranges {
        let size = usize::try_from(range.length)
            .map_err(|_| String::from("retained Boot Services range does not fit usize"))?;
        crate::pci::mmio::map_identity_ram_region(range.physical_start, size, range.executable)
            .map_err(|error| {
                alloc::format!(
                    "retained identity map failed base=0x{:X} bytes=0x{:X} type={} exec={} error={error:?}",
                    range.physical_start,
                    range.length,
                    range.memory_type,
                    range.executable
                )
            })?;
        stats.ranges += 1;
        stats.bytes = stats.bytes.checked_add(range.length)
            .ok_or_else(|| String::from("retained identity byte count overflow"))?;
        if range.executable {
            stats.code_ranges += 1;
        } else {
            stats.data_ranges += 1;
        }
    }

    Ok(stats)
}

fn retained_capture() -> Result<RetainedCapture, String> {
    let response = crate::limine::trueos_hii_capture_response()
        .ok_or_else(|| String::from("patched Limine TRUEOS capture response absent"))?;
    let payload_len = usize::try_from(response.size)
        .map_err(|_| String::from("Limine capture size does not fit usize"))?;
    if payload_len < size_of::<Trpay1Header>() || payload_len > MAX_PAYLOAD_BYTES {
        return Err(alloc::format!(
            "Limine capture size outside bound: {payload_len}"
        ));
    }
    let payload_phys = crate::limine::try_as_phys_addr(response.address)
        .ok_or_else(|| String::from("Limine capture pointer is not mappable"))?;
    if !limine_range_covered(payload_phys, payload_len as u64) {
        return Err(String::from("Limine capture payload crosses an unmapped physical hole"));
    }
    let mapping = crate::pci::mmio::map_mmio_region_exact(payload_phys, payload_len)
        .map_err(|error| alloc::format!("Limine capture payload map failed: {error:?}"))?;
    let payload = unsafe { core::slice::from_raw_parts(mapping.as_ptr(), payload_len) };

    let header = read_unaligned::<Trpay1Header>(payload, 0)
        .ok_or_else(|| String::from("Limine capture header truncated"))?;
    let magic = header.magic;
    let version = header.version;
    let header_bytes = usize::from(header.header_bytes);
    let section_entry_bytes = usize::from(header.section_entry_bytes);
    let section_count = header.section_count as usize;
    let total_bytes = header.total_bytes as usize;
    let capture_flags = header.capture_flags;

    if magic != TRPAY1_MAGIC || version != TRPAY1_VERSION {
        return Err(alloc::format!(
            "Limine capture identity mismatch magic_ok={} version={}",
            magic == TRPAY1_MAGIC,
            version
        ));
    }
    if header_bytes < size_of::<Trpay1Header>()
        || section_entry_bytes < size_of::<Trpay1Section>()
    {
        return Err(String::from("Limine capture header or section-entry size is too small"));
    }
    if section_count == 0 || section_count > MAX_SECTIONS || total_bytes != payload_len {
        return Err(alloc::format!(
            "Limine capture shape invalid sections={section_count} total_bytes={total_bytes} payload_bytes={payload_len}"
        ));
    }
    if capture_flags & CAPTURE_FLAG_BOOT_SERVICES_RETAINED == 0 {
        return Err(alloc::format!(
            "Limine did not prove retained Boot Services (capture_flags=0x{:08X})",
            capture_flags
        ));
    }

    let directory_bytes = section_count
        .checked_mul(section_entry_bytes)
        .ok_or_else(|| String::from("Limine section directory overflow"))?;
    let directory_end = header_bytes
        .checked_add(directory_bytes)
        .ok_or_else(|| String::from("Limine section directory overflow"))?;
    if directory_end > payload.len() {
        return Err(String::from("Limine section directory truncated"));
    }

    let mut retained_section = None;
    for index in 0..section_count {
        let offset = header_bytes
            .checked_add(index.checked_mul(section_entry_bytes)
                .ok_or_else(|| String::from("Limine section index overflow"))?)
            .ok_or_else(|| String::from("Limine section index overflow"))?;
        let section = read_unaligned::<Trpay1Section>(payload, offset)
            .ok_or_else(|| String::from("Limine section entry truncated"))?;
        let kind = section.kind;
        if kind == SEC_BOOT_SERVICES {
            if retained_section.is_some() {
                return Err(String::from("multiple retained Boot Services sections"));
            }
            retained_section = Some(section);
        }
    }

    let section = retained_section
        .ok_or_else(|| String::from("retained Boot Services range section absent; paired Limine update required"))?;
    let section_flags = section.flags;
    let section_status = section.status;
    let section_offset = section.offset as usize;
    let section_len = section.length as usize;
    let section_crc32 = section.crc32;
    let section_end = section_offset
        .checked_add(section_len)
        .ok_or_else(|| String::from("retained range section overflow"))?;
    if section_flags & 1 == 0 || section_status != 0 {
        return Err(alloc::format!(
            "retained range section not complete flags=0x{section_flags:08X} status=0x{section_status:016X}"
        ));
    }
    if section_len < size_of::<Trbsr1Header>()
        || section_offset < directory_end
        || section_end > payload.len()
    {
        return Err(String::from("retained range section bounds invalid"));
    }
    let section_bytes = &payload[section_offset..section_end];
    if crc32fast::hash(section_bytes) != section_crc32 {
        return Err(String::from("retained range section CRC mismatch"));
    }

    let range_header = read_unaligned::<Trbsr1Header>(section_bytes, 0)
        .ok_or_else(|| String::from("retained range header truncated"))?;
    let range_magic = range_header.magic;
    let range_version = range_header.version;
    let range_header_bytes = usize::from(range_header.header_bytes);
    let range_entry_bytes = usize::from(range_header.entry_bytes);
    let range_count = range_header.count as usize;
    let range_flags = range_header.flags;
    if range_magic != TRBSR1_MAGIC || range_version != TRBSR1_VERSION {
        return Err(alloc::format!(
            "retained range identity mismatch magic_ok={} version={}",
            range_magic == TRBSR1_MAGIC,
            range_version
        ));
    }
    if range_flags & 1 == 0
        || range_header_bytes < size_of::<Trbsr1Header>()
        || range_entry_bytes < size_of::<Trbsr1Entry>()
        || range_count == 0
        || range_count > MAX_RETAINED_RANGES
    {
        return Err(alloc::format!(
            "retained range shape invalid flags=0x{range_flags:08X} count={range_count} header_bytes={range_header_bytes} entry_bytes={range_entry_bytes}"
        ));
    }
    let ranges_end = range_header_bytes
        .checked_add(range_count.checked_mul(range_entry_bytes)
            .ok_or_else(|| String::from("retained range table overflow"))?)
        .ok_or_else(|| String::from("retained range table overflow"))?;
    if ranges_end > section_bytes.len() {
        return Err(String::from("retained range table truncated"));
    }

    let mut ranges = Vec::with_capacity(range_count);
    let mut total = 0u64;
    for index in 0..range_count {
        let offset = range_header_bytes
            .checked_add(index.checked_mul(range_entry_bytes)
                .ok_or_else(|| String::from("retained range index overflow"))?)
            .ok_or_else(|| String::from("retained range index overflow"))?;
        let entry = read_unaligned::<Trbsr1Entry>(section_bytes, offset)
            .ok_or_else(|| String::from("retained range entry truncated"))?;
        let physical_start = entry.physical_start;
        let length = entry.length;
        let memory_type = entry.memory_type;
        let flags = entry.flags;
        let executable = flags & BOOT_SERVICES_RANGE_EXECUTABLE != 0;

        if physical_start % PAGE_4K != 0 || length == 0 || length % PAGE_4K != 0 {
            return Err(alloc::format!(
                "retained range {index} is not page-shaped base=0x{physical_start:X} bytes=0x{length:X}"
            ));
        }
        match memory_type {
            EFI_BOOT_SERVICES_CODE if executable => {}
            EFI_BOOT_SERVICES_DATA if !executable => {}
            _ => {
                return Err(alloc::format!(
                    "retained range {index} type/exec mismatch type={memory_type} exec={executable}"
                ));
            }
        }
        let _end = physical_start.checked_add(length)
            .ok_or_else(|| String::from("retained range address overflow"))?;
        if !limine_range_covered(physical_start, length) {
            return Err(alloc::format!(
                "retained range {index} crosses a Limine memory-map hole base=0x{physical_start:X} bytes=0x{length:X}"
            ));
        }
        total = total.checked_add(length)
            .ok_or_else(|| String::from("retained range total overflow"))?;
        if total > MAX_RETAINED_TOTAL_BYTES {
            return Err(alloc::format!(
                "retained range total exceeds bound bytes={total} bound={MAX_RETAINED_TOTAL_BYTES}"
            ));
        }
        ranges.push(RetainedRange {
            physical_start,
            length,
            executable,
            memory_type,
        });
    }

    Ok(RetainedCapture {
        capture_flags,
        ranges,
    })
}

fn read_unaligned<T: Copy>(bytes: &[u8], offset: usize) -> Option<T> {
    let end = offset.checked_add(size_of::<T>())?;
    if end > bytes.len() {
        return None;
    }
    Some(unsafe { core::ptr::read_unaligned(bytes.as_ptr().add(offset).cast::<T>()) })
}

fn limine_range_covered(base: u64, length: u64) -> bool {
    if length == 0 {
        return false;
    }
    let Some(end) = base.checked_add(length) else {
        return false;
    };
    let Some(entries) = crate::limine::memmap_entries() else {
        return false;
    };

    let mut cursor = base;
    while cursor < end {
        let mut next = cursor;
        for entry in entries {
            let entry_end = entry.base.saturating_add(entry.length);
            if entry.base <= cursor && entry_end > cursor {
                next = next.max(entry_end.min(end));
            }
        }
        if next == cursor {
            return false;
        }
        cursor = next;
    }
    true
}

fn active_mapping_phys(virt: u64) -> Option<u64> {
    let hhdm = crate::limine::hhdm_offset()?;
    let (pml4_frame, _) = Cr3::read();
    let pml4_phys = pml4_frame.start_address().as_u64();

    let pml4_i = ((virt >> 39) & 0x1FF) as usize;
    let pdpt_i = ((virt >> 30) & 0x1FF) as usize;
    let pd_i = ((virt >> 21) & 0x1FF) as usize;
    let pt_i = ((virt >> 12) & 0x1FF) as usize;

    let pml4e = read_page_entry(hhdm, pml4_phys, pml4_i)?;
    if pml4e & PAGE_PRESENT == 0 {
        return None;
    }

    let pdpt_phys = pml4e & PAGE_PHYS_MASK;
    let pdpte = read_page_entry(hhdm, pdpt_phys, pdpt_i)?;
    if pdpte & PAGE_PRESENT == 0 {
        return None;
    }
    if pdpte & PAGE_HUGE != 0 {
        let base = (pdpte & PAGE_PHYS_MASK) & !(PAGE_1G - 1);
        return base.checked_add(virt & (PAGE_1G - 1));
    }

    let pd_phys = pdpte & PAGE_PHYS_MASK;
    let pde = read_page_entry(hhdm, pd_phys, pd_i)?;
    if pde & PAGE_PRESENT == 0 {
        return None;
    }
    if pde & PAGE_HUGE != 0 {
        let base = (pde & PAGE_PHYS_MASK) & !(PAGE_2M - 1);
        return base.checked_add(virt & (PAGE_2M - 1));
    }

    let pt_phys = pde & PAGE_PHYS_MASK;
    let pte = read_page_entry(hhdm, pt_phys, pt_i)?;
    if pte & PAGE_PRESENT == 0 {
        return None;
    }
    (pte & PAGE_PHYS_MASK).checked_add(virt & 0xFFF)
}

fn read_page_entry(hhdm: u64, table_phys: u64, index: usize) -> Option<u64> {
    let byte_offset = u64::try_from(index).ok()?.checked_mul(8)?;
    let address = hhdm.checked_add(table_phys)?.checked_add(byte_offset)?;
    Some(unsafe { core::ptr::read_volatile(address as *const u64) })
}
