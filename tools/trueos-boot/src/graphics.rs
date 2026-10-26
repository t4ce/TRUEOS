//! Select the loader's display mode and preserve the same display's EDID.
//!
//! All firmware buffers are released or copied while boot services are live.

extern crate alloc;

use self::alloc::vec::Vec;
use core::{mem, ptr};
use r_efi::{
    base::{Guid, Handle, Status},
    protocols::graphics_output as gop,
    system::{self, BootServices, SystemTable},
};
use crate::handoff::Framebuffer;

pub const PREFERRED_WIDTH: u32 = 2560;
pub const PREFERRED_HEIGHT: u32 = 1440;
const MAX_MODES: u32 = 4096;
const MAX_HANDLES: usize = 4096;
const EDID_BLOCK: usize = 128;
const MAX_EDID_BYTES: usize = 32 * EDID_BLOCK;
const MAX_PROBES_PER_STAGE: usize = 16;
const EDID_ACTIVE: Guid = Guid::from_fields(
    0xbd8c1056, 0x9f36, 0x44ec, 0x92, 0xa8, &[0xa6, 0x33, 0x7f, 0x81, 0x79, 0x86],
);
const EDID_DISCOVERED: Guid = Guid::from_fields(
    0x1c0c34f6, 0xd380, 0x41fa, 0xa0, 0x49, &[0x8a, 0xd0, 0x6c, 0x1a, 0x66, 0xaa],
);

