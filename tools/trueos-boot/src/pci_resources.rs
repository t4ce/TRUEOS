//! Preserve firmware-owned LPSS I2C BAR assignments before ExitBootServices.
//!
//! No address allocator or BAR sizing probe runs here. GetBarAttributes is the
//! firmware's resource record, including assignments whose BAR programming was
//! deferred. The kernel may restore only these device-specific reservations.

use alloc::{boxed::Box, vec, vec::Vec};
use core::{ffi::c_void, ptr};
use r_efi::{
    base::{Boolean, Guid, Handle, Status},
    protocols::pci_io,
    system::{self, BootServices},
};

use crate::{firmware_bar_wire as wire, handoff::Blob};

const MAX_HANDLES: usize = 1024;
const QWORD_RESOURCE_BYTES: usize = 48;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Location {
    pub segment: u16,
    pub bus: u8,
    pub slot: u8,
    pub function: u8,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ConfigSnapshot {
    pub identity: u32,
    pub class: u32,
    pub command: u16,
    pub bar0: u32,
    pub bar0_high: u32,
    pub pmcsr: Option<u16>,
}

impl ConfigSnapshot {
    fn from_config(config: &[u32; 64]) -> Self {
        Self {
            identity: config[0],
            class: config[2],
            command: config[1] as u16,
            bar0: config[4],
            bar0_high: config[5],
            pmcsr: pmcsr(config),
        }
    }

    fn supported(self) -> bool {
        self.identity as u16 == 0x8086
            && (self.identity >> 16) as u16 == 0xa0e8
            && self.class >> 16 == 0x0c80
    }

    fn bar_base(self) -> Option<u64> {
        if self.bar0 == u32::MAX || self.bar0 & 1 != 0 {
            return None;
        }
        let low = u64::from(self.bar0 & !0xf);
        let base = match (self.bar0 >> 1) & 3 {
            0 => low,
            2 => (u64::from(self.bar0_high) << 32) | low,
            _ => return None,
        };
        (base != 0 && base & 0xfff == 0).then_some(base)
    }
}

#[derive(Debug)]
pub(crate) enum Event {
    Handles {
        count: usize,
        status: Status,
    },
    Snapshot {
        phase: &'static str,
        location: Location,
        config: ConfigSnapshot,
    },
    Resource {
        phase: &'static str,
        location: Location,
        status: Status,
        result: Result<(u64, u64), &'static str>,
    },
    Connect {
        location: Location,
        status: Status,
    },
    Rejected {
        location: Option<Location>,
        reason: &'static str,
    },
    FreeFailed(Status),
    Captured {
        count: usize,
    },
}

#[derive(Clone, Copy)]
struct Services {
    locate_handles: system::BootLocateHandleBuffer,
    handle_protocol: system::BootHandleProtocol,
    connect_controller: system::BootConnectController,
    free_pool: system::BootFreePool,
}

/// Boot services and returned resource/handle buffers are valid only before
/// ExitBootServices. Keep the encoded bytes alive as an ordinary boot module;
/// Handoff::prepare protects its physical range from kernel allocation.
pub(crate) unsafe fn capture(
    bs: *mut BootServices,
    mut log: impl FnMut(Event),
) -> Option<Blob<'static>> {
    let services = Services {
        locate_handles: (*bs).locate_handle_buffer,
        handle_protocol: (*bs).handle_protocol,
        connect_controller: (*bs).connect_controller,
        free_pool: (*bs).free_pool,
    };
    let records = collect(services, &mut log);
    let bytes = match encode(&records) {
        Ok(bytes) => bytes,
        Err(reason) => {
            log(Event::Rejected {
                location: None,
                reason,
            });
            return None;
        }
    };
    let bytes = Box::leak(bytes.into_boxed_slice());
    log(Event::Captured {
        count: records.len(),
    });
    Some(Blob {
        phys: bytes.as_ptr() as u64,
        len: bytes.len(),
        path: "/trueos-pci-bars.v1",
        cmdline: "trueos.pci.bar-reservations.v1",
    })
}

