#![no_std]
#![no_main]
#![allow(unsafe_op_in_unsafe_fn)]

use core::ffi::c_void;
use core::mem::{align_of, size_of};
use core::ptr::{self, null_mut};

use r_efi::base::{Boolean, Guid, Handle, Status};
use r_efi::hii;
use r_efi::protocols::{device_path, hii_database, loaded_image};
use r_efi::system::{self, BootServices, SystemTable};

const PAGE_BYTES: usize = 4096;
const MAX_HII_PACKAGE_BYTES: usize = 12 * 1024 * 1024;
const MAX_HII_CONFIG_BYTES: usize = 4 * 1024 * 1024;
const MAX_CATALOG_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;
const MAX_DEVICE_PATH_BYTES: usize = 64 * 1024;

const CATALOG_MAGIC: [u8; 8] = *b"TRBIOS1\0";
const CATALOG_VERSION: u16 = 1;
const PAYLOAD_MAGIC: [u8; 8] = *b"TRPAY1\0\0";
const PAYLOAD_VERSION: u16 = 1;
const STATUS_MAGIC: [u8; 8] = *b"TRSTAT1\0";
const STATUS_VERSION: u16 = 1;

const CATALOG_FLAG_HII_PACKAGES: u32 = 1 << 0;
const CATALOG_FLAG_FORMS: u32 = 1 << 1;
const CATALOG_FLAG_STRINGS: u32 = 1 << 2;
const CATALOG_FLAG_CONFIG: u32 = 1 << 3;
const CATALOG_FLAG_PROTOCOLS: u32 = 1 << 4;

const CAPTURE_FLAG_HII_DATABASE: u32 = 1 << 0;
const CAPTURE_FLAG_HII_PACKAGES: u32 = 1 << 1;
const CAPTURE_FLAG_HII_PARSE_VALID: u32 = 1 << 2;
const CAPTURE_FLAG_CONFIG_ROUTING: u32 = 1 << 3;
const CAPTURE_FLAG_CONFIG: u32 = 1 << 4;

const SECTION_KIND_CAPTURE_STATUS: u32 = 1;
const SECTION_KIND_HII_PACKAGE_LISTS: u32 = 2;
const SECTION_KIND_HII_CONFIG_UTF16: u32 = 3;

const SECTION_FLAG_CAPTURED: u32 = 1 << 0;
const SECTION_FLAG_RAW_HII: u32 = 1 << 1;
const SECTION_FLAG_UTF16: u32 = 1 << 2;
const SECTION_FLAG_NUL_TERMINATED: u32 = 1 << 3;

const HII_PACKAGE_TYPE_FORMS: u8 = 0x02;
const HII_PACKAGE_TYPE_STRINGS: u8 = 0x04;

const TRUEOS_BIOS_CATALOG_GUID: Guid = Guid::from_fields(
    0x184d_a5de,
    0xfa77,
    0x4a1f,
    0xb4,
    0x27,
    &[0xd4, 0xdb, 0xfc, 0xe6, 0xd7, 0xf7],
);

const HII_CONFIG_ROUTING_GUID: Guid = Guid::from_fields(
    0x587e_72d7,
    0xcc50,
    0x4f79,
    0x82,
    0x09,
    &[0xca, 0x29, 0x1f, 0xc1, 0xa1, 0x0f],
);

const LIMINE_PATH: [u16; 21] = [
    b'\\' as u16,
    b'E' as u16,
    b'F' as u16,
    b'I' as u16,
    b'\\' as u16,
    b'B' as u16,
    b'O' as u16,
    b'O' as u16,
    b'T' as u16,
    b'\\' as u16,
    b'L' as u16,
    b'I' as u16,
    b'M' as u16,
    b'I' as u16,
    b'N' as u16,
    b'E' as u16,
    b'.' as u16,
    b'E' as u16,
    b'F' as u16,
    b'I' as u16,
    0,
];