#[repr(C)]
struct EdidProtocol { size_of_edid: u32, edid: *const u8 }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModeInfo {
    pub index: u32,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub pixel_format: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EdidSource { Active, Discovered }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProbeStage { BeforeMode, AfterMode }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProbeKind { Gop, EdidActiveHandle }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EdidValidation { Unavailable, NullProtocol, NullData, Short, Header, Checksum, Valid }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EdidProbe {
    pub status: Status,
    pub size: u32,
    pub valid: bool,
    pub validation: EdidValidation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GopProbe {
    pub stage: ProbeStage,
    pub kind: ProbeKind,
    pub handle: usize,
    pub protocol: usize,
    pub protocol_status: Status,
    pub mode: Option<ModeInfo>,
    pub frame_buffer_base: u64,
    pub frame_buffer_size: usize,
    pub same_scanout: bool,
    pub edid_active: EdidProbe,
    pub edid_discovered: EdidProbe,
}

pub struct Output {
    pub framebuffer: Option<Framebuffer>,
    pub before: Option<ModeInfo>,
    pub after: Option<ModeInfo>,
    pub preferred_mode: Option<u32>,
    pub set_status: Option<Status>,
    pub queried_modes: u32,
    pub query_failures: u32,
    pub free_failures: u32,
    pub console_gop: bool,
    /// Owned bytes, copied from the same GOP device after mode selection.
    pub edid: Option<Vec<u8>>,
    pub edid_source: Option<EdidSource>,
    /// Bounded firmware evidence; does not participate in display selection.
    pub probes: Vec<GopProbe>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidProtocol,
    Firmware { operation: &'static str, status: Status },
}

#[derive(Clone, Copy)]
struct Services {
    handle_protocol: system::BootHandleProtocol,
    locate_protocol: system::BootLocateProtocol,
    locate_handle_buffer: system::BootLocateHandleBuffer,
    free_pool: system::BootFreePool,
}

struct Display {
    protocol: *mut gop::Protocol,
    handle: Handle,
    console: bool,
    free_failures: u32,
}

/// Prefer a directly addressable 2560x1440 RGB/BGR mode. If unavailable or
/// rejected by firmware, retain whichever valid mode is currently active.
///
/// # Safety
/// The system table and boot services must be valid. Call before configuring
/// the loader's framebuffer console, and only before ExitBootServices.
pub unsafe fn initialize(st: *mut SystemTable) -> Result<Output, Error> {
    let st = unsafe { st.as_ref() }.ok_or(Error::InvalidProtocol)?;
    let bs: &BootServices = unsafe { st.boot_services.as_ref() }.ok_or(Error::InvalidProtocol)?;
    let services = Services {
        handle_protocol: bs.handle_protocol,
        locate_protocol: bs.locate_protocol,
        locate_handle_buffer: bs.locate_handle_buffer,
        free_pool: bs.free_pool,
    };
    let display = unsafe { find_display(st.console_out_handle, services) }?;
    unsafe { configure(display, services) }
}

unsafe fn find_display(console: Handle, services: Services) -> Result<Display, Error> {
    let mut guid = gop::PROTOCOL_GUID;
    let mut raw = ptr::null_mut();
    if !console.is_null() {
        let status = (services.handle_protocol)(console, &mut guid, &mut raw);
        if !status.is_error() && !raw.is_null() {
            // A multi-output firmware console can expose only BLT operations.
            // Prefer one unambiguous physical display with a linear buffer.
            if unsafe { capture(raw.cast()) }.is_none() {
                let (physical, free_failures) = unsafe { unique_physical_display(raw.cast(), services) };
                if let Some(mut physical) = physical {
                    physical.free_failures += free_failures;
                    return Ok(physical);
                }
                return Ok(Display { protocol: raw.cast(), handle: console, console: true, free_failures });
            }
            return Ok(Display { protocol: raw.cast(), handle: console, console: true, free_failures: 0 });
        }
    }
    raw = ptr::null_mut();
    let status = (services.locate_protocol)(&mut guid, ptr::null_mut(), &mut raw);
    if status.is_error() { return Err(Error::Firmware { operation: "locate GOP", status }); }
    if raw.is_null() { return Err(Error::InvalidProtocol); }

    // LocateProtocol returns no owning handle. Recover that handle by matching
    // the exact GOP pointer, so EDID cannot come from a different adapter.
    let (handles, free_failures) = unsafe { gop_handles(services) };
    let mut selected = ptr::null_mut();
    for handle in handles {
        if unsafe { gop_on(handle, services) }.is_some_and(|candidate| candidate.cast() == raw) {
            selected = handle;
            break;
        }
    }
    Ok(Display { protocol: raw.cast(), handle: selected, console: false, free_failures })
}

unsafe fn gop_handles(services: Services) -> (Vec<Handle>, u32) {
    unsafe { protocol_handles(gop::PROTOCOL_GUID, services) }
}

unsafe fn protocol_handles(mut guid: Guid, services: Services) -> (Vec<Handle>, u32) {
    let mut count = 0usize;
    let mut handles: *mut Handle = ptr::null_mut();
    let status = (services.locate_handle_buffer)(
        system::BY_PROTOCOL, &mut guid, ptr::null_mut(), &mut count, &mut handles,
    );
    let copied = if !status.is_error() && !handles.is_null() && count <= MAX_HANDLES {
        unsafe { core::slice::from_raw_parts(handles, count) }.to_vec()
    } else { Vec::new() };
    let failed = !handles.is_null() && (services.free_pool)(handles.cast()).is_error();
    (copied, u32::from(failed))
}

unsafe fn gop_on(handle: Handle, services: Services) -> Option<*mut gop::Protocol> {
    let mut guid = gop::PROTOCOL_GUID;
    let mut raw = ptr::null_mut();
    if (services.handle_protocol)(handle, &mut guid, &mut raw).is_error() || raw.is_null() { None }
    else { Some(raw.cast()) }
}

unsafe fn unique_physical_display(console: *mut gop::Protocol, services: Services) -> (Option<Display>, u32) {
    let (handles, free_failures) = unsafe { gop_handles(services) };
    let mut candidates = Vec::new();
    for handle in handles {
        let Some(protocol) = (unsafe { gop_on(handle, services) }) else { continue; };
        if protocol == console || unsafe { capture(protocol) }.is_none()
            || candidates.iter().any(|(candidate, _, _)| *candidate == protocol)
        { continue; }
        let has_edid = unsafe { read_edid(handle, services.handle_protocol) }.0.is_some();
        candidates.push((protocol, handle, has_edid));
    }
    let with_edid = candidates.iter().filter(|(_, _, has_edid)| *has_edid).count();
    let selected = if with_edid == 1 {
        candidates.into_iter().find(|(_, _, has_edid)| *has_edid)
    } else if candidates.len() == 1 {
        candidates.into_iter().next()
    } else { None };
    let display = selected.map(|(protocol, handle, _)| Display { protocol, handle, console: false, free_failures: 0 });
    (display, free_failures)
}

fn direct_pixels(info: &gop::ModeInformation) -> bool {
    matches!(info.pixel_format,
        gop::PIXEL_RED_GREEN_BLUE_RESERVED_8_BIT_PER_COLOR |
        gop::PIXEL_BLUE_GREEN_RED_RESERVED_8_BIT_PER_COLOR)
        && info.horizontal_resolution > 0 && info.vertical_resolution > 0
        && info.pixels_per_scan_line >= info.horizontal_resolution
}

fn summary(index: u32, info: &gop::ModeInformation) -> ModeInfo {
    ModeInfo { index, width: info.horizontal_resolution, height: info.vertical_resolution,
        stride: info.pixels_per_scan_line, pixel_format: info.pixel_format }
}

unsafe fn current(protocol: *mut gop::Protocol) -> Option<ModeInfo> {
    let mode = unsafe { (*protocol).mode.as_ref() }?;
    if mode.size_of_info < mem::size_of::<gop::ModeInformation>() { return None; }
    let info = unsafe { mode.info.as_ref() }?;
    Some(summary(mode.mode, info))
}

unsafe fn capture(protocol: *mut gop::Protocol) -> Option<Framebuffer> {
    let mode = unsafe { (*protocol).mode.as_ref() }?;
    if mode.mode >= mode.max_mode || mode.size_of_info < mem::size_of::<gop::ModeInformation>() { return None; }
    let info = unsafe { mode.info.as_ref() }?;
    if !direct_pixels(info) || mode.frame_buffer_base == 0 { return None; }
    let pitch = u64::from(info.pixels_per_scan_line).checked_mul(4)?;
    let bytes = pitch.checked_mul(u64::from(info.vertical_resolution))?;
    if bytes > mode.frame_buffer_size as u64 { return None; }
    mode.frame_buffer_base.checked_add(bytes)?;
    let (red, blue) = if info.pixel_format == gop::PIXEL_RED_GREEN_BLUE_RESERVED_8_BIT_PER_COLOR { (0, 16) } else { (16, 0) };
    Some(Framebuffer {
        phys: mode.frame_buffer_base, width: u64::from(info.horizontal_resolution),
        height: u64::from(info.vertical_resolution), pitch, bpp: 32,
        red_mask_size: 8, red_mask_shift: red, green_mask_size: 8, green_mask_shift: 8,
        blue_mask_size: 8, blue_mask_shift: blue,
    })
}

unsafe fn configure(display: Display, services: Services) -> Result<Output, Error> {
    let protocol = display.protocol;
    if protocol.is_null() || unsafe { (*protocol).mode.is_null() } { return Err(Error::InvalidProtocol); }
    let before = unsafe { current(protocol) };
    let mut output = Output {
        framebuffer: None, before, after: None, preferred_mode: None,
        set_status: None, queried_modes: 0, query_failures: 0,
        free_failures: display.free_failures, console_gop: display.console,
        edid: None, edid_source: None, probes: Vec::new(),
    };
    output.free_failures += unsafe { collect_probes(protocol, ProbeStage::BeforeMode, services, &mut output.probes) };
    // Preserve the firmware's original display metadata before SetMode can
    // replace protocol buffers or change a console/physical-GOP association.
    // The working Limine path likewise captures EDID before changing modes.
    let (mut initial_edid, mut initial_source) = unsafe { read_edid(display.handle, services.handle_protocol) };
    if initial_edid.is_none() && unsafe { capture(protocol) }.is_some() {
        let (edid, source, free_failures) = unsafe { alias_edid(protocol, display.handle, services) };
        initial_edid = edid;
        initial_source = source;
        output.free_failures += free_failures;
    }
    let already_preferred = before.is_some_and(|info| info.width == PREFERRED_WIDTH && info.height == PREFERRED_HEIGHT)
        && unsafe { capture(protocol) }.is_some();
    if already_preferred {
        output.preferred_mode = before.map(|info| info.index);
    } else {
        let maximum = unsafe { (*(*protocol).mode).max_mode }.min(MAX_MODES);
        for index in 0..maximum {
            let mut size = 0usize;
            let mut queried: *mut gop::ModeInformation = ptr::null_mut();
            let status = unsafe { ((*protocol).query_mode)(protocol, index, &mut size, &mut queried) };
            output.queried_modes += 1;
            let info = if !status.is_error() && !queried.is_null() && size >= mem::size_of::<gop::ModeInformation>() {
                Some(unsafe { queried.read() })
            } else {
                output.query_failures += 1;
                None
            };
            // QueryMode allocates this buffer. Release it even when firmware
            // returned malformed metadata or a failure status with a buffer.
            if !queried.is_null() && (services.free_pool)(queried.cast()).is_error() {
                output.free_failures += 1;
            }
            if info.is_some_and(|info| direct_pixels(&info)
                && info.horizontal_resolution == PREFERRED_WIDTH && info.vertical_resolution == PREFERRED_HEIGHT)
            {
                output.preferred_mode = Some(index);
                output.set_status = Some(unsafe { ((*protocol).set_mode)(protocol, index) });
                break;
            }
        }
    }
    // SetMode may replace both the mode information and framebuffer address.
    // Capture fresh metadata for both successful and rejected mode changes.
    output.after = unsafe { current(protocol) };
    output.framebuffer = unsafe { capture(protocol) };
    output.free_failures += unsafe { collect_probes(protocol, ProbeStage::AfterMode, services, &mut output.probes) };
    let (mut edid, mut source) = unsafe { read_edid(display.handle, services.handle_protocol) };
    if edid.is_none() && output.framebuffer.is_some() {
        let (aliased_edid, aliased_source, free_failures) = unsafe { alias_edid(protocol, display.handle, services) };
        edid = aliased_edid;
        source = aliased_source;
        output.free_failures += free_failures;
    }
    if edid.is_none() {
        edid = initial_edid;
        source = initial_source;
    }
    output.edid = edid;
    output.edid_source = source;
    Ok(output)
}

unsafe fn probe_edid(handle: Handle, mut guid: Guid, services: Services) -> EdidProbe {
    let mut raw = ptr::null_mut();
    let status = (services.handle_protocol)(handle, &mut guid, &mut raw);
    let mut probe = EdidProbe { status, size: 0, valid: false, validation: EdidValidation::Unavailable };
    if status.is_error() { return probe; }
    let Some(protocol) = (unsafe { raw.cast::<EdidProtocol>().as_ref() }) else {
        probe.validation = EdidValidation::NullProtocol;
        return probe;
    };
    probe.size = protocol.size_of_edid;
    if protocol.edid.is_null() { probe.validation = EdidValidation::NullData; return probe; }
    let size = (protocol.size_of_edid as usize).min(MAX_EDID_BYTES);
    if size < EDID_BLOCK { probe.validation = EdidValidation::Short; return probe; }
    let data = unsafe { core::slice::from_raw_parts(protocol.edid, size) };
    if data[..8] != [0, 255, 255, 255, 255, 255, 255, 0] {
        probe.validation = EdidValidation::Header;
    } else if validate_edid(data).is_none() {
        probe.validation = EdidValidation::Checksum;
    } else {
        probe.valid = true;
        probe.validation = EdidValidation::Valid;
    }
    probe
}

unsafe fn collect_probes(
    selected: *mut gop::Protocol, stage: ProbeStage, services: Services, probes: &mut Vec<GopProbe>,
) -> u32 {
    let first = probes.len();
    let limit = first + MAX_PROBES_PER_STAGE;
    let mut free_failures = 0;
    for (guid, kind) in [(gop::PROTOCOL_GUID, ProbeKind::Gop), (EDID_ACTIVE, ProbeKind::EdidActiveHandle)] {
        let (handles, failed) = unsafe { protocol_handles(guid, services) };
        free_failures += failed;
        for handle in handles {
            if probes.len() >= limit { break; }
            if probes[first..].iter().any(|probe| probe.handle == handle as usize) { continue; }
            let mut guid = gop::PROTOCOL_GUID;
            let mut raw = ptr::null_mut();
            let protocol_status = (services.handle_protocol)(handle, &mut guid, &mut raw);
            let protocol = if protocol_status.is_error() { ptr::null_mut() } else { raw.cast::<gop::Protocol>() };
            let mode = if protocol.is_null() { None } else { unsafe { current(protocol) } };
            let descriptor = if protocol.is_null() { None } else { unsafe { (*protocol).mode.as_ref() } };
            let frame_buffer_base = descriptor.map_or(0, |mode| mode.frame_buffer_base);
            let frame_buffer_size = descriptor.map_or(0, |mode| mode.frame_buffer_size);
            let same_scanout = !protocol.is_null() && unsafe { same_scanout(selected, protocol) };
            probes.push(GopProbe {
                stage, kind, handle: handle as usize, protocol: protocol as usize, protocol_status,
                mode, frame_buffer_base, frame_buffer_size, same_scanout,
                edid_active: unsafe { probe_edid(handle, EDID_ACTIVE, services) },
                edid_discovered: unsafe { probe_edid(handle, EDID_DISCOVERED, services) },
            });
        }
    }
    free_failures
}

unsafe fn same_scanout(a: *mut gop::Protocol, b: *mut gop::Protocol) -> bool {
    let (Some(a_info), Some(b_info)) = (unsafe { current(a) }, unsafe { current(b) }) else { return false; };
    let (Some(a_fb), Some(b_fb)) = (unsafe { capture(a) }, unsafe { capture(b) }) else { return false; };
    a_fb.phys == b_fb.phys && a_info.width == b_info.width && a_info.height == b_info.height
        && a_info.stride == b_info.stride && a_info.pixel_format == b_info.pixel_format
        && unsafe { (*(*a).mode).frame_buffer_size == (*(*b).mode).frame_buffer_size }
}

/// ConSplitter can expose a virtual GOP with the same linear framebuffer as
/// its physical child, or share its exact interface, while EDID lives only on
/// the child's handle. Exclude the selected handle rather than its interface.
/// Accept that association only when one physical GOP matches exactly, even if a
/// second matching output has no EDID of its own.
unsafe fn alias_edid(protocol: *mut gop::Protocol, selected_handle: Handle, services: Services) -> (Option<Vec<u8>>, Option<EdidSource>, u32) {
    let (handles, free_failures) = unsafe { gop_handles(services) };
    let mut matched: Option<(*mut gop::Protocol, Vec<u8>, EdidSource)> = None;
    let mut matching_protocol = None;
    for handle in handles {
        if handle == selected_handle { continue; }
        let Some(candidate) = (unsafe { gop_on(handle, services) }) else { continue; };
        if !unsafe { same_scanout(protocol, candidate) } { continue; }
        if matching_protocol.is_some_and(|previous| previous != candidate) {
            return (None, None, free_failures);
        }
        matching_protocol = Some(candidate);
        let (Some(data), Some(source)) = (unsafe { read_edid(handle, services.handle_protocol) }) else { continue; };
        if let Some((previous, previous_data, _)) = &matched {
            if *previous != candidate || *previous_data != data { return (None, None, free_failures); }
        } else { matched = Some((candidate, data, source)); }
    }
    match matched {
        Some((_, data, source)) => (Some(data), Some(source), free_failures),
        None => (None, None, free_failures),
    }
}

unsafe fn read_edid(handle: Handle, handle_protocol: system::BootHandleProtocol) -> (Option<Vec<u8>>, Option<EdidSource>) {
    if handle.is_null() { return (None, None); }
    for (mut guid, source) in [(EDID_ACTIVE, EdidSource::Active), (EDID_DISCOVERED, EdidSource::Discovered)] {
        let mut raw = ptr::null_mut();
        let status = handle_protocol(handle, &mut guid, &mut raw);
        if status.is_error() || raw.is_null() { continue; }
        let protocol = unsafe { &*raw.cast::<EdidProtocol>() };
        let size = (protocol.size_of_edid as usize).min(MAX_EDID_BYTES);
        if protocol.edid.is_null() || size < EDID_BLOCK { continue; }
        let data = unsafe { core::slice::from_raw_parts(protocol.edid, size) };
        if let Some(data) = validate_edid(data) { return (Some(data.to_vec()), Some(source)); }
    }
    (None, None)
}

fn validate_edid(data: &[u8]) -> Option<&[u8]> {
    if data.len() < EDID_BLOCK || data[..8] != [0, 255, 255, 255, 255, 255, 255, 0]
    { return None; }
    let declared_bytes = (usize::from(data[126]) + 1).checked_mul(EDID_BLOCK)?;
    let available = data.len().min(declared_bytes).min(MAX_EDID_BYTES);
    let mut valid_bytes = 0;
    for block in data[..available].chunks_exact(EDID_BLOCK) {
        if block.iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte)) != 0 { break; }
        valid_bytes += EDID_BLOCK;
    }
    // The kernel accepts base-only EDID. Missing or bad extension blocks must
    // not discard the valid base containing the display's physical size.
    if valid_bytes == 0 { None } else { Some(&data[..valid_bytes]) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use self::alloc::{boxed::Box, vec, vec::Vec};
    use std::cell::RefCell;
    use core::ffi::c_void;

    struct Lookup {
        devices: Vec<*mut Mock>,
        allocation: *mut Handle,
        allocation_len: usize,
        freed: bool,
    }
    std::thread_local! { static LOOKUP: RefCell<Option<Lookup>> = const { RefCell::new(None) }; }

    fn info(width: u32, height: u32, pixel_format: u32) -> gop::ModeInformation {
        gop::ModeInformation { version: 0, horizontal_resolution: width, vertical_resolution: height,
            pixel_format, pixel_information: gop::PixelBitmask { red_mask: 0, green_mask: 0, blue_mask: 0, reserved_mask: 0 },
            pixels_per_scan_line: width }
    }

    #[repr(C)]
    struct Mock {
        protocol: gop::Protocol,
        mode: Box<gop::Mode>,
        modes: Vec<gop::ModeInformation>,
        queries: Vec<u32>,
        sets: Vec<u32>,
        freed: usize,
        fail_query: Option<u32>,
        short_query: Option<u32>,
        fail_set: bool,
        invalidate_edid_on_set: bool,
        free_status: Status,
        edid: Vec<u8>,
        active: EdidProtocol,
        discovered: EdidProtocol,
        no_gop: bool,
        gop_alias: *mut gop::Protocol,
    }

    #[repr(C)]
    struct QueryAllocation { info: gop::ModeInformation, owner: *mut Mock }

    extern "efiapi" fn query(protocol: *mut gop::Protocol, index: u32, size: *mut usize, result: *mut *mut gop::ModeInformation) -> Status {
        let mock = unsafe { &mut *protocol.cast::<Mock>() };
        mock.queries.push(index);
        let Some(info) = mock.modes.get(index as usize).copied() else { return Status::NOT_FOUND; };
        let allocation = Box::new(QueryAllocation { info, owner: mock });
        unsafe { *result = Box::into_raw(allocation).cast();
            *size = if mock.short_query == Some(index) { 1 } else { mem::size_of::<gop::ModeInformation>() }; }
        if mock.fail_query == Some(index) { Status::DEVICE_ERROR } else { Status::SUCCESS }
    }

    extern "efiapi" fn set(protocol: *mut gop::Protocol, index: u32) -> Status {
        let mock = unsafe { &mut *protocol.cast::<Mock>() };
        mock.sets.push(index);
        if mock.fail_set { return Status::DEVICE_ERROR; }
        mock.mode.mode = index;
        let info = &mut mock.modes[index as usize];
        mock.mode.info = info;
        mock.mode.frame_buffer_size = info.pixels_per_scan_line as usize * info.vertical_resolution as usize * 4;
        // Deliberately change the framebuffer to catch stale capture metadata.
        mock.mode.frame_buffer_base = 0x200000;
        if mock.invalidate_edid_on_set {
            mock.edid.fill(0);
            mock.active.size_of_edid = 0;
            mock.discovered.size_of_edid = 0;
        }
        Status::SUCCESS
    }

    extern "efiapi" fn blt(_: *mut gop::Protocol, _: *mut gop::BltPixel, _: u32, _: usize, _: usize, _: usize, _: usize, _: usize, _: usize, _: usize) -> Status {
        Status::UNSUPPORTED
    }

    extern "efiapi" fn free(allocation: *mut c_void) -> Status {
        let allocation = unsafe { Box::from_raw(allocation.cast::<QueryAllocation>()) };
        let owner = unsafe { &mut *allocation.owner };
        owner.freed += 1;
        owner.free_status
    }

    extern "efiapi" fn handle(handle: Handle, guid: *mut Guid, raw: *mut *mut c_void) -> Status {
        let mock = unsafe { &mut *handle.cast::<Mock>() };
        let guid = unsafe { *guid };
        if guid == gop::PROTOCOL_GUID && mock.no_gop { return Status::UNSUPPORTED; }
        let result = if guid == gop::PROTOCOL_GUID {
            if mock.gop_alias.is_null() { ptr::addr_of_mut!(mock.protocol).cast() }
            else { mock.gop_alias.cast() }
        }
            else if guid == EDID_ACTIVE { ptr::addr_of_mut!(mock.active).cast() }
            else if guid == EDID_DISCOVERED { ptr::addr_of_mut!(mock.discovered).cast() }
            else { return Status::UNSUPPORTED; };
        unsafe { *raw = result; }
        Status::SUCCESS
    }

    extern "efiapi" fn locate(_: *mut Guid, _: *mut c_void, _: *mut *mut c_void) -> Status { Status::NOT_FOUND }
    extern "efiapi" fn locate_handles(_: u32, _: *mut Guid, _: *mut c_void, _: *mut usize, _: *mut *mut Handle) -> Status { Status::NOT_FOUND }

    extern "efiapi" fn locate_second(_: *mut Guid, _: *mut c_void, raw: *mut *mut c_void) -> Status {
        LOOKUP.with(|lookup| {
            let lookup = lookup.borrow();
            let device = lookup.as_ref().unwrap().devices[1];
            unsafe { *raw = ptr::addr_of_mut!((*device).protocol).cast(); }
        });
        Status::SUCCESS
    }

    extern "efiapi" fn list_devices(search: u32, guid: *mut Guid, _: *mut c_void, count: *mut usize, handles_out: *mut *mut Handle) -> Status {
        assert_eq!(search, system::BY_PROTOCOL);
        let guid = unsafe { *guid };
        assert!(guid == gop::PROTOCOL_GUID || guid == EDID_ACTIVE);
        LOOKUP.with(|lookup| {
            let mut lookup = lookup.borrow_mut();
            let lookup = lookup.as_mut().unwrap();
            let handles: Vec<Handle> = lookup.devices.iter()
                .filter(|device| guid != gop::PROTOCOL_GUID || !unsafe { (***device).no_gop })
                .map(|device| device.cast()).collect();
            lookup.allocation_len = handles.len();
            lookup.allocation = Box::into_raw(handles.into_boxed_slice()).cast();
            unsafe { *count = lookup.allocation_len; }
            unsafe { *handles_out = lookup.allocation; }
        });
        Status::SUCCESS
    }

    extern "efiapi" fn free_lookup(allocation: *mut c_void) -> Status {
        let handle_list = LOOKUP.with(|lookup| {
            let mut lookup = lookup.borrow_mut();
            let lookup = lookup.as_mut().unwrap();
            if allocation == lookup.allocation.cast() {
                unsafe { drop(Box::from_raw(ptr::slice_from_raw_parts_mut(lookup.allocation, lookup.allocation_len))); }
                lookup.allocation = ptr::null_mut();
                lookup.freed = true;
                true
            } else { false }
        });
        if handle_list { Status::SUCCESS } else { free(allocation) }
    }

    fn services() -> Services {
        Services { handle_protocol: handle, locate_protocol: locate, locate_handle_buffer: locate_handles, free_pool: free }
    }

    fn lookup_services(devices: Vec<*mut Mock>) -> Services {
        LOOKUP.with(|lookup| *lookup.borrow_mut() = Some(Lookup {
            devices, allocation: ptr::null_mut(), allocation_len: 0, freed: false,
        }));
        Services { locate_protocol: locate_second, locate_handle_buffer: list_devices,
            free_pool: free_lookup, ..services() }
    }

    fn edid() -> Vec<u8> {
        let mut data = vec![0; EDID_BLOCK];
        data[..8].copy_from_slice(&[0, 255, 255, 255, 255, 255, 255, 0]);
        data[18] = 1; data[19] = 4; data[21] = 60; data[22] = 34;
        checksum(&mut data);
        data
    }

    fn checksum(data: &mut [u8]) {
        let sum = data[..127].iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte));
        data[127] = 0u8.wrapping_sub(sum);
    }

