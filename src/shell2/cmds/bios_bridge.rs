use alloc::string::String;
use core::mem::size_of;

use spin::Mutex;

use crate::efi::EfiTableHeader;
use crate::shell2::shell2_cmd::ParseOutcome;

use super::super::{ShellBackend2, print_shell_line};

const TRPAY1_MAGIC: [u8; 8] = *b"TRPAY1\0\0";
const TRPAY1_VERSION: u16 = 1;
const TRFWC1_MAGIC: [u8; 8] = *b"TRFWC1\0\0";
const TRFWC1_VERSION: u16 = 1;
const SEC_FIRMWARE_CONTEXT: u32 = 5;
const MAX_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;
const MAX_SECTIONS: usize = 16;

const CAPTURE_FLAG_BOOT_SERVICES_RETAINED: u32 = 1u32 << 31;
const FW_CONTEXT_COMPLETE: u32 = 1u32 << 0;
const FW_CONTEXT_QUIESCED: u32 = 1u32 << 1;
const FW_CONTEXT_CR3_RETAINED: u32 = 1u32 << 2;
const FW_CONTEXT_BRIDGE_READY: u32 = 1u32 << 3;
const FW_CONTEXT_WATCHDOG_DISABLED: u32 = 1u32 << 4;
const FW_CONTEXT_EXIT_GROUP_SENT: u32 = 1u32 << 5;
const REQUIRED_CONTEXT_FLAGS: u32 = FW_CONTEXT_COMPLETE
    | FW_CONTEXT_QUIESCED
    | FW_CONTEXT_CR3_RETAINED
    | FW_CONTEXT_BRIDGE_READY
    | FW_CONTEXT_EXIT_GROUP_SENT;

const EFI_BOOT_SERVICES_SIGNATURE: u64 = 0x5652_4553_544F_4F42; // "BOOTSERV"
const PROBE_PAYLOAD: &[u8] = b"TRUEOS/UEFI/BootServices/CalculateCrc32/v1";
const PROBE_EXPECTED_CRC32: u32 = 0xB667_632F;
const BRIDGE_PAYLOAD_BYTES: usize = 64;
const BRIDGE_CONTROL_BYTES: usize = 168;
const BRIDGE_PAYLOAD_OFFSET: u64 = 104;
const BRIDGE_CRC_OUTPUT_OFFSET: u64 = 96;
const CR4_LA57: u64 = 1 << 12;
const CR4_PCIDE: u64 = 1 << 17;
const CR4_ADDRESS_SPACE_MASK: u64 = CR4_LA57 | CR4_PCIDE;

static BRIDGE_LOCK: Mutex<()> = Mutex::new(());

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
struct Trfwc1Context {
    magic: [u8; 8],
    version: u16,
    bytes: u16,
    flags: u32,
    failure_stage: u32,
    reserved0: u32,
    firmware_cr3: u64,
    firmware_cr4: u64,
    boot_services_virtual: u64,
    boot_services_physical: u64,
    calculate_crc32_virtual: u64,
    calculate_crc32_physical: u64,
    bridge_entry_virtual: u64,
    bridge_entry_physical: u64,
    bridge_entry_bytes: u32,
    bridge_control_bytes: u32,
    bridge_control_virtual: u64,
    bridge_control_physical: u64,
    bridge_stack_base_virtual: u64,
    bridge_stack_bytes: u32,
    page_table_pages: u32,
}

#[repr(C)]
struct BridgeControl {
    firmware_cr3: u64,
    firmware_stack_top: u64,
    target: u64,
    arg0: u64,
    arg1: u64,
    arg2: u64,
    arg3: u64,
    result: u64,
    caller_cr3: u64,
    caller_rsp: u64,
    caller_cr4: u64,
    reserved: u64,
    crc_output: u32,
    payload_len: u32,
    payload: [u8; BRIDGE_PAYLOAD_BYTES],
}