#[repr(C)]
#[derive(Clone, Copy)]
struct BiosCatalogHeader {
    magic: [u8; 8],
    version: u16,
    header_bytes: u16,
    flags: u32,
    package_list_count: u32,
    formset_count: u32,
    question_count: u32,
    payload_bytes: u32,
    payload_crc32: u32,
    reserved: u32,
    payload_phys: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct PayloadHeader {
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

#[repr(C)]
#[derive(Clone, Copy)]
struct SectionEntry {
    kind: u32,
    flags: u32,
    offset: u32,
    length: u32,
    crc32: u32,
    reserved: u32,
    status: u64,
}

impl SectionEntry {
    const EMPTY: Self = Self {
        kind: 0,
        flags: 0,
        offset: 0,
        length: 0,
        crc32: 0,
        reserved: 0,
        status: 0,
    };
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CaptureStatusV1 {
    magic: [u8; 8],
    version: u16,
    bytes: u16,
    flags: u32,
    hii_database_locate_status: u64,
    hii_export_query_status: u64,
    hii_export_status: u64,
    hii_parse_status: u64,
    hii_bytes: u32,
    package_lists: u32,
    form_packages: u32,
    string_packages: u32,
    config_routing_locate_status: u64,
    config_export_status: u64,
    config_bytes: u32,
    reserved: u32,
}

impl CaptureStatusV1 {
    fn new() -> Self {
        let not_started = status_raw(Status::NOT_STARTED);
        Self {
            magic: STATUS_MAGIC,
            version: STATUS_VERSION,
            bytes: size_of::<Self>() as u16,
            flags: 0,
            hii_database_locate_status: not_started,
            hii_export_query_status: not_started,
            hii_export_status: not_started,
            hii_parse_status: not_started,
            hii_bytes: 0,
            package_lists: 0,
            form_packages: 0,
            string_packages: 0,
            config_routing_locate_status: not_started,
            config_export_status: not_started,
            config_bytes: 0,
            reserved: 0,
        }
    }
}

#[repr(C)]
struct HiiConfigRoutingProtocol {
    extract_config: usize,
    export_config: extern "efiapi" fn(
        *const HiiConfigRoutingProtocol,
        *mut *mut u16,
    ) -> Status,
    route_config: usize,
    block_to_config: usize,
    config_to_block: usize,
    get_alt_config: usize,
}

struct CaptureBuffers {
    hii: *mut u8,
    hii_len: usize,
    config: *mut u16,
    config_len: usize,
    status: CaptureStatusV1,
}

impl CaptureBuffers {
    fn new() -> Self {
        Self {
            hii: null_mut(),
            hii_len: 0,
            config: null_mut(),
            config_len: 0,
            status: CaptureStatusV1::new(),
        }
    }
}

struct PublishedCatalog {
    physical_base: u64,
    pages: usize,
}

#[panic_handler]
fn panic_handler(_info: &core::panic::PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[no_mangle]
pub extern "efiapi" fn efi_main(image: Handle, st: *mut SystemTable) -> Status {
    if st.is_null() {
        return Status::INVALID_PARAMETER;
    }

    let bs = unsafe { (*st).boot_services };
    if bs.is_null() {
        return Status::NOT_READY;
    }

    print_line(st, "TRUEOS FirmwareScout: capture-only HII handoff");

    let mut capture = CaptureBuffers::new();
    unsafe {
        capture_hii_packages(bs, &mut capture);
        capture_hii_config(bs, &mut capture);
    }

    let published = match unsafe { publish_catalog(bs, &capture) } {
        Ok(published) => {
            print_line(st, "FirmwareScout: TRBIOS1 catalog installed");
            published
        }
        Err(status) => {
            print_status(st, "FirmwareScout: catalog publication failed", status);
            unsafe { release_capture_buffers(bs, &mut capture) };
            return status;
        }
    };

    unsafe { release_capture_buffers(bs, &mut capture) };

    print_line(st, "FirmwareScout: chainloading \\EFI\\BOOT\\LIMINE.EFI");
    let status = unsafe { chainload_limine(image, st, bs) };

    // A successful operating-system loader does not return. Any return means the
    // handoff did not complete, so remove the table and release the reserved pages.
    print_status(st, "FirmwareScout: Limine returned", status);
    unsafe {
        let mut guid = TRUEOS_BIOS_CATALOG_GUID;
        let _ = ((*bs).install_configuration_table)(&mut guid, null_mut());
        let _ = ((*bs).free_pages)(published.physical_base, published.pages);
    }
    status
}

unsafe fn capture_hii_packages(bs: *mut BootServices, capture: &mut CaptureBuffers) {
    let mut guid = hii_database::PROTOCOL_GUID;
    let mut interface: *mut c_void = null_mut();
    let locate = ((*bs).locate_protocol)(&mut guid, null_mut(), &mut interface);
    capture.status.hii_database_locate_status = status_raw(locate);
    if locate.is_error() || interface.is_null() {
        return;
    }
    capture.status.flags |= CAPTURE_FLAG_HII_DATABASE;

    let protocol = interface as *const hii_database::Protocol;
    let mut bytes = 0usize;
    let query = ((*protocol).export_package_lists)(protocol, null_mut(), &mut bytes, null_mut());
    capture.status.hii_export_query_status = status_raw(query);

    if bytes == 0 || bytes > MAX_HII_PACKAGE_BYTES {
        capture.status.hii_export_status = status_raw(Status::BAD_BUFFER_SIZE);
        return;
    }
    if query.is_error()
        && query.as_usize() != Status::BUFFER_TOO_SMALL.as_usize()
        && query.as_usize() != Status::OUT_OF_RESOURCES.as_usize()
    {
        capture.status.hii_export_status = status_raw(query);
        return;
    }

    let mut buffer: *mut c_void = null_mut();
    let allocate = ((*bs).allocate_pool)(system::LOADER_DATA, bytes, &mut buffer);
    if allocate.is_error() || buffer.is_null() {
        capture.status.hii_export_status = status_raw(allocate);
        return;
    }

    let mut exported_bytes = bytes;
    let export = ((*protocol).export_package_lists)(
        protocol,
        null_mut(),
        &mut exported_bytes,
        buffer as *mut hii::PackageListHeader,
    );
    capture.status.hii_export_status = status_raw(export);
    if export.is_error() || exported_bytes == 0 || exported_bytes > bytes {
        let _ = ((*bs).free_pool)(buffer);
        return;
    }

    capture.hii = buffer as *mut u8;
    capture.hii_len = exported_bytes;
    capture.status.hii_bytes = clamp_u32(exported_bytes);
    capture.status.flags |= CAPTURE_FLAG_HII_PACKAGES;

    match summarize_hii_packages(capture.hii, capture.hii_len) {
        Ok(summary) => {
            capture.status.package_lists = summary.package_lists;
            capture.status.form_packages = summary.form_packages;
            capture.status.string_packages = summary.string_packages;
            capture.status.hii_parse_status = status_raw(Status::SUCCESS);
            capture.status.flags |= CAPTURE_FLAG_HII_PARSE_VALID;
        }
        Err(status) => {
            capture.status.hii_parse_status = status_raw(status);
        }
    }
}

unsafe fn capture_hii_config(bs: *mut BootServices, capture: &mut CaptureBuffers) {
    let mut guid = HII_CONFIG_ROUTING_GUID;
    let mut interface: *mut c_void = null_mut();
    let locate = ((*bs).locate_protocol)(&mut guid, null_mut(), &mut interface);
    capture.status.config_routing_locate_status = status_raw(locate);
    if locate.is_error() || interface.is_null() {
        return;
    }
    capture.status.flags |= CAPTURE_FLAG_CONFIG_ROUTING;

    let protocol = interface as *const HiiConfigRoutingProtocol;
    let mut config: *mut u16 = null_mut();
    let export = ((*protocol).export_config)(protocol, &mut config);
    capture.status.config_export_status = status_raw(export);
    if export.is_error() || config.is_null() {
        return;
    }

    let max_units = MAX_HII_CONFIG_BYTES / size_of::<u16>();
    let mut units = 0usize;
    while units < max_units {
        if ptr::read(config.add(units)) == 0 {
            units += 1;
            break;
        }
        units += 1;
    }

    let terminated = units != 0 && ptr::read(config.add(units - 1)) == 0;
    if !terminated {
        capture.status.config_export_status = status_raw(Status::BAD_BUFFER_SIZE);
        let _ = ((*bs).free_pool)(config as *mut c_void);
        return;
    }

    let Some(bytes) = units.checked_mul(size_of::<u16>()) else {
        capture.status.config_export_status = status_raw(Status::BAD_BUFFER_SIZE);
        let _ = ((*bs).free_pool)(config as *mut c_void);
        return;
    };
    capture.config = config;
    capture.config_len = bytes;
    capture.status.config_bytes = clamp_u32(bytes);
    capture.status.flags |= CAPTURE_FLAG_CONFIG;
}

unsafe fn publish_catalog(
    bs: *mut BootServices,
    capture: &CaptureBuffers,
) -> Result<PublishedCatalog, Status> {
    let has_hii = !capture.hii.is_null() && capture.hii_len != 0;
    let has_config = !capture.config.is_null() && capture.config_len != 0;
    let section_count = 1usize + has_hii as usize + has_config as usize;

    let status_crc32 = calculate_crc32(
        bs,
        (&capture.status as *const CaptureStatusV1).cast::<u8>(),
        size_of::<CaptureStatusV1>(),
    )?;
    let hii_crc32 = if has_hii {
        Some(calculate_crc32(bs, capture.hii, capture.hii_len)?)
    } else {
        None
    };
    let config_crc32 = if has_config {
        Some(calculate_crc32(
            bs,
            capture.config.cast::<u8>(),
            capture.config_len,
        )?)
    } else {
        None
    };

    let directory_bytes = size_of::<PayloadHeader>()
        .checked_add(
            section_count
                .checked_mul(size_of::<SectionEntry>())
                .ok_or(Status::BAD_BUFFER_SIZE)?,
        )
        .ok_or(Status::BAD_BUFFER_SIZE)?;
    let status_offset = align_up(directory_bytes, align_of::<CaptureStatusV1>())
        .ok_or(Status::BAD_BUFFER_SIZE)?;
    let mut cursor = status_offset
        .checked_add(size_of::<CaptureStatusV1>())
        .ok_or(Status::BAD_BUFFER_SIZE)?;

    let hii_offset = if has_hii {
        cursor = align_up(cursor, 8).ok_or(Status::BAD_BUFFER_SIZE)?;
        let offset = cursor;
        cursor = cursor
            .checked_add(capture.hii_len)
            .ok_or(Status::BAD_BUFFER_SIZE)?;
        Some(offset)
    } else {
        None
    };

    let config_offset = if has_config {
        cursor = align_up(cursor, align_of::<u16>()).ok_or(Status::BAD_BUFFER_SIZE)?;
        let offset = cursor;
        cursor = cursor
            .checked_add(capture.config_len)
            .ok_or(Status::BAD_BUFFER_SIZE)?;
        Some(offset)
    } else {
        None
    };

    let payload_bytes = cursor;
    if payload_bytes == 0 || payload_bytes > MAX_CATALOG_PAYLOAD_BYTES {
        return Err(Status::BAD_BUFFER_SIZE);
    }

    let payload_offset = align_up(size_of::<BiosCatalogHeader>(), 16)
        .ok_or(Status::BAD_BUFFER_SIZE)?;
    let allocation_bytes = payload_offset
        .checked_add(payload_bytes)
        .ok_or(Status::BAD_BUFFER_SIZE)?;
    let pages = allocation_bytes
        .checked_add(PAGE_BYTES - 1)
        .ok_or(Status::BAD_BUFFER_SIZE)?
        / PAGE_BYTES;
    if pages == 0 {
        return Err(Status::BAD_BUFFER_SIZE);
    }

    let mut physical_base = 0u64;
    let allocate = ((*bs).allocate_pages)(
        system::ALLOCATE_ANY_PAGES,
        system::RESERVED_MEMORY_TYPE,
        pages,
        &mut physical_base,
    );
    if allocate.is_error() {
        return Err(allocate);
    }

    let allocation_ptr = physical_base as *mut u8;
    ptr::write_bytes(allocation_ptr, 0, pages * PAGE_BYTES);
    let payload_ptr = allocation_ptr.add(payload_offset);

    let mut entries = [SectionEntry::EMPTY; 3];
    let mut entry_index = 0usize;

    ptr::copy_nonoverlapping(
        (&capture.status as *const CaptureStatusV1).cast::<u8>(),
        payload_ptr.add(status_offset),
        size_of::<CaptureStatusV1>(),
    );
    entries[entry_index] = SectionEntry {
        kind: SECTION_KIND_CAPTURE_STATUS,
        flags: SECTION_FLAG_CAPTURED,
        offset: clamp_u32(status_offset),
        length: size_of::<CaptureStatusV1>() as u32,
        crc32: status_crc32,
        reserved: 0,
        status: status_raw(Status::SUCCESS),
    };
    entry_index += 1;

    if let Some(offset) = hii_offset {
        ptr::copy_nonoverlapping(capture.hii, payload_ptr.add(offset), capture.hii_len);
        entries[entry_index] = SectionEntry {
            kind: SECTION_KIND_HII_PACKAGE_LISTS,
            flags: SECTION_FLAG_CAPTURED | SECTION_FLAG_RAW_HII,
            offset: clamp_u32(offset),
            length: clamp_u32(capture.hii_len),
            crc32: hii_crc32.unwrap_or(0),
            reserved: 0,
            status: capture.status.hii_export_status,
        };
        entry_index += 1;
    }

    if let Some(offset) = config_offset {
        ptr::copy_nonoverlapping(
            capture.config.cast::<u8>(),
            payload_ptr.add(offset),
            capture.config_len,
        );
        entries[entry_index] = SectionEntry {
            kind: SECTION_KIND_HII_CONFIG_UTF16,
            flags: SECTION_FLAG_CAPTURED
                | SECTION_FLAG_UTF16
                | SECTION_FLAG_NUL_TERMINATED,
            offset: clamp_u32(offset),
            length: clamp_u32(capture.config_len),
            crc32: config_crc32.unwrap_or(0),
            reserved: 0,
            status: capture.status.config_export_status,
        };
        entry_index += 1;
    }

    if entry_index != section_count {
        let _ = ((*bs).free_pages)(physical_base, pages);
        return Err(Status::COMPROMISED_DATA);
    }

    let payload_header = PayloadHeader {
        magic: PAYLOAD_MAGIC,
        version: PAYLOAD_VERSION,
        header_bytes: size_of::<PayloadHeader>() as u16,
        section_entry_bytes: size_of::<SectionEntry>() as u16,
        reserved0: 0,
        section_count: section_count as u32,
        total_bytes: clamp_u32(payload_bytes),
        capture_flags: capture.status.flags,
        reserved1: 0,
    };
    ptr::write_unaligned(payload_ptr.cast::<PayloadHeader>(), payload_header);
    ptr::copy_nonoverlapping(
        entries.as_ptr().cast::<u8>(),
        payload_ptr.add(size_of::<PayloadHeader>()),
        section_count * size_of::<SectionEntry>(),
    );

    let payload_crc32 = match calculate_crc32(bs, payload_ptr, payload_bytes) {
        Ok(crc) => crc,
        Err(status) => {
            let _ = ((*bs).free_pages)(physical_base, pages);
            return Err(status);
        }
    };

    let mut catalog_flags = CATALOG_FLAG_PROTOCOLS;
    if has_hii {
        catalog_flags |= CATALOG_FLAG_HII_PACKAGES;
    }
    if capture.status.form_packages != 0 {
        catalog_flags |= CATALOG_FLAG_FORMS;
    }
    if capture.status.string_packages != 0 {
        catalog_flags |= CATALOG_FLAG_STRINGS;
    }
    if has_config {
        catalog_flags |= CATALOG_FLAG_CONFIG;
    }

    let catalog = BiosCatalogHeader {
        magic: CATALOG_MAGIC,
        version: CATALOG_VERSION,
        header_bytes: size_of::<BiosCatalogHeader>() as u16,
        flags: catalog_flags,
        package_list_count: capture.status.package_lists,
        // Raw form packages are captured, but formsets and questions are not
        // counted until the kernel-side IFR parser exists.
        formset_count: 0,
        question_count: 0,
        payload_bytes: clamp_u32(payload_bytes),
        payload_crc32,
        reserved: 0,
        payload_phys: physical_base + payload_offset as u64,
    };
    ptr::write_unaligned(allocation_ptr.cast::<BiosCatalogHeader>(), catalog);

    let mut guid = TRUEOS_BIOS_CATALOG_GUID;
    let install = ((*bs).install_configuration_table)(&mut guid, allocation_ptr.cast::<c_void>());
    if install.is_error() {
        let _ = ((*bs).free_pages)(physical_base, pages);
        return Err(install);
    }

    Ok(PublishedCatalog {
        physical_base,
        pages,
    })
}

unsafe fn release_capture_buffers(bs: *mut BootServices, capture: &mut CaptureBuffers) {
    if !capture.hii.is_null() {
        let _ = ((*bs).free_pool)(capture.hii.cast::<c_void>());
        capture.hii = null_mut();
        capture.hii_len = 0;
    }
    if !capture.config.is_null() {
        let _ = ((*bs).free_pool)(capture.config.cast::<c_void>());
        capture.config = null_mut();
        capture.config_len = 0;
    }
}

#[derive(Clone, Copy)]
struct HiiSummary {
    package_lists: u32,
    form_packages: u32,
    string_packages: u32,
}

unsafe fn summarize_hii_packages(buffer: *const u8, bytes: usize) -> Result<HiiSummary, Status> {
    const PACKAGE_LIST_HEADER_BYTES: usize = 20;
    const PACKAGE_HEADER_BYTES: usize = 4;

    if buffer.is_null() || bytes < PACKAGE_LIST_HEADER_BYTES {
        return Err(Status::COMPROMISED_DATA);
    }

    let mut summary = HiiSummary {
        package_lists: 0,
        form_packages: 0,
        string_packages: 0,
    };
    let mut list_offset = 0usize;

    while list_offset < bytes {
        let header_end = list_offset
            .checked_add(PACKAGE_LIST_HEADER_BYTES)
            .ok_or(Status::COMPROMISED_DATA)?;
        if header_end > bytes {
            return Err(Status::COMPROMISED_DATA);
        }
        let list_len = read_u32_le(buffer.add(list_offset + 16)) as usize;
        if list_len < PACKAGE_LIST_HEADER_BYTES {
            return Err(Status::COMPROMISED_DATA);
        }
        let list_end = list_offset
            .checked_add(list_len)
            .ok_or(Status::COMPROMISED_DATA)?;
        if list_end > bytes {
            return Err(Status::COMPROMISED_DATA);
        }

        summary.package_lists = summary.package_lists.saturating_add(1);
        let mut package_offset = header_end;
        while package_offset < list_end {
            let package_header_end = package_offset
                .checked_add(PACKAGE_HEADER_BYTES)
                .ok_or(Status::COMPROMISED_DATA)?;
            if package_header_end > list_end {
                return Err(Status::COMPROMISED_DATA);
            }
            let raw = read_u32_le(buffer.add(package_offset));
            let package_len = (raw & 0x00ff_ffff) as usize;
            let package_type = (raw >> 24) as u8;
            if package_len < PACKAGE_HEADER_BYTES {
                return Err(Status::COMPROMISED_DATA);
            }
            let package_end = package_offset
                .checked_add(package_len)
                .ok_or(Status::COMPROMISED_DATA)?;
            if package_end > list_end {
                return Err(Status::COMPROMISED_DATA);
            }
            if package_type == HII_PACKAGE_TYPE_FORMS {
                summary.form_packages = summary.form_packages.saturating_add(1);
            } else if package_type == HII_PACKAGE_TYPE_STRINGS {
                summary.string_packages = summary.string_packages.saturating_add(1);
            }
            package_offset = package_end;
        }
        if package_offset != list_end {
            return Err(Status::COMPROMISED_DATA);
        }
        list_offset = list_end;
    }

    if list_offset != bytes || summary.package_lists == 0 {
        return Err(Status::COMPROMISED_DATA);
    }
    Ok(summary)
}

unsafe fn chainload_limine(
    image: Handle,
    st: *mut SystemTable,
    bs: *mut BootServices,
) -> Status {
    let mut loaded_guid = loaded_image::PROTOCOL_GUID;
    let mut loaded_raw: *mut c_void = null_mut();
    let loaded_status = ((*bs).handle_protocol)(image, &mut loaded_guid, &mut loaded_raw);
    if loaded_status.is_error() || loaded_raw.is_null() {
        return loaded_status;
    }
    let loaded = loaded_raw as *mut loaded_image::Protocol;

    let mut device_path_guid = device_path::PROTOCOL_GUID;
    let mut base_path_raw: *mut c_void = null_mut();
    let path_status = ((*bs).handle_protocol)(
        (*loaded).device_handle,
        &mut device_path_guid,
        &mut base_path_raw,
    );

    let path_allocation: *mut c_void;
    let device_path = if !path_status.is_error() && !base_path_raw.is_null() {
        match build_limine_device_path(bs, base_path_raw as *const device_path::Protocol) {
            Ok((path, allocation)) => {
                path_allocation = allocation;
                path
            }
            Err(_) => match build_relative_limine_path(bs) {
                Ok((path, allocation)) => {
                    path_allocation = allocation;
                    path
                }
                Err(status) => return status,
            },
        }
    } else {
        match build_relative_limine_path(bs) {
            Ok((path, allocation)) => {
                path_allocation = allocation;
                path
            }
            Err(status) => return status,
        }
    };

    let mut child: Handle = null_mut();
    let load = ((*bs).load_image)(
        Boolean::FALSE,
        image,
        device_path,
        null_mut(),
        0,
        &mut child,
    );
    if !path_allocation.is_null() {
        let _ = ((*bs).free_pool)(path_allocation);
    }
    if load.is_error() || child.is_null() {
        print_status(st, "FirmwareScout: LoadImage LIMINE.EFI failed", load);
        return load;
    }

    let mut exit_data_size = 0usize;
    let mut exit_data: *mut u16 = null_mut();
    let start = ((*bs).start_image)(child, &mut exit_data_size, &mut exit_data);
    if !exit_data.is_null() {
        let _ = ((*bs).free_pool)(exit_data.cast::<c_void>());
    }
    start
}

unsafe fn build_limine_device_path(
    bs: *mut BootServices,
    base: *const device_path::Protocol,
) -> Result<(*mut device_path::Protocol, *mut c_void), Status> {
    let prefix_bytes = device_path_prefix_bytes(base)?;
    build_path_with_prefix(bs, base.cast::<u8>(), prefix_bytes)
}

unsafe fn build_relative_limine_path(
    bs: *mut BootServices,
) -> Result<(*mut device_path::Protocol, *mut c_void), Status> {
    build_path_with_prefix(bs, core::ptr::null(), 0)
}

unsafe fn build_path_with_prefix(
    bs: *mut BootServices,
    prefix: *const u8,
    prefix_bytes: usize,
) -> Result<(*mut device_path::Protocol, *mut c_void), Status> {
    let file_node_bytes = size_of::<device_path::Protocol>()
        .checked_add(LIMINE_PATH.len() * size_of::<u16>())
        .ok_or(Status::BAD_BUFFER_SIZE)?;
    if file_node_bytes > u16::MAX as usize {
        return Err(Status::BAD_BUFFER_SIZE);
    }
    let total_bytes = prefix_bytes
        .checked_add(file_node_bytes)
        .and_then(|value| value.checked_add(size_of::<device_path::Protocol>()))
        .ok_or(Status::BAD_BUFFER_SIZE)?;
    if total_bytes > MAX_DEVICE_PATH_BYTES {
        return Err(Status::BAD_BUFFER_SIZE);
    }

    let mut allocation: *mut c_void = null_mut();
    let status = ((*bs).allocate_pool)(system::LOADER_DATA, total_bytes, &mut allocation);
    if status.is_error() || allocation.is_null() {
        return Err(status);
    }
    let bytes = allocation.cast::<u8>();
    ptr::write_bytes(bytes, 0, total_bytes);
    if prefix_bytes != 0 {
        if prefix.is_null() {
            let _ = ((*bs).free_pool)(allocation);
            return Err(Status::INVALID_PARAMETER);
        }
        ptr::copy_nonoverlapping(prefix, bytes, prefix_bytes);
    }

    let file = bytes.add(prefix_bytes);
    ptr::write(file, device_path::TYPE_MEDIA);
    ptr::write(file.add(1), device_path::Media::SUBTYPE_FILE_PATH);
    ptr::write(file.add(2), file_node_bytes as u8);
    ptr::write(file.add(3), (file_node_bytes >> 8) as u8);
    ptr::copy_nonoverlapping(
        LIMINE_PATH.as_ptr().cast::<u8>(),
        file.add(size_of::<device_path::Protocol>()),
        LIMINE_PATH.len() * size_of::<u16>(),
    );

    let end = file.add(file_node_bytes);
    ptr::write(end, device_path::TYPE_END);
    ptr::write(end.add(1), device_path::End::SUBTYPE_ENTIRE);
    ptr::write(end.add(2), size_of::<device_path::Protocol>() as u8);
    ptr::write(end.add(3), 0);

    Ok((bytes.cast::<device_path::Protocol>(), allocation))
}

unsafe fn device_path_prefix_bytes(base: *const device_path::Protocol) -> Result<usize, Status> {
    if base.is_null() {
        return Err(Status::INVALID_PARAMETER);
    }
    let mut offset = 0usize;
    while offset < MAX_DEVICE_PATH_BYTES {
        let node = base.cast::<u8>().add(offset);
        let node_type = ptr::read(node);
        let node_subtype = ptr::read(node.add(1));
        let node_bytes = (ptr::read(node.add(2)) as usize)
            | ((ptr::read(node.add(3)) as usize) << 8);
        if node_bytes < size_of::<device_path::Protocol>() {
            return Err(Status::COMPROMISED_DATA);
        }
        if node_type == device_path::TYPE_END
            && node_subtype == device_path::End::SUBTYPE_ENTIRE
        {
            return Ok(offset);
        }
        offset = offset
            .checked_add(node_bytes)
            .ok_or(Status::COMPROMISED_DATA)?;
    }
    Err(Status::COMPROMISED_DATA)
}

unsafe fn calculate_crc32(
    bs: *mut BootServices,
    bytes: *const u8,
    len: usize,
) -> Result<u32, Status> {
    if bytes.is_null() || len == 0 {
        return Err(Status::INVALID_PARAMETER);
    }
    let mut crc = 0u32;
    let status = ((*bs).calculate_crc32)(bytes as *mut c_void, len, &mut crc);
    if status.is_error() {
        Err(status)
    } else {
        Ok(crc)
    }
}

unsafe fn read_u32_le(ptr: *const u8) -> u32 {
    (ptr::read(ptr) as u32)
        | ((ptr::read(ptr.add(1)) as u32) << 8)
        | ((ptr::read(ptr.add(2)) as u32) << 16)
        | ((ptr::read(ptr.add(3)) as u32) << 24)
}

fn align_up(value: usize, alignment: usize) -> Option<usize> {
    if alignment == 0 || !alignment.is_power_of_two() {
        return None;
    }
    value
        .checked_add(alignment - 1)
        .map(|rounded| rounded & !(alignment - 1))
}

fn clamp_u32(value: usize) -> u32 {
    if value > u32::MAX as usize {
        u32::MAX
    } else {
        value as u32
    }
}

fn status_raw(status: Status) -> u64 {
    status.as_usize() as u64
}

fn print_status(st: *mut SystemTable, prefix: &str, status: Status) {
    print_ascii(st, prefix);
    print_ascii(st, " status=0x");
    print_hex_usize(st, status.as_usize());
    print_ascii(st, "\r\n");
}

fn print_line(st: *mut SystemTable, text: &str) {
    print_ascii(st, text);
    print_ascii(st, "\r\n");
}

fn print_hex_usize(st: *mut SystemTable, value: usize) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut chars = [0u8; size_of::<usize>() * 2];
    let digits = chars.len();
    let mut index = 0usize;
    while index < digits {
        let shift = (digits - 1 - index) * 4;
        chars[index] = HEX[(value >> shift) & 0x0f];
        index += 1;
    }
    let text = unsafe { core::str::from_utf8_unchecked(&chars) };
    print_ascii(st, text);
}

fn print_ascii(st: *mut SystemTable, text: &str) {
    if st.is_null() {
        return;
    }
    let console = unsafe { (*st).con_out };
    if console.is_null() {
        return;
    }

    let mut utf16 = [0u16; 96];
    let mut used = 0usize;
    for byte in text.bytes() {
        if used + 1 >= utf16.len() {
            utf16[used] = 0;
            unsafe {
                let _ = ((*console).output_string)(console, utf16.as_mut_ptr());
            }
            used = 0;
        }
        utf16[used] = if byte.is_ascii() { byte as u16 } else { b'?' as u16 };
        used += 1;
    }
    if used != 0 {
        utf16[used] = 0;
        unsafe {
            let _ = ((*console).output_string)(console, utf16.as_mut_ptr());
        }
    }
}