    fn mock(modes: Vec<gop::ModeInformation>) -> Box<Mock> {
        let first = modes[0];
        let mut mock = Box::new(Mock {
            protocol: gop::Protocol { query_mode: query, set_mode: set, blt, mode: ptr::null_mut() },
            mode: Box::new(gop::Mode { max_mode: modes.len() as u32, mode: 0, info: ptr::null_mut(),
                size_of_info: mem::size_of::<gop::ModeInformation>(), frame_buffer_base: 0x100000,
                frame_buffer_size: first.pixels_per_scan_line as usize * first.vertical_resolution as usize * 4 }),
            modes, queries: Vec::new(), sets: Vec::new(), freed: 0, fail_query: None, short_query: None,
            fail_set: false, invalidate_edid_on_set: false, free_status: Status::SUCCESS, edid: edid(),
            active: EdidProtocol { size_of_edid: 0, edid: ptr::null() },
            discovered: EdidProtocol { size_of_edid: 0, edid: ptr::null() },
            no_gop: false, gop_alias: ptr::null_mut(),
        });
        mock.mode.info = &mut mock.modes[0];
        mock.protocol.mode = &mut *mock.mode;
        mock.active = EdidProtocol { size_of_edid: mock.edid.len() as u32, edid: mock.edid.as_ptr() };
        mock
    }

