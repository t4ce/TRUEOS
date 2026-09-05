use alloc::string::String;
use core::mem::size_of;
use x86_64::registers::control::Cr3;

use crate::efi::EfiTableHeader;
use crate::shell2::shell2_cmd::ParseOutcome;

use super::super::{ShellBackend2, print_shell_line};

const TRPAY1_MAGIC: [u8; 8] = *b"TRPAY1\0\0";
const TRPAY1_VERSION: u16 = 1;
const MAX_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;
const CAPTURE_FLAG_BOOT_SERVICES_RETAINED: u32 = 1u32 << 31;
const EFI_BOOT_SERVICES_SIGNATURE: u64 = 0x5652_4553_544F_4F42; // "BOOTSERV"
const PROBE_PAYLOAD: &[u8] = b"TRUEOS/UEFI/BootServices/CalculateCrc32/v1";
const PROBE_EXPECTED_CRC32: u32 = 0xB667_632F;

const PAGE_PRESENT: u64 = 1 << 0;
const PAGE_HUGE: u64 = 1 << 7;
const PAGE_PHYS_MASK: u64 = 0x000F_FFFF_FFFF_F000;
const PAGE_2M: u64 = 2 * 1024 * 1024;
const PAGE_1G: u64 = 1024 * 1024 * 1024;

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

struct ProbeResult {
    capture_flags: u32,
    boot_services_raw: u64,
    boot_services_phys: u64,
    calculate_crc32_raw: u64,
    calculate_crc32_phys: u64,
    table_revision: u32,
    table_header_bytes: u32,
    firmware_status: usize,
    firmware_crc32: u32,
}

pub(crate) fn try_parse(io: &'static dyn ShellBackend2) -> ParseOutcome {
    print_shell_line(io, "=== TRUEOS Live UEFI Boot Services CRC32 Probe ===");
    print_shell_line(
        io,
        "policy=experimental single-call probe; CalculateCrc32 only; no allocation, protocol mutation, device I/O, variable write, reset, or firmware write",
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
                    "boot_services raw=0x{:016X} phys=0x{:016X} revision=0x{:08X} header_bytes=0x{:X}",
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
    let capture_flags = retained_capture_flags()?;
    let system_table = crate::efi::system_table()
        .ok_or_else(|| String::from("EFI system table unavailable"))?;
    let boot_services_raw = system_table.boot_services as u64;
    if boot_services_raw == 0 {
        return Err(String::from("EFI system table BootServices pointer is zero"));
    }

    let boot_services_phys = crate::limine::try_as_phys_addr(boot_services_raw)
        .ok_or_else(|| alloc::format!("BootServices pointer is not mappable: 0x{boot_services_raw:X}"))?;
    if !crate::limine::memmap_contains_phys_range(
        boot_services_phys,
        size_of::<EfiBootServicesThroughCrc32>(),
    ) {
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

    let calculate_crc32_raw = boot_services.calculate_crc32 as u64;
    let calculate_crc32_phys = crate::limine::try_as_phys_addr(calculate_crc32_raw)
        .ok_or_else(|| {
            alloc::format!(
                "CalculateCrc32 pointer is not a known physical/HHDM address: 0x{calculate_crc32_raw:X}"
            )
        })?;
    if !crate::limine::memmap_contains_phys_range(calculate_crc32_phys, 1) {
        return Err(String::from("CalculateCrc32 code address is outside the Limine memory map"));
    }

    // The first experiment deliberately calls firmware at its original UEFI
    // address rather than through an alias. Walk the active CR3 instead of
    // using TRUEOS's metadata-only virt->phys helper: Limine intentionally
    // leaves a low identity map on x86, but that compatibility mapping is not
    // part of the normal kernel address-translation API.
    let identity_phys = active_mapping_phys(calculate_crc32_raw)
        .ok_or_else(|| String::from("CalculateCrc32 original address is not mapped by active CR3"))?;
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
        capture_flags,
        boot_services_raw,
        boot_services_phys,
        calculate_crc32_raw,
        calculate_crc32_phys,
        table_revision: boot_services.hdr.revision,
        table_header_bytes: boot_services.hdr.header_size,
        firmware_status,
        firmware_crc32,
    })
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

fn retained_capture_flags() -> Result<u32, String> {
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
    if !crate::limine::memmap_contains_phys_range(payload_phys, size_of::<Trpay1Header>()) {
        return Err(String::from("Limine capture header crosses the memory map"));
    }
    let mapping = crate::pci::mmio::map_limine_struct::<Trpay1Header>(payload_phys)
        .map_err(|error| alloc::format!("Limine capture header map failed: {error:?}"))?;
    let header = unsafe { core::ptr::read_unaligned(mapping.as_ptr()) };
    // Never pass packed fields directly to formatting macros; copy them out so
    // Rust cannot form an unaligned reference while reporting probe failures.
    let magic = header.magic;
    let version = header.version;
    let header_bytes = header.header_bytes;
    let total_bytes = header.total_bytes;
    let capture_flags = header.capture_flags;

    if magic != TRPAY1_MAGIC || version != TRPAY1_VERSION {
        return Err(alloc::format!(
            "Limine capture identity mismatch magic_ok={} version={}",
            magic == TRPAY1_MAGIC,
            version
        ));
    }
    if usize::from(header_bytes) < size_of::<Trpay1Header>() {
        return Err(String::from("Limine capture header_bytes is too small"));
    }
    if total_bytes as usize > payload_len {
        return Err(String::from("Limine capture total_bytes exceeds response size"));
    }
    if capture_flags & CAPTURE_FLAG_BOOT_SERVICES_RETAINED == 0 {
        return Err(alloc::format!(
            "Limine did not prove retained Boot Services (capture_flags=0x{:08X})",
            capture_flags
        ));
    }
    Ok(capture_flags)
}