fn encode(records: &[wire::Reservation]) -> Result<Vec<u8>, &'static str> {
    if records.len() > wire::MAX_RECORDS {
        return Err("too many firmware BAR reservations");
    }
    let mut bytes = vec![0; wire::HEADER_BYTES + records.len() * wire::RECORD_BYTES];
    wire::encode_header(&mut bytes[..wire::HEADER_BYTES], records.len())
        .map_err(|_| "firmware BAR module header could not be encoded")?;
    for (record, output) in records
        .iter()
        .zip(bytes[wire::HEADER_BYTES..].chunks_exact_mut(wire::RECORD_BYTES))
    {
        record
            .encode(output)
            .map_err(|_| "firmware BAR reservation could not be encoded")?;
    }
    Ok(bytes)
}

unsafe fn collect(services: Services, log: &mut impl FnMut(Event)) -> Vec<wire::Reservation> {
    let mut guid = pci_io::PROTOCOL_GUID;
    let mut count = 0;
    let mut raw_handles: *mut Handle = ptr::null_mut();
    let status = (services.locate_handles)(
        system::BY_PROTOCOL,
        &mut guid,
        ptr::null_mut(),
        &mut count,
        &mut raw_handles,
    );
    log(Event::Handles { count, status });
    let handles = if !status.is_error() && !raw_handles.is_null() && count <= MAX_HANDLES {
        core::slice::from_raw_parts(raw_handles, count).to_vec()
    } else {
        Vec::new()
    };
    free(raw_handles.cast(), services, log);
    let mut records: Vec<wire::Reservation> = Vec::new();
    let mut seen = Vec::new();
    let mut ambiguous = false;
    for handle in handles {
        let Some(io) = io_on(handle, services) else {
            continue;
        };
        let Some(location) = location(io) else {
            continue;
        };
        let Some(before) = snapshot(io) else {
            continue;
        };
        if location.segment != 0 || !before.supported() {
            continue;
        }
        if seen.contains(&location) {
            // Never let aliases with conflicting firmware records silently
            // choose an address by handle-enumeration order.
            ambiguous = true;
            log(Event::Rejected {
                location: Some(location),
                reason: "duplicate LPSS PCI handles",
            });
            continue;
        }
        seen.push(location);
        log(Event::Snapshot {
            phase: "before-connect",
            location,
            config: before,
        });
        let mut assignment = resource(io, services, location, "before-connect", log);
        let mut current = before;
        if before.bar_base().is_none() {
            let status = (services.connect_controller)(
                handle,
                ptr::null_mut(),
                ptr::null_mut(),
                Boolean::FALSE,
            );
            log(Event::Connect { location, status });
            let Some(io) = io_on(handle, services) else {
                log(Event::Rejected {
                    location: Some(location),
                    reason: "PCI protocol unavailable after connect",
                });
                continue;
            };
            let Some(after) = snapshot(io) else {
                log(Event::Rejected {
                    location: Some(location),
                    reason: "PCI config unavailable after connect",
                });
                continue;
            };
            log(Event::Snapshot {
                phase: "after-connect",
                location,
                config: after,
            });
            if after.identity != before.identity || !after.supported() {
                log(Event::Rejected {
                    location: Some(location),
                    reason: "PCI identity changed during connect",
                });
                continue;
            }
            current = after;
            assignment = resource(io, services, location, "after-connect", log);
        }
        let Ok((base, length)) = assignment else {
            continue;
        };
        if !matches!((current.bar0 >> 1) & 3, 0 | 2)
            || current.bar0 & 1 != 0
            || current.bar0 == u32::MAX
        {
            log(Event::Rejected {
                location: Some(location),
                reason: "BAR0 is not a memory BAR",
            });
            continue;
        }
        if (current.bar0 >> 1) & 3 == 0 && base + length > 0x1_0000_0000 {
            log(Event::Rejected {
                location: Some(location),
                reason: "firmware resource exceeds 32-bit BAR0",
            });
            continue;
        }
        if current
            .bar_base()
            .is_some_and(|programmed| programmed != base)
        {
            log(Event::Rejected {
                location: Some(location),
                reason: "firmware resource conflicts with programmed BAR0",
            });
            continue;
        }
        if records.len() == wire::MAX_RECORDS {
            log(Event::Rejected {
                location: Some(location),
                reason: "firmware BAR record capacity exceeded",
            });
            break;
        }
        records.push(wire::Reservation {
            segment: location.segment,
            bus: location.bus,
            slot: location.slot,
            function: location.function,
            bar_index: 0,
            vendor_id: before.identity as u16,
            device_id: (before.identity >> 16) as u16,
            base,
            length,
        });
    }
    if ambiguous {
        records.clear();
    }
    records
}