    unsafe fn run(mock: &mut Mock) -> Output {
        let display = unsafe { find_display((mock as *mut Mock).cast(), services()) }.unwrap();
        unsafe { configure(display, services()) }.unwrap()
    }

    #[test]
    fn preferred_mode_is_selected_and_all_query_buffers_freed() {
        let mut mock = mock(vec![info(1024, 768, 1), info(2560, 1440, 3), info(2560, 1440, 0)]);
        let output = unsafe { run(&mut mock) };
        assert!(output.console_gop);
        assert_eq!(output.before.unwrap().width, 1024);
        assert_eq!(output.after.unwrap().width, 2560);
        assert_eq!(output.preferred_mode, Some(2));
        assert_eq!(output.set_status, Some(Status::SUCCESS));
        assert_eq!(mock.queries, vec![0, 1, 2]);
        assert_eq!(mock.sets, vec![2]);
        assert_eq!(mock.freed, 3);
        let framebuffer = output.framebuffer.unwrap();
        assert_eq!((framebuffer.phys, framebuffer.width, framebuffer.height), (0x200000, 2560, 1440));
        assert_eq!((framebuffer.red_mask_shift, framebuffer.blue_mask_shift), (0, 16));
        assert_eq!(output.edid_source, Some(EdidSource::Active));
        assert_eq!(output.edid.as_deref(), Some(mock.edid.as_slice()));
    }