// UEFI 2.x EFI_BOOT_SERVICES layout through CalculateCrc32. We inspect the
// table through a TRUEOS alias, but execute the function only after crossing
// into the retained firmware CR3.
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

struct BridgeCapture {
    capture_flags: u32,
    section_status: u64,
    context: Trfwc1Context,
}

struct ProbeResult {
    capture_flags: u32,
    context_flags: u32,
    firmware_cr3: u64,
    firmware_cr4: u64,
    page_table_pages: u32,
    bridge_entry_virtual: u64,
    bridge_entry_physical: u64,
    bridge_control_virtual: u64,
    bridge_control_physical: u64,
    bridge_stack_base_virtual: u64,
    bridge_stack_bytes: u32,
    boot_services_virtual: u64,
    boot_services_physical: u64,
    calculate_crc32_virtual: u64,
    calculate_crc32_physical: u64,
    firmware_status: u64,
    firmware_crc32: u32,
}

pub(crate) fn try_parse(io: &'static dyn ShellBackend2) -> ParseOutcome {
    print_shell_line(io, "=== TRUEOS Retained Firmware Context CRC32 Probe ===");
    print_shell_line(
        io,
        "policy=experimental firmware-context bridge v1; BSP only; firmware device owners quiesced; switch to retained firmware CR3 + private bridge stack; CalculateCrc32 only; no firmware allocation, device I/O, variable write, reset, or HII callback",
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
                    "firmware_context=retained capture_flags=0x{:08X} context_flags=0x{:08X} quiesced=yes exit_group=signaled watchdog={}",
                    result.capture_flags,
                    result.context_flags,
                    if result.context_flags & FW_CONTEXT_WATCHDOG_DISABLED != 0 {
                        "disabled"
                    } else {
                        "not-proven"
                    }
                )
                .as_str(),
            );
            print_shell_line(
                io,
                alloc::format!(
                    "firmware_address_space cr3=0x{:016X} cr4=0x{:016X} retained_page_tables={} transition=TRUEOS->firmware->TRUEOS",
                    result.firmware_cr3,
                    result.firmware_cr4,
                    result.page_table_pages
                )
                .as_str(),
            );
            print_shell_line(
                io,
                alloc::format!(
                    "bridge entry_va=0x{:016X} entry_pa=0x{:016X} control_va=0x{:016X} control_pa=0x{:016X} stack_va=0x{:016X} stack_bytes={}",
                    result.bridge_entry_virtual,
                    result.bridge_entry_physical,
                    result.bridge_control_virtual,
                    result.bridge_control_physical,
                    result.bridge_stack_base_virtual,
                    result.bridge_stack_bytes
                )
                .as_str(),
            );
            print_shell_line(
                io,
                alloc::format!(
                    "boot_services va=0x{:016X} pa=0x{:016X} CalculateCrc32_va=0x{:016X} CalculateCrc32_pa=0x{:016X}",
                    result.boot_services_virtual,
                    result.boot_services_physical,
                    result.calculate_crc32_virtual,
                    result.calculate_crc32_physical
                )
                .as_str(),
            );
            print_shell_line(
                io,
                alloc::format!(
                    "firmware_status=0x{:016X} firmware_crc32=0x{:08X} expected_crc32=0x{:08X}",
                    result.firmware_status,
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
    if crate::percpu::current_slot() != 0 {
        return Err(alloc::format!(
            "firmware bridge is BSP-only in v1; current_slot={}",
            crate::percpu::current_slot()
        ));
    }

    if size_of::<BridgeControl>() != BRIDGE_CONTROL_BYTES {
        return Err(alloc::format!(
            "bridge ABI size mismatch rust={} limine={BRIDGE_CONTROL_BYTES}",
            size_of::<BridgeControl>()
        ));
    }

    let capture = firmware_context_capture()?;
    let context = capture.context;
    let context_flags = context.flags;
    let failure_stage = context.failure_stage;

    if capture.capture_flags & CAPTURE_FLAG_BOOT_SERVICES_RETAINED == 0 {
        return Err(alloc::format!(
            "Limine quiesced but did not retain firmware context capture_flags=0x{:08X} context_flags=0x{:08X} failure_stage={}({}) section_status=0x{:016X}; fallback real ExitBootServices was used",
            capture.capture_flags,
            context_flags,
            failure_stage,
            failure_stage_name(failure_stage),
            capture.section_status
        ));
    }
    if context_flags & REQUIRED_CONTEXT_FLAGS != REQUIRED_CONTEXT_FLAGS {
        return Err(alloc::format!(
            "retained firmware context is incomplete flags=0x{context_flags:08X} required=0x{REQUIRED_CONTEXT_FLAGS:08X}"
        ));
    }

    let firmware_cr3 = context.firmware_cr3;
    let firmware_cr4 = context.firmware_cr4;
    let boot_services_virtual = context.boot_services_virtual;
    let boot_services_physical = context.boot_services_physical;
    let calculate_crc32_virtual = context.calculate_crc32_virtual;
    let calculate_crc32_physical = context.calculate_crc32_physical;
    let bridge_entry_virtual = context.bridge_entry_virtual;
    let bridge_entry_physical = context.bridge_entry_physical;
    let bridge_entry_bytes = context.bridge_entry_bytes as usize;
    let bridge_control_bytes = context.bridge_control_bytes as usize;
    let bridge_control_virtual = context.bridge_control_virtual;
    let bridge_control_physical = context.bridge_control_physical;
    let bridge_stack_base_virtual = context.bridge_stack_base_virtual;
    let bridge_stack_bytes = context.bridge_stack_bytes;
    let page_table_pages = context.page_table_pages;

    if firmware_cr3 & 0x000F_FFFF_FFFF_F000 == 0
        || boot_services_virtual == 0
        || boot_services_physical == 0
        || calculate_crc32_virtual == 0
        || calculate_crc32_physical == 0
        || bridge_entry_virtual == 0
        || bridge_entry_physical == 0
        || bridge_control_virtual == 0
        || bridge_control_physical == 0
    {
        return Err(String::from("retained firmware context contains a zero critical address"));
    }
    if bridge_entry_bytes == 0 || bridge_entry_bytes > 4096 {
        return Err(alloc::format!("bridge entry size invalid bytes={bridge_entry_bytes}"));
    }
    if bridge_control_bytes != BRIDGE_CONTROL_BYTES {
        return Err(alloc::format!(
            "bridge control ABI mismatch context={} expected={BRIDGE_CONTROL_BYTES}",
            bridge_control_bytes
        ));
    }
    if bridge_stack_bytes < 4096 || bridge_stack_bytes > 1024 * 1024 {
        return Err(alloc::format!("bridge stack size invalid bytes={bridge_stack_bytes}"));
    }
    let bridge_stack_top = bridge_stack_base_virtual
        .checked_add(u64::from(bridge_stack_bytes))
        .ok_or_else(|| String::from("bridge stack address overflow"))?
        & !0xFu64;

    let caller_cr4 = read_cr4();
    if caller_cr4 & CR4_ADDRESS_SPACE_MASK != firmware_cr4 & CR4_ADDRESS_SPACE_MASK {
        return Err(alloc::format!(
            "firmware/TRUEOS paging mode mismatch caller_cr4=0x{caller_cr4:016X} firmware_cr4=0x{firmware_cr4:016X} mask=0x{CR4_ADDRESS_SPACE_MASK:016X}"
        ));
    }

    if !limine_range_covered(boot_services_physical, size_of::<EfiBootServicesThroughCrc32>() as u64)
        || !limine_range_covered(bridge_entry_physical, bridge_entry_bytes as u64)
        || !limine_range_covered(bridge_control_physical, BRIDGE_CONTROL_BYTES as u64)
    {
        return Err(String::from("retained firmware bridge physical range crosses a Limine memory-map hole"));
    }

    let system_table = crate::efi::system_table()
        .ok_or_else(|| String::from("EFI system table unavailable"))?;
    if system_table.boot_services as u64 != boot_services_virtual {
        return Err(alloc::format!(
            "BootServices virtual pointer changed system_table=0x{:016X} context=0x{:016X}",
            system_table.boot_services as u64,
            boot_services_virtual
        ));
    }

    let table_mapping = crate::pci::mmio::map_limine_struct::<EfiBootServicesThroughCrc32>(
        boot_services_physical,
    )
    .map_err(|error| alloc::format!("BootServices inspection map failed: {error:?}"))?;
    let boot_services = unsafe { core::ptr::read_unaligned(table_mapping.as_ptr()) };
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
    if boot_services.calculate_crc32 as u64 != calculate_crc32_virtual {
        return Err(alloc::format!(
            "CalculateCrc32 virtual pointer mismatch table=0x{:016X} context=0x{:016X}",
            boot_services.calculate_crc32 as u64,
            calculate_crc32_virtual
        ));
    }

    let _guard = BRIDGE_LOCK.lock();
    let _bridge_mapping = crate::pci::mmio::map_ram_region_at(
        bridge_entry_virtual,
        bridge_entry_physical,
        bridge_entry_bytes,
        true,
    )
    .map_err(|error| alloc::format!("bridge code map failed: {error:?}"))?;
    let control_mapping = crate::pci::mmio::map_ram_region_at(
        bridge_control_virtual,
        bridge_control_physical,
        BRIDGE_CONTROL_BYTES,
        false,
    )
    .map_err(|error| alloc::format!("bridge control map failed: {error:?}"))?;
    let control = control_mapping.as_ptr().cast::<BridgeControl>();

    unsafe {
        core::ptr::write_bytes(control.cast::<u8>(), 0, BRIDGE_CONTROL_BYTES);
        (*control).firmware_cr3 = firmware_cr3;
        (*control).firmware_stack_top = bridge_stack_top;
        (*control).target = calculate_crc32_virtual;
        (*control).payload_len = PROBE_PAYLOAD.len() as u32;
        core::ptr::copy_nonoverlapping(
            PROBE_PAYLOAD.as_ptr(),
            core::ptr::addr_of_mut!((*control).payload).cast::<u8>(),
            PROBE_PAYLOAD.len(),
        );
        (*control).arg0 = bridge_control_virtual + BRIDGE_PAYLOAD_OFFSET;
        (*control).arg1 = PROBE_PAYLOAD.len() as u64;
        (*control).arg2 = bridge_control_virtual + BRIDGE_CRC_OUTPUT_OFFSET;
        (*control).arg3 = 0;
        (*control).crc_output = 0;
        (*control).result = u64::MAX;
    }

    type FirmwareBridgeEntry = unsafe extern "C" fn(*mut BridgeControl);
    let bridge: FirmwareBridgeEntry = unsafe {
        core::mem::transmute::<usize, FirmwareBridgeEntry>(bridge_entry_virtual as usize)
    };
    unsafe { bridge(control) };

    let firmware_status = unsafe { core::ptr::read_volatile(core::ptr::addr_of!((*control).result)) };
    let firmware_crc32 = unsafe { core::ptr::read_volatile(core::ptr::addr_of!((*control).crc_output)) };

    Ok(ProbeResult {
        capture_flags: capture.capture_flags,
        context_flags,
        firmware_cr3,
        firmware_cr4,
        page_table_pages,
        bridge_entry_virtual,
        bridge_entry_physical,
        bridge_control_virtual,
        bridge_control_physical,
        bridge_stack_base_virtual,
        bridge_stack_bytes,
        boot_services_virtual,
        boot_services_physical,
        calculate_crc32_virtual,
        calculate_crc32_physical,
        firmware_status,
        firmware_crc32,
    })
}

fn firmware_context_capture() -> Result<BridgeCapture, String> {
    let response = crate::limine::trueos_hii_capture_response()
        .ok_or_else(|| String::from("patched Limine TRUEOS capture response absent"))?;
    let payload_len = usize::try_from(response.size)
        .map_err(|_| String::from("Limine capture size does not fit usize"))?;
    if payload_len < size_of::<Trpay1Header>() || payload_len > MAX_PAYLOAD_BYTES {
        return Err(alloc::format!("Limine capture size outside bound: {payload_len}"));
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
            "Limine capture identity mismatch magic_ok={} version={version}",
            magic == TRPAY1_MAGIC
        ));
    }
    if header_bytes < size_of::<Trpay1Header>()
        || section_entry_bytes < size_of::<Trpay1Section>()
        || section_count == 0
        || section_count > MAX_SECTIONS
        || total_bytes != payload_len
    {
        return Err(String::from("Limine capture shape invalid"));
    }
    let directory_end = header_bytes
        .checked_add(
            section_count
                .checked_mul(section_entry_bytes)
                .ok_or_else(|| String::from("Limine section directory overflow"))?,
        )
        .ok_or_else(|| String::from("Limine section directory overflow"))?;
    if directory_end > payload.len() {
        return Err(String::from("Limine section directory truncated"));
    }

    let mut found = None;
    for index in 0..section_count {
        let offset = header_bytes
            .checked_add(index.checked_mul(section_entry_bytes)
                .ok_or_else(|| String::from("Limine section index overflow"))?)
            .ok_or_else(|| String::from("Limine section index overflow"))?;
        let section = read_unaligned::<Trpay1Section>(payload, offset)
            .ok_or_else(|| String::from("Limine section entry truncated"))?;
        if section.kind == SEC_FIRMWARE_CONTEXT {
            if found.is_some() {
                return Err(String::from("multiple retained firmware-context sections"));
            }
            found = Some(section);
        }
    }

    let section = found
        .ok_or_else(|| String::from("retained firmware-context section absent; paired Limine bridge update required"))?;
    let section_offset = section.offset as usize;
    let section_len = section.length as usize;
    let section_end = section_offset
        .checked_add(section_len)
        .ok_or_else(|| String::from("firmware-context section overflow"))?;
    if section_len < size_of::<Trfwc1Context>()
        || section_offset < directory_end
        || section_end > payload.len()
    {
        return Err(String::from("firmware-context section bounds invalid"));
    }
    let bytes = &payload[section_offset..section_end];
    if crc32fast::hash(bytes) != section.crc32 {
        return Err(String::from("firmware-context section CRC mismatch"));
    }

    let context = read_unaligned::<Trfwc1Context>(bytes, 0)
        .ok_or_else(|| String::from("firmware-context body truncated"))?;
    let context_magic = context.magic;
    let context_version = context.version;
    let context_bytes = context.bytes;
    if context_magic != TRFWC1_MAGIC
        || context_version != TRFWC1_VERSION
        || usize::from(context_bytes) < size_of::<Trfwc1Context>()
        || usize::from(context_bytes) > bytes.len()
    {
        return Err(alloc::format!(
            "firmware-context identity/size mismatch magic_ok={} version={} bytes={}",
            context_magic == TRFWC1_MAGIC,
            context_version,
            context_bytes
        ));
    }

    Ok(BridgeCapture {
        capture_flags,
        section_status: section.status,
        context,
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

fn read_cr4() -> u64 {
    let value: u64;
    unsafe {
        core::arch::asm!(
            "mov {}, cr4",
            out(reg) value,
            options(nomem, nostack, preserves_flags),
        );
    }
    value
}

fn failure_stage_name(stage: u32) -> &'static str {
    match stage {
        0 => "none",
        1 => "bridge-allocation",
        2 => "firmware-quiesce",
        3 => "firmware-va-translation",
        4 => "firmware-page-tables",
        5 => "retained-memory-map",
        6 => "boot-services-table-crc",
        _ => "unknown",
    }
}