unsafe fn io_on(handle: Handle, services: Services) -> Option<*mut pci_io::Protocol> {
    let mut guid: Guid = pci_io::PROTOCOL_GUID;
    let mut raw = ptr::null_mut();
    let status = (services.handle_protocol)(handle, &mut guid, &mut raw);
    (!status.is_error() && !raw.is_null()).then_some(raw.cast())
}

unsafe fn location(io: *mut pci_io::Protocol) -> Option<Location> {
    let (mut segment, mut bus, mut slot, mut function) = (0, 0, 0, 0);
    if ((*io).get_location)(io, &mut segment, &mut bus, &mut slot, &mut function).is_error()
        || segment > u16::MAX as usize
        || bus > u8::MAX as usize
        || slot >= 32
        || function >= 8
    {
        return None;
    }
    Some(Location {
        segment: segment as u16,
        bus: bus as u8,
        slot: slot as u8,
        function: function as u8,
    })
}

unsafe fn snapshot(io: *mut pci_io::Protocol) -> Option<ConfigSnapshot> {
    let mut config = [0; 64];
    let status = ((*io).pci.read)(
        io,
        pci_io::WIDTH_UINT32,
        0,
        config.len(),
        config.as_mut_ptr().cast(),
    );
    (!status.is_error()).then(|| ConfigSnapshot::from_config(&config))
}

fn pmcsr(config: &[u32; 64]) -> Option<u16> {
    if config[1] & (1 << 20) == 0 {
        return None;
    }
    let byte = |offset: usize| ((config[offset / 4] >> ((offset % 4) * 8)) & 0xff) as u8;
    let mut capability = byte(0x34) & !3;
    let mut seen = [false; 64];
    for _ in 0..48 {
        let offset = usize::from(capability);
        if !(0x40..=0xf8).contains(&offset) || seen[offset / 4] {
            return None;
        }
        seen[offset / 4] = true;
        if byte(offset) == 1 {
            return Some(u16::from_le_bytes([byte(offset + 4), byte(offset + 5)]));
        }
        capability = byte(offset + 1) & !3;
    }
    None
}

unsafe fn resource(
    io: *mut pci_io::Protocol,
    services: Services,
    location: Location,
    phase: &'static str,
    log: &mut impl FnMut(Event),
) -> Result<(u64, u64), &'static str> {
    let mut raw = ptr::null_mut();
    let status = ((*io).get_bar_attributes)(io, 0, ptr::null_mut(), &mut raw);
    let result = if status.is_error() || raw.is_null() {
        Err("firmware BAR resource unavailable")
    } else {
        let bytes = raw.cast::<u8>();
        // This API returns an ACPI descriptor buffer ending in an EndTag. Only
        // its standard single-QWord BAR shape is supported. Inspect the header
        // before copying the complete object promised by that shape.
        if bytes.read() != 0x8a || [bytes.add(1).read(), bytes.add(2).read()] != [43, 0] {
            Err("firmware BAR descriptor shape unsupported")
        } else {
            let mut copy = [0; QWORD_RESOURCE_BYTES];
            ptr::copy_nonoverlapping(bytes, copy.as_mut_ptr(), copy.len());
            parse_resource(&copy)
        }
    };
    free(raw, services, log);
    log(Event::Resource {
        phase,
        location,
        status,
        result,
    });
    result
}