    #[test]
    fn original_edid_copy_survives_mode_change_invalidating_firmware_buffer() {
        let mut mock = mock(vec![info(1024, 768, 1), info(2560, 1440, 1)]);
        let original = mock.edid.clone();
        mock.invalidate_edid_on_set = true;
        let output = unsafe { run(&mut mock) };
        assert_eq!(output.after.unwrap().width, 2560);
        assert_eq!(mock.active.size_of_edid, 0);
        assert!(mock.edid.iter().all(|byte| *byte == 0));
        assert_eq!(output.edid, Some(original));
        assert_eq!(output.edid_source, Some(EdidSource::Active));
    }

    #[test]
    fn probes_record_before_after_edid_loss_and_edid_handle_without_gop() {
        let mut selected = mock(vec![info(1024, 768, 1), info(2560, 1440, 1)]);
        selected.invalidate_edid_on_set = true;
        let selected_handle = (&mut *selected as *mut Mock).cast();
        let mut edid_only = mock(vec![info(1024, 768, 1)]);
        edid_only.no_gop = true;
        let edid_handle: Handle = (&mut *edid_only as *mut Mock).cast();
        let services = lookup_services(vec![&mut *selected, &mut *edid_only]);
        let display = unsafe { find_display(selected_handle, services) }.unwrap();
        let output = unsafe { configure(display, services) }.unwrap();
        assert_eq!(output.probes.len(), 4);
        let before = output.probes.iter().find(|probe| probe.handle == selected_handle as usize && probe.stage == ProbeStage::BeforeMode).unwrap();
        assert_eq!(before.mode.unwrap().width, 1024);
        assert!(before.same_scanout && before.edid_active.valid);
        assert_eq!(before.edid_active.validation, EdidValidation::Valid);
        let after = output.probes.iter().find(|probe| probe.handle == selected_handle as usize && probe.stage == ProbeStage::AfterMode).unwrap();
        assert_eq!(after.mode.unwrap().width, 2560);
        assert!(!after.edid_active.valid);
        assert_eq!(after.edid_active.size, 0);
        for probe in output.probes.iter().filter(|probe| probe.handle == edid_handle as usize) {
            assert_eq!(probe.kind, ProbeKind::EdidActiveHandle);
            assert_eq!(probe.protocol_status, Status::UNSUPPORTED);
            assert_eq!(probe.protocol, 0);
            assert!(probe.mode.is_none());
            assert!(!probe.same_scanout);
            assert!(probe.edid_active.valid);
        }
        LOOKUP.with(|lookup| *lookup.borrow_mut() = None);
    }