unsafe fn free(raw: *mut c_void, services: Services, log: &mut impl FnMut(Event)) {
    if !raw.is_null() {
        let status = (services.free_pool)(raw);
        if status.is_error() {
            log(Event::FreeFailed(status));
        }
    }
}

fn parse_resource(bytes: &[u8]) -> Result<(u64, u64), &'static str> {
    if bytes.len() != QWORD_RESOURCE_BYTES
        || bytes[..3] != [0x8a, 43, 0]
        || bytes[3] != 0
        || bytes[46] != 0x79
    {
        return Err("firmware BAR is not a bounded QWord memory resource");
    }
    if bytes[47] != 0 && bytes.iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte)) != 0 {
        return Err("firmware BAR resource checksum invalid");
    }
    let integer = |offset| u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap());
    let width = integer(6);
    let base = integer(14);
    // GetBarAttributes uses AddrRangeMax for the BAR's alignment mask, unlike
    // a root bridge's ACPI resource producer descriptor.
    let alignment_mask = integer(22);
    let translation = integer(30);
    let length = integer(38);
    let end = base
        .checked_add(length)
        .ok_or("firmware BAR range overflows")?;
    if !matches!(width, 32 | 64)
        || translation != 0
        || base == 0
        || base & 0xfff != 0
        || length < 4096
        || !length.is_power_of_two()
        || base & (length - 1) != 0
        || (width == 32 && end > 0x1_0000_0000)
    {
        return Err("firmware BAR memory range invalid");
    }
    if alignment_mask != 0
        && (!alignment_mask
            .checked_add(1)
            .is_some_and(u64::is_power_of_two)
            || base & alignment_mask != 0)
    {
        return Err("firmware BAR alignment invalid");
    }
    Ok((base, length))
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::cell::RefCell;
    use std::thread_local;

    fn descriptor(base: u64, length: u64) -> [u8; 48] {
        let mut bytes = [0; 48];
        bytes[..3].copy_from_slice(&[0x8a, 43, 0]);
        for (offset, value) in [(6, 64u64), (14, base), (22, length - 1), (38, length)] {
            bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        bytes[46] = 0x79;
        bytes
    }

    #[test]
    fn firmware_bar_descriptor_uses_alignment_not_an_address_maximum() {
        let bytes = descriptor(0x4017_0000_00, 4096);
        assert_eq!(parse_resource(&bytes), Ok((0x4017_0000_00, 4096)));
        assert_eq!(
            parse_resource(&descriptor(0x8000_0000, 8192)),
            Ok((0x8000_0000, 8192))
        );
    }

    #[test]
    fn malformed_or_translated_bar_resources_are_rejected() {
        let good = descriptor(0x8000_0000, 4096);
        for offset in [0, 1, 2, 3, 30, 46] {
            let mut bad = good;
            bad[offset] ^= 1;
            assert!(parse_resource(&bad).is_err());
        }
        for length in 0..good.len() {
            assert!(parse_resource(&good[..length]).is_err());
        }
        for (base, length) in [
            (0, 4096),
            (0x8000_0001, 4096),
            (0x8000_0000, 2048),
            (0x8000_0000, 12288),
            (u64::MAX & !0xfff, 4096),
        ] {
            assert!(parse_resource(&descriptor(base, length)).is_err());
        }
    }

    #[test]
    fn lpss_admission_rejects_unrelated_devices_before_connecting() {
        let mut config = [0; 64];
        config[0] = 0xa0e8_8086;
        config[2] = 0x0c80_0000;
        assert!(ConfigSnapshot::from_config(&config).supported());
        config[0] = 0xa0ed_8086; // USB controller on the same laptop.
        assert!(!ConfigSnapshot::from_config(&config).supported());
        config[0] = 0xa0e9_8086;
        assert!(!ConfigSnapshot::from_config(&config).supported());
        config[0] = 0xa0e8_8086;
        config[2] = 0x0c03_0000;
        assert!(!ConfigSnapshot::from_config(&config).supported());
    }

    #[test]
    fn power_snapshot_follows_capabilities_and_rejects_cycles() {
        let mut config = [0; 64];
        config[1] = 1 << 20;
        config[0x34 / 4] = 0x80;
        config[0x80 / 4] = 0x0000_a005;
        config[0xa0 / 4] = 1;
        config[0xa4 / 4] = 0x8103;
        assert_eq!(pmcsr(&config), Some(0x8103));
        config[0xa0 / 4] = 0x0000_8005;
        assert_eq!(pmcsr(&config), None);
        config[0x34 / 4] = 0xfc;
        assert_eq!(pmcsr(&config), None);
    }

    // All firmware callbacks are mocked. Any path trying to access a device's
    // MMIO, DMA or PCI write operation gets UNSUPPORTED instead.
    #[repr(C)]
    struct Mock {
        protocol: pci_io::Protocol,
        config: [u32; 64],
        location: Location,
        descriptor: [u8; 48],
        resource_status: Status,
        change_identity_on_connect: bool,
    }

    #[derive(Default)]
    struct Firmware {
        devices: Vec<*mut Mock>,
        handles: Option<(*mut Handle, usize)>,
        resources: Vec<*mut [u8; 48]>,
        connects: Vec<Location>,
        resource_queries: usize,
        freed: usize,
    }

    thread_local! { static FIRMWARE: RefCell<Firmware> = RefCell::new(Firmware::default()); }

    extern "efiapi" fn poll(
        _: *mut pci_io::Protocol,
        _: u32,
        _: u8,
        _: u64,
        _: u64,
        _: u64,
        _: u64,
        _: *mut u64,
    ) -> Status {
        Status::UNSUPPORTED
    }
    extern "efiapi" fn memory(
        _: *mut pci_io::Protocol,
        _: u32,
        _: u8,
        _: u64,
        _: usize,
        _: *mut c_void,
    ) -> Status {
        Status::UNSUPPORTED
    }
    extern "efiapi" fn config_write(
        _: *mut pci_io::Protocol,
        _: u32,
        _: u32,
        _: usize,
        _: *mut c_void,
    ) -> Status {
        Status::UNSUPPORTED
    }
    extern "efiapi" fn copy_memory(
        _: *mut pci_io::Protocol,
        _: u32,
        _: u8,
        _: u64,
        _: u8,
        _: u64,
        _: usize,
    ) -> Status {
        Status::UNSUPPORTED
    }
    extern "efiapi" fn map(
        _: *mut pci_io::Protocol,
        _: u32,
        _: *mut c_void,
        _: *mut usize,
        _: *mut u64,
        _: *mut *mut c_void,
    ) -> Status {
        Status::UNSUPPORTED
    }
    extern "efiapi" fn unmap(_: *mut pci_io::Protocol, _: *mut c_void) -> Status {
        Status::UNSUPPORTED
    }
    extern "efiapi" fn allocate(
        _: *mut pci_io::Protocol,
        _: u32,
        _: u32,
        _: usize,
        _: *mut *mut c_void,
        _: u64,
    ) -> Status {
        Status::UNSUPPORTED
    }
    extern "efiapi" fn free_buffer(_: *mut pci_io::Protocol, _: usize, _: *mut c_void) -> Status {
        Status::UNSUPPORTED
    }
    extern "efiapi" fn flush(_: *mut pci_io::Protocol) -> Status {
        Status::UNSUPPORTED
    }
    extern "efiapi" fn attributes(_: *mut pci_io::Protocol, _: u32, _: u64, _: *mut u64) -> Status {
        Status::UNSUPPORTED
    }
    extern "efiapi" fn set_attributes(
        _: *mut pci_io::Protocol,
        _: u64,
        _: u8,
        _: *mut u64,
        _: *mut u64,
    ) -> Status {
        Status::UNSUPPORTED
    }

    extern "efiapi" fn read_config(
        io: *mut pci_io::Protocol,
        width: u32,
        offset: u32,
        count: usize,
        output: *mut c_void,
    ) -> Status {
        assert_eq!((width, offset, count), (pci_io::WIDTH_UINT32, 0, 64));
        unsafe {
            ptr::copy_nonoverlapping((*io.cast::<Mock>()).config.as_ptr(), output.cast(), count);
        }
        Status::SUCCESS
    }

    extern "efiapi" fn get_location(
        io: *mut pci_io::Protocol,
        segment: *mut usize,
        bus: *mut usize,
        slot: *mut usize,
        function: *mut usize,
    ) -> Status {
        unsafe {
            let location = (*io.cast::<Mock>()).location;
            *segment = location.segment.into();
            *bus = location.bus.into();
            *slot = location.slot.into();
            *function = location.function.into();
        }
        Status::SUCCESS
    }

    extern "efiapi" fn get_bar(
        io: *mut pci_io::Protocol,
        index: u8,
        _: *mut u64,
        resources: *mut *mut c_void,
    ) -> Status {
        assert_eq!(index, 0);
        let mock = unsafe { &*io.cast::<Mock>() };
        let allocation = Box::into_raw(Box::new(mock.descriptor));
        FIRMWARE.with(|firmware| {
            let mut firmware = firmware.borrow_mut();
            firmware.resources.push(allocation);
            firmware.resource_queries += 1;
        });
        unsafe {
            *resources = allocation.cast();
        }
        mock.resource_status
    }

    extern "efiapi" fn handles(
        search: u32,
        guid: *mut Guid,
        _: *mut c_void,
        count: *mut usize,
        output: *mut *mut Handle,
    ) -> Status {
        assert_eq!(search, system::BY_PROTOCOL);
        assert_eq!(unsafe { *guid }, pci_io::PROTOCOL_GUID);
        FIRMWARE.with(|firmware| {
            let mut firmware = firmware.borrow_mut();
            let devices: Vec<Handle> = firmware
                .devices
                .iter()
                .map(|device| device.cast())
                .collect();
            let length = devices.len();
            let allocation = Box::into_raw(devices.into_boxed_slice()).cast::<Handle>();
            firmware.handles = Some((allocation, length));
            unsafe {
                *count = length;
                *output = allocation;
            }
        });
        Status::SUCCESS
    }

    extern "efiapi" fn protocol(
        handle: Handle,
        guid: *mut Guid,
        output: *mut *mut c_void,
    ) -> Status {
        assert_eq!(unsafe { *guid }, pci_io::PROTOCOL_GUID);
        unsafe {
            *output = handle;
        }
        Status::SUCCESS
    }

    extern "efiapi" fn connect(
        handle: Handle,
        drivers: *mut Handle,
        path: *mut r_efi::protocols::device_path::Protocol,
        recursive: Boolean,
    ) -> Status {
        assert!(drivers.is_null() && path.is_null());
        assert_eq!(recursive, Boolean::FALSE);
        let mock = unsafe { &mut *handle.cast::<Mock>() };
        FIRMWARE.with(|firmware| firmware.borrow_mut().connects.push(mock.location));
        if mock.change_identity_on_connect {
            mock.config[0] ^= 1;
        }
        Status::NOT_FOUND
    }

    extern "efiapi" fn free_pool(allocation: *mut c_void) -> Status {
        FIRMWARE.with(|firmware| {
            let mut firmware = firmware.borrow_mut();
            if let Some((handles, length)) = firmware
                .handles
                .filter(|(handles, _)| handles.cast() == allocation)
            {
                unsafe {
                    drop(Box::from_raw(ptr::slice_from_raw_parts_mut(
                        handles, length,
                    )));
                }
                firmware.handles = None;
            } else {
                let index = firmware
                    .resources
                    .iter()
                    .position(|resource| resource.cast() == allocation)
                    .unwrap();
                let resource = firmware.resources.remove(index);
                unsafe {
                    drop(Box::from_raw(resource));
                }
            }
            firmware.freed += 1;
        });
        Status::SUCCESS
    }

    fn mock() -> Box<Mock> {
        let mut config = [0; 64];
        config[0] = 0xa0e8_8086;
        config[2] = 0x0c80_0000;
        config[4] = 4;
        Box::new(Mock {
            protocol: pci_io::Protocol {
                poll_mem: poll,
                poll_io: poll,
                mem: pci_io::Access {
                    read: memory,
                    write: memory,
                },
                io: pci_io::Access {
                    read: memory,
                    write: memory,
                },
                pci: pci_io::ConfigAccess {
                    read: read_config,
                    write: config_write,
                },
                copy_mem: copy_memory,
                map,
                unmap,
                allocate_buffer: allocate,
                free_buffer,
                flush,
                get_location,
                attributes,
                get_bar_attributes: get_bar,
                set_bar_attributes: set_attributes,
                rom_size: 0,
                rom_image: ptr::null_mut(),
            },
            config,
            location: Location {
                segment: 0,
                bus: 0,
                slot: 21,
                function: 0,
            },
            descriptor: descriptor(0x4017_0000_00, 4096),
            resource_status: Status::SUCCESS,
            change_identity_on_connect: false,
        })
    }

    fn collect_mock(devices: &mut [Box<Mock>]) -> Vec<wire::Reservation> {
        FIRMWARE.with(|firmware| {
            *firmware.borrow_mut() = Firmware {
                devices: devices
                    .iter_mut()
                    .map(|device| ptr::from_mut(&mut **device))
                    .collect(),
                ..Default::default()
            }
        });
        unsafe {
            collect(
                Services {
                    locate_handles: handles,
                    handle_protocol: protocol,
                    connect_controller: connect,
                    free_pool,
                },
                &mut |_| {},
            )
        }
    }

    #[test]
    fn capture_preserves_a_firmware_reservation_even_if_bar_programming_stays_deferred() {
        let mut devices = [mock()];
        let records = collect_mock(&mut devices);
        assert_eq!(records.len(), 1);
        assert_eq!((records[0].base, records[0].length), (0x4017_0000_00, 4096));
        assert_eq!(devices[0].config[4], 4); // Producer never programs the BAR.
        FIRMWARE.with(|firmware| {
            let firmware = firmware.borrow();
            assert_eq!(firmware.connects, [devices[0].location]);
            assert_eq!((firmware.resource_queries, firmware.freed), (2, 3));
            assert!(firmware.handles.is_none() && firmware.resources.is_empty());
        });
        let module = encode(&records).unwrap();
        assert_eq!(
            wire::find(&module, 0, 0, 21, 0, 0x8086, 0xa0e8, 0).unwrap(),
            Some(records[0])
        );
    }

    #[test]
    fn unrelated_or_nonzero_segment_devices_are_not_connected_or_queried() {
        let mut devices = [mock(), mock(), mock(), mock()];
        devices[0].config[0] = 0xa0ed_8086;
        devices[1].config[0] = 0xa0e9_8086;
        devices[2].config[2] = 0x0c03_0000;
        devices[3].location.segment = 1;
        assert!(collect_mock(&mut devices).is_empty());
        FIRMWARE.with(|firmware| {
            let firmware = firmware.borrow();
            assert!(firmware.connects.is_empty());
            assert_eq!((firmware.resource_queries, firmware.freed), (0, 1));
        });
    }

    #[test]
    fn resource_error_buffers_are_freed_and_programmed_conflicts_are_rejected() {
        let mut devices = [mock()];
        devices[0].resource_status = Status::DEVICE_ERROR;
        assert!(collect_mock(&mut devices).is_empty());
        FIRMWARE.with(|firmware| assert_eq!(firmware.borrow().freed, 3));
        devices[0].resource_status = Status::SUCCESS;
        devices[0].config[4] = 0x8000_0004;
        assert!(collect_mock(&mut devices).is_empty());
        FIRMWARE.with(|firmware| {
            let firmware = firmware.borrow();
            assert!(firmware.connects.is_empty());
            assert_eq!(firmware.freed, 2);
        });
    }

    #[test]
    fn identity_changes_during_firmware_connect_cannot_create_a_reservation() {
        let mut devices = [mock()];
        devices[0].change_identity_on_connect = true;
        assert!(collect_mock(&mut devices).is_empty());
        FIRMWARE.with(|firmware| assert_eq!(firmware.borrow().freed, 2));
    }
}