    #[test]
    fn probes_report_capacity_mismatch_and_have_a_fixed_record_bound() {
        let mut devices: Vec<_> = (0..20).map(|_| mock(vec![info(2560, 1440, 1)])).collect();
        devices[1].mode.frame_buffer_size += 4096;
        let selected = (&mut *devices[0] as *mut Mock).cast();
        let alias = (&mut *devices[1] as *mut Mock).cast::<core::ffi::c_void>() as usize;
        let pointers = devices.iter_mut().map(|device| &mut **device as *mut Mock).collect();
        let services = lookup_services(pointers);
        let display = unsafe { find_display(selected, services) }.unwrap();
        let output = unsafe { configure(display, services) }.unwrap();
        assert_eq!(output.probes.len(), MAX_PROBES_PER_STAGE * 2);
        assert_eq!(output.probes.iter().filter(|probe| probe.stage == ProbeStage::BeforeMode).count(), MAX_PROBES_PER_STAGE);
        assert_eq!(output.probes.iter().filter(|probe| probe.stage == ProbeStage::AfterMode).count(), MAX_PROBES_PER_STAGE);
        let alias = output.probes.iter().find(|probe| probe.handle == alias).unwrap();
        assert_eq!(alias.mode.unwrap().width, 2560);
        assert_eq!(alias.frame_buffer_base, 0x100000);
        assert_eq!(alias.frame_buffer_size, 2560 * 1440 * 4 + 4096);
        assert!(!alias.same_scanout);
        assert!(alias.edid_active.valid);
        LOOKUP.with(|lookup| *lookup.borrow_mut() = None);
    }

    #[test]
    fn unavailable_or_rejected_preferred_mode_keeps_valid_current_mode() {
        for preferred in [false, true] {
            let mut modes = vec![info(1920, 1080, 1)];
            if preferred { modes.push(info(2560, 1440, 1)); }
            let mut mock = mock(modes);
            mock.fail_set = true;
            let output = unsafe { run(&mut mock) };
            assert_eq!(output.after, output.before);
            let framebuffer = output.framebuffer.unwrap();
            assert_eq!((framebuffer.phys, framebuffer.width, framebuffer.height), (0x100000, 1920, 1080));
            assert_eq!((framebuffer.red_mask_shift, framebuffer.blue_mask_shift), (16, 0));
            assert_eq!(output.set_status, if preferred { Some(Status::DEVICE_ERROR) } else { None });
        }
    }

    #[test]
    fn already_preferred_does_not_reset_display_or_allocate_queries() {
        let mut mock = mock(vec![info(2560, 1440, 1)]);
        let output = unsafe { run(&mut mock) };
        assert_eq!(output.preferred_mode, Some(0));
        assert!(mock.queries.is_empty());
        assert!(mock.sets.is_empty());
        assert_eq!(mock.freed, 0);
    }

    #[test]
    fn malformed_queries_and_firmware_failures_release_buffers() {
        let mut mock = mock(vec![info(1024, 768, 1), info(2560, 1440, 1), info(2560, 1440, 1)]);
        mock.fail_query = Some(0);
        mock.short_query = Some(1);
        mock.free_status = Status::DEVICE_ERROR;
        let output = unsafe { run(&mut mock) };
        assert_eq!(output.preferred_mode, Some(2));
        assert_eq!(output.query_failures, 2);
        assert_eq!(output.free_failures, 3);
        assert_eq!(mock.freed, 3);
    }

    #[test]
    fn invalid_framebuffer_bounds_formats_and_stride_are_rejected() {
        let mut mock = mock(vec![info(1024, 768, 1)]);
        mock.mode.frame_buffer_size -= 1;
        assert!(unsafe { capture(&mut mock.protocol) }.is_none());
        mock.mode.frame_buffer_size = usize::MAX;
        mock.mode.frame_buffer_base = u64::MAX - 1;
        assert!(unsafe { capture(&mut mock.protocol) }.is_none());
        mock.mode.frame_buffer_base = 0x100000;
        mock.modes[0].pixels_per_scan_line = 1023;
        assert!(unsafe { capture(&mut mock.protocol) }.is_none());
        mock.modes[0].pixels_per_scan_line = 1024;
        mock.modes[0].pixel_format = gop::PIXEL_BLT_ONLY;
        assert!(unsafe { capture(&mut mock.protocol) }.is_none());
        mock.modes[0].pixel_format = gop::PIXEL_BIT_MASK;
        assert!(unsafe { capture(&mut mock.protocol) }.is_none());
    }

    #[test]
    fn firmware_mode_count_is_bounded() {
        let mut mock = mock(vec![info(1024, 768, 1)]);
        mock.mode.max_mode = u32::MAX;
        let output = unsafe { run(&mut mock) };
        assert_eq!(output.queried_modes, MAX_MODES);
        assert_eq!(mock.queries.len(), MAX_MODES as usize);
        assert_eq!(mock.freed, 1);
    }

    #[test]
    fn global_gop_fallback_matches_its_handle_before_reading_edid() {
        let mut other = mock(vec![info(1920, 1080, 1)]);
        other.edid[21] = 30;
        checksum(&mut other.edid);
        let mut selected = mock(vec![info(2560, 1440, 1)]);
        let services = lookup_services(vec![&mut *other, &mut *selected]);
        let display = unsafe { find_display(ptr::null_mut(), services) }.unwrap();
        assert_eq!(display.handle, (&mut *selected as *mut Mock).cast());
        assert!(!display.console);
        assert!(LOOKUP.with(|lookup| lookup.borrow().as_ref().unwrap().freed));
        let output = unsafe { configure(display, services) }.unwrap();
        assert_eq!(output.edid.unwrap()[21], 60);
        LOOKUP.with(|lookup| *lookup.borrow_mut() = None);
    }

    #[test]
    fn virtual_console_edid_uses_unique_matching_scanout_not_another_monitor() {
        let mut console = mock(vec![info(2560, 1440, 1)]);
        console.active.size_of_edid = 0;
        let mut other = mock(vec![info(2560, 1440, 1)]);
        other.mode.frame_buffer_base = 0x300000;
        other.edid[21] = 30;
        checksum(&mut other.edid);
        let mut physical = mock(vec![info(2560, 1440, 1)]);
        let services = lookup_services(vec![&mut *console, &mut *other, &mut *physical]);
        let display = unsafe { find_display((&mut *console as *mut Mock).cast(), services) }.unwrap();
        assert!(display.console);
        let output = unsafe { configure(display, services) }.unwrap();
        assert_eq!(output.edid.unwrap()[21], 60);
        assert!(LOOKUP.with(|lookup| lookup.borrow().as_ref().unwrap().freed));
        LOOKUP.with(|lookup| *lookup.borrow_mut() = None);
    }

    #[test]
    fn console_sharing_physical_gop_interface_reads_edid_from_owning_handle() {
        for invalidate_on_set in [false, true] {
            let mut physical = mock(vec![info(1024, 768, 1), info(2560, 1440, 1)]);
            physical.invalidate_edid_on_set = invalidate_on_set;
            let original = physical.edid.clone();
            let mut console = mock(vec![info(1024, 768, 1)]);
            console.active.size_of_edid = 0;
            console.gop_alias = ptr::addr_of_mut!(physical.protocol);
            // Firmware enumerates only the physical owner, while ConsoleOutHandle
            // returns that same GOP interface without exposing EDID on the console.
            let services = lookup_services(vec![&mut *physical]);
            let display = unsafe { find_display((&mut *console as *mut Mock).cast(), services) }.unwrap();
            assert!(display.console);
            assert_eq!(display.protocol, ptr::addr_of_mut!(physical.protocol));
            let output = unsafe { configure(display, services) }.unwrap();
            assert_eq!(output.after.unwrap().width, 2560);
            assert_eq!(output.edid, Some(original));
            assert_eq!(output.edid_source, Some(EdidSource::Active));
            LOOKUP.with(|lookup| *lookup.borrow_mut() = None);
        }
    }

    #[test]
    fn shared_interface_still_rejects_another_gop_with_matching_scanout() {
        let mut physical = mock(vec![info(2560, 1440, 1)]);
        let mut clone = mock(vec![info(2560, 1440, 1)]);
        let mut console = mock(vec![info(2560, 1440, 1)]);
        console.active.size_of_edid = 0;
        console.gop_alias = ptr::addr_of_mut!(physical.protocol);
        let services = lookup_services(vec![&mut *physical, &mut *clone]);
        let display = unsafe { find_display((&mut *console as *mut Mock).cast(), services) }.unwrap();
        let output = unsafe { configure(display, services) }.unwrap();
        assert!(output.framebuffer.is_some());
        assert!(output.edid.is_none());
        LOOKUP.with(|lookup| *lookup.borrow_mut() = None);
    }

    #[test]
    fn original_physical_alias_edid_survives_mode_change_replacing_scanout_metadata() {
        let mut console = mock(vec![info(1024, 768, 1), info(2560, 1440, 1)]);
        console.active.size_of_edid = 0;
        let mut physical = mock(vec![info(1024, 768, 1)]);
        let original = physical.edid.clone();
        let services = lookup_services(vec![&mut *console, &mut *physical]);
        let display = unsafe { find_display((&mut *console as *mut Mock).cast(), services) }.unwrap();
        let output = unsafe { configure(display, services) }.unwrap();
        assert_eq!(output.after.unwrap().width, 2560);
        assert!(!unsafe { same_scanout(&mut console.protocol, &mut physical.protocol) });
        assert_eq!(output.edid, Some(original));
        assert_eq!(output.edid_source, Some(EdidSource::Active));
        LOOKUP.with(|lookup| *lookup.borrow_mut() = None);
    }

    #[test]
    fn clone_outputs_with_matching_scanout_are_ambiguous_for_edid() {
        for clone_has_edid in [false, true] {
            let mut console = mock(vec![info(2560, 1440, 1)]);
            console.active.size_of_edid = 0;
            let mut physical = mock(vec![info(2560, 1440, 1)]);
            let mut clone = mock(vec![info(2560, 1440, 1)]);
            if !clone_has_edid { clone.active.size_of_edid = 0; }
            let services = lookup_services(vec![&mut *console, &mut *physical, &mut *clone]);
            let display = unsafe { find_display((&mut *console as *mut Mock).cast(), services) }.unwrap();
            let output = unsafe { configure(display, services) }.unwrap();
            assert!(output.framebuffer.is_some());
            assert!(output.edid.is_none());
            LOOKUP.with(|lookup| *lookup.borrow_mut() = None);
        }
    }

    #[test]
    fn blt_only_virtual_console_uses_unambiguous_physical_display() {
        let mut console = mock(vec![info(1024, 768, gop::PIXEL_BLT_ONLY)]);
        let mut physical = mock(vec![info(2560, 1440, 1)]);
        let services = lookup_services(vec![&mut *console, &mut *physical]);
        let display = unsafe { find_display((&mut *console as *mut Mock).cast(), services) }.unwrap();
        assert!(!display.console);
        assert_eq!(display.protocol, ptr::addr_of_mut!(physical.protocol));
        let output = unsafe { configure(display, services) }.unwrap();
        assert_eq!(output.framebuffer.unwrap().width, 2560);
        assert_eq!(output.edid.unwrap()[21], 60);
        LOOKUP.with(|lookup| *lookup.borrow_mut() = None);
    }

    #[test]
    fn edid_validation_checks_declared_blocks_and_falls_back_to_discovered() {
        let mut data = edid();
        assert_eq!(validate_edid(&data), Some(data.as_slice()));
        data[21] ^= 1;
        assert!(validate_edid(&data).is_none());
        data = edid();
        data[126] = 1;
        checksum(&mut data);
        assert_eq!(validate_edid(&data), Some(data.as_slice()));
        data.extend([0u8; 128]);
        assert_eq!(validate_edid(&data), Some(data.as_slice()));
        data[128] = 1;
        assert_eq!(validate_edid(&data), Some(&data[..128]));

        let mut mock = mock(vec![info(2560, 1440, 1)]);
        mock.discovered = EdidProtocol { size_of_edid: mock.edid.len() as u32, edid: mock.edid.as_ptr() };
        mock.active.size_of_edid = 1;
        let output = unsafe { run(&mut mock) };
        assert_eq!(output.edid_source, Some(EdidSource::Discovered));
        let copied = output.edid.unwrap();
        mock.edid[21] = 0;
        assert_eq!(copied[21], 60); // The handoff owns a copy, not firmware data.
        let (data, source) = unsafe { read_edid(ptr::null_mut(), handle) };
        assert!(data.is_none() && source.is_none());
    }
}
