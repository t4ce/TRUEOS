//! Read the staged boot files using the firmware's existing IPv4 PXE session.
//!
//! No network driver or DHCP client is linked into the loader. The PXE session
//! belongs to the device that loaded this image, and is used before exiting
//! boot services. r-efi 5.3 does not yet define the PXE Base Code protocol.

use core::{ffi::c_void, mem, ptr};
use r_efi::{base::{Boolean, Guid, Handle, IpAddress, Status}, system::BootServices};

const PROTOCOL_GUID: Guid = Guid::from_fields(
    0x03c4e603, 0xac28, 0x11d3, 0x9a, 0x2d, &[0x00, 0x90, 0x27, 0x3f, 0xc1, 0x4d],
);
const MAX_FILE_BYTES: u64 = 1024 * 1024 * 1024;
// Leave room for the TFTP, UDP and IPv4 headers within a normal 1500-byte MTU.
const BLOCK_BYTES: usize = 1400;
const TFTP_GET_FILE_SIZE: i32 = 1;
const TFTP_READ_FILE: i32 = 2;

type Mtftp = unsafe extern "efiapi" fn(
    *mut Protocol, i32, *mut c_void, Boolean, *mut u64, *const usize,
    *const IpAddress, *const u8, *const c_void, Boolean,
) -> Status;
type Stop = unsafe extern "efiapi" fn(*mut Protocol) -> Status;

/// EFI_PXE_BASE_CODE_PROTOCOL. Unused function pointers stay opaque rather
/// than declaring unrelated Rust function signatures for them.
#[repr(C)]
struct Protocol {
    revision: u64,
    start: usize,
    stop: Stop,
    dhcp: usize,
    discover: usize,
    mtftp: Mtftp,
    udp_write: usize,
    udp_read: usize,
    set_ip_filter: usize,
    arp: usize,
    set_parameters: usize,
    set_station_ip: usize,
    set_packets: usize,
    mode: *const Mode,
}

/// EFI_PXE_BASE_CODE_PACKET is a 1472-byte union with four-byte alignment.
#[repr(C, align(4))]
#[derive(Clone, Copy)]
struct Packet { raw: [u8; 1472] }

/// The read-only prefix of EFI_PXE_BASE_CODE_MODE through the cached packets.
/// ARP tables and firmware error packets follow this prefix and are not read.
#[repr(C)]
struct Mode {
    started: Boolean,
    ipv6_available: Boolean,
    ipv6_supported: Boolean,
    using_ipv6: Boolean,
    bis_supported: Boolean,
    bis_detected: Boolean,
    auto_arp: Boolean,
    send_guid: Boolean,
    dhcp_discover_valid: Boolean,
    dhcp_ack_received: Boolean,
    proxy_offer_received: Boolean,
    pxe_discover_valid: Boolean,
    pxe_reply_received: Boolean,
    pxe_bis_reply_received: Boolean,
    icmp_error_received: Boolean,
    tftp_error_received: Boolean,
    make_callbacks: Boolean,
    ttl: u8,
    tos: u8,
    station_ip: IpAddress,
    subnet_mask: IpAddress,
    dhcp_discover: Packet,
    dhcp_ack: Packet,
    proxy_offer: Packet,
    pxe_discover: Packet,
    pxe_reply: Packet,
    pxe_bis_reply: Packet,
}

const _: () = {
    assert!(mem::size_of::<Packet>() == 1472);
    assert!(mem::align_of::<Packet>() == 4);
    assert!(mem::offset_of!(Mode, station_ip) == 20);
    assert!(mem::offset_of!(Mode, dhcp_ack) == 1524);
    assert!(mem::offset_of!(Mode, proxy_offer) == 2996);
    assert!(mem::offset_of!(Mode, pxe_reply) == 5940);
    assert!(mem::offset_of!(Protocol, mtftp) == 40);
    assert!(mem::offset_of!(Protocol, mode) == 104);
    assert!(mem::size_of::<Protocol>() == 112);
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Firmware { operation: &'static str, status: Status },
    InvalidProtocol,
    NotStarted,
    Ipv6,
    NoServer,
    InvalidPath,
    InvalidSize,
    ChangedSize,
    Allocation(&'static str),
}

pub struct Network {
    protocol: *mut Protocol,
    server: [u8; 4],
}

impl Network {
    /// Reuse only the network device that loaded this EFI image. Probing a
    /// globally located PXE protocol could silently select another adapter.
    ///
    /// # Safety
    /// Boot services and the image's device handle must remain valid until
    /// the last call to `read`. Call only before ExitBootServices.
    pub unsafe fn open(device: Handle, bs: *mut BootServices) -> Result<Self, Error> {
        if bs.is_null() || device.is_null() { return Err(Error::InvalidProtocol); }
        let mut guid = PROTOCOL_GUID;
        let mut raw = ptr::null_mut();
        let status = unsafe { ((*bs).handle_protocol)(device, &mut guid, &mut raw) };
        if status.is_error() {
            return Err(Error::Firmware { operation: "open PXE Base Code", status });
        }
        unsafe { Self::from_protocol(raw.cast()) }
    }

    unsafe fn from_protocol(protocol: *mut Protocol) -> Result<Self, Error> {
        let protocol_ref = unsafe { protocol.as_ref() }.ok_or(Error::InvalidProtocol)?;
        let mode = unsafe { protocol_ref.mode.as_ref() }.ok_or(Error::InvalidProtocol)?;
        if !bool::from(mode.started) { return Err(Error::NotStarted); }
        if bool::from(mode.using_ipv6) { return Err(Error::Ipv6); }
        let server = server_from_mode(mode).ok_or(Error::NoServer)?;
        Ok(Self { protocol, server })
    }

    pub fn server_ipv4(&self) -> [u8; 4] { self.server }

    /// Stop firmware network activity after every boot file has been loaded.
    /// This can free firmware allocations, so call before the final memory
    /// map and ExitBootServices sequence.
    ///
    /// # Safety
    /// Firmware boot services must still be active. No network request may
    /// be running concurrently, and this must be the final use of the session.
    pub unsafe fn finish(self) -> Result<(), Error> {
        let status = unsafe { ((*self.protocol).stop)(self.protocol) };
        if status.is_error() && status != Status::NOT_STARTED {
            Err(Error::Firmware { operation: "stop PXE Base Code", status })
        } else {
            Ok(())
        }
    }

    /// Allocate and download one complete boot file. The allocator receives
    /// the number of bytes, and returns its persistent physical address.
    ///
    /// # Safety
    /// The allocation must be writable and identity-mapped for at least the
    /// requested byte count, and must outlive the kernel handoff. Firmware
    /// boot services must still be active; calls must not run concurrently.
    pub unsafe fn read(
        &self, path: &str,
        mut allocate: impl FnMut(usize) -> Result<u64, &'static str>,
    ) -> Result<&'static [u8], Error> {
        let filename = tftp_filename(path)?;
        let mut server = IpAddress { addr: [0; 4] };
        // Zero the union first so all 16 bytes have defined contents.
        server.v4.addr = self.server;
        let block_size = BLOCK_BYTES;
        let mut size = 0u64;
        let mtftp = unsafe { (*self.protocol).mtftp };
        let status = unsafe { mtftp(
            self.protocol, TFTP_GET_FILE_SIZE, ptr::null_mut(), Boolean::FALSE,
            &mut size, &block_size, &server, filename.as_ptr(), ptr::null(), Boolean::FALSE,
        ) };
        if status.is_error() {
            return Err(Error::Firmware { operation: "TFTP file size", status });
        }
        if size == 0 || size > MAX_FILE_BYTES { return Err(Error::InvalidSize); }
        let expected = size as usize;
        let address = allocate(expected).map_err(Error::Allocation)?;
        if address == 0 { return Err(Error::Allocation("allocator returned address zero")); }
        let status = unsafe { mtftp(
            self.protocol, TFTP_READ_FILE, address as *mut c_void, Boolean::FALSE,
            &mut size, &block_size, &server, filename.as_ptr(), ptr::null(), Boolean::FALSE,
        ) };
        if status.is_error() {
            return Err(Error::Firmware { operation: "TFTP read file", status });
        }
        if size != expected as u64 { return Err(Error::ChangedSize); }
        Ok(unsafe { core::slice::from_raw_parts(address as *const u8, expected) })
    }
}

fn tftp_filename(path: &str) -> Result<[u8; 256], Error> {
    let path = path.trim_start_matches(['/', '\\']);
    if path.is_empty() || path.len() >= 256 || !path.is_ascii() || path.as_bytes().contains(&0) {
        return Err(Error::InvalidPath);
    }
    let mut name = [0; 256];
    for (out, input) in name.iter_mut().zip(path.bytes()) {
        *out = if input == b'\\' { b'/' } else { input };
    }
    if name[..path.len()].split(|byte| *byte == b'/').any(|part| part.is_empty() || part == b"." || part == b"..") {
        return Err(Error::InvalidPath);
    }
    Ok(name)
}

fn server_from_mode(mode: &Mode) -> Option<[u8; 4]> {
    // The regular DHCP server may be the router. A PXE reply or ProxyDHCP
    // offer identifies the boot server ahead of the address-lease ACK.
    for (valid, packet) in [
        (mode.pxe_reply_received, &mode.pxe_reply),
        (mode.proxy_offer_received, &mode.proxy_offer),
        (mode.dhcp_ack_received, &mode.dhcp_ack),
    ] {
        if bool::from(valid) {
            if let Some(server) = server_from_packet(packet) { return Some(server); }
        }
    }
    None
}

fn usable_ipv4(ip: [u8; 4]) -> Option<[u8; 4]> {
    if ip[0] == 0 || ip[0] == 127 || ip[0] >= 224 { None } else { Some(ip) }
}

fn literal_ipv4(value: &[u8]) -> Option<[u8; 4]> {
    let value = value.strip_suffix(&[0]).unwrap_or(value);
    let mut ip = [0u8; 4];
    let mut parts = value.split(|byte| *byte == b'.');
    for octet in &mut ip {
        let part = parts.next()?;
        if part.is_empty() || part.len() > 3 { return None; }
        let mut number = 0u16;
        for byte in part {
            if !byte.is_ascii_digit() { return None; }
            number = number * 10 + u16::from(*byte - b'0');
        }
        *octet = u8::try_from(number).ok()?;
    }
    if parts.next().is_some() { return None; }
    usable_ipv4(ip)
}

/// Read only a literal server address from DHCP option 66. Option 54 is the
/// address-lease server and deliberately does not select a TFTP server.
fn options(mut bytes: &[u8]) -> Option<(Option<[u8; 4]>, u8)> {
    let mut server = None;
    let mut overload = 0;
    while let Some((&code, remaining)) = bytes.split_first() {
        bytes = remaining;
        match code { 0 => continue, 255 => break, _ => {} }
        let (&length, remaining) = bytes.split_first()?;
        let length = usize::from(length);
        if length > remaining.len() { return None; }
        let (value, remaining) = remaining.split_at(length);
        bytes = remaining;
        if code == 66 && server.is_none() { server = literal_ipv4(value); }
        if code == 52 && value.len() == 1 { overload = value[0] & 3; }
    }
    Some((server, overload))
}

fn server_from_packet(packet: &Packet) -> Option<[u8; 4]> {
    let raw = &packet.raw;
    if raw[0] != 2 { return None; } // BOOTREPLY
    if let Some(ip) = usable_ipv4(raw[20..24].try_into().ok()?) { return Some(ip); }
    if raw[236..240] != [99, 130, 83, 99] { return None; }
    let (server, overload) = options(&raw[240..])?;
    if server.is_some() { return server; }
    // RFC 2132 option-overload: bit 0 uses bootfile, bit 1 server-name.
    if overload & 1 != 0 {
        if let Some((Some(ip), _)) = options(&raw[108..236]) { return Some(ip); }
    }
    if overload & 2 != 0 {
        if let Some((Some(ip), _)) = options(&raw[44..108]) { return Some(ip); }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate alloc;
    use self::alloc::{boxed::Box, vec, vec::Vec};

    fn packet(server: [u8; 4]) -> Packet {
        let mut packet = Packet { raw: [0; 1472] };
        packet.raw[0] = 2;
        packet.raw[20..24].copy_from_slice(&server);
        packet.raw[236..240].copy_from_slice(&[99, 130, 83, 99]);
        packet
    }

    fn mode() -> Mode { unsafe { mem::zeroed() } }

    #[test]
    fn proxy_and_pxe_replies_take_priority_over_router_dhcp() {
        let mut mode = mode();
        mode.dhcp_ack_received = Boolean::TRUE;
        mode.dhcp_ack = packet([192, 168, 1, 1]);
        mode.proxy_offer_received = Boolean::TRUE;
        mode.proxy_offer = packet([192, 168, 1, 20]);
        mode.pxe_reply = packet([192, 168, 1, 30]);
        assert_eq!(server_from_mode(&mode), Some([192, 168, 1, 20]));
        mode.pxe_reply_received = Boolean::TRUE;
        assert_eq!(server_from_mode(&mode), Some([192, 168, 1, 30]));
        mode.pxe_reply = packet([0; 4]);
        assert_eq!(server_from_mode(&mode), Some([192, 168, 1, 20]));
    }

    #[test]
    fn option_66_accepts_literal_ip_and_never_uses_dhcp_server_identifier() {
        let mut packet = packet([0; 4]);
        let data = [54, 4, 192, 168, 1, 1, 66, 12, b'1', b'9', b'2', b'.', b'1', b'6', b'8', b'.', b'1', b'.', b'2', b'0', 255];
        packet.raw[240..240 + data.len()].copy_from_slice(&data);
        assert_eq!(server_from_packet(&packet), Some([192, 168, 1, 20]));
        packet.raw[246] = 255;
        assert_eq!(server_from_packet(&packet), None);
        assert_eq!(literal_ipv4(b"boot-server.example"), None);
        assert_eq!(literal_ipv4(b"192.168.1.256"), None);
    }

    #[test]
    fn option_overload_and_malformed_option_lengths_are_bounded() {
        let mut packet = packet([0; 4]);
        packet.raw[240..244].copy_from_slice(&[52, 1, 1, 255]);
        let data = b"\x42\x08"; // option 66, eight bytes
        packet.raw[108..110].copy_from_slice(data);
        packet.raw[110..118].copy_from_slice(b"10.1.2.3");
        packet.raw[118] = 255;
        assert_eq!(server_from_packet(&packet), Some([10, 1, 2, 3]));
        assert_eq!(options(&[66, 9, b'1']), None);
        assert_eq!(options(&[66]), None);
        for code in 0..=255u8 {
            let _ = options(&[code]);
            let _ = options(&[code, 255]);
        }
    }

    #[test]
    fn filesystem_paths_become_root_relative_tftp_names() {
        let name = tftp_filename("\\EFI\\BOOT\\tgl_guc_70.bin").unwrap();
        assert_eq!(&name[..24], b"EFI/BOOT/tgl_guc_70.bin\0");
        for path in ["", "/", "x\0y", "é", "../TRUEOS.elf", "EFI//BOOT", "EFI/./BOOT"] {
            assert_eq!(tftp_filename(path), Err(Error::InvalidPath));
        }
    }

    #[repr(C)]
    struct Mock {
        protocol: Protocol,
        calls: Vec<(i32, [u8; 4], Vec<u8>)>,
        size: u64,
        reported_read_size: u64,
        fail_operation: Option<i32>,
        stopped: bool,
        stop_status: Status,
    }

    unsafe extern "efiapi" fn stop(protocol: *mut Protocol) -> Status {
        let mock = unsafe { &mut *protocol.cast::<Mock>() };
        mock.stopped = true;
        mock.stop_status
    }

    unsafe extern "efiapi" fn mtftp(
        protocol: *mut Protocol, operation: i32, buffer: *mut c_void,
        overwrite: Boolean, size: *mut u64, block: *const usize,
        server: *const IpAddress, filename: *const u8,
        info: *const c_void, discard: Boolean,
    ) -> Status {
        let mock = unsafe { &mut *protocol.cast::<Mock>() };
        assert!(!bool::from(overwrite));
        assert!(!bool::from(discard));
        assert!(info.is_null());
        assert_eq!(unsafe { *block }, 1400);
        let mut filename_bytes = Vec::new();
        for index in 0..256 {
            let byte = unsafe { *filename.add(index) };
            if byte == 0 { break; }
            filename_bytes.push(byte);
        }
        mock.calls.push((operation, unsafe { (*server).v4.addr }, filename_bytes));
        if mock.fail_operation == Some(operation) { return Status::TFTP_ERROR; }
        if operation == TFTP_GET_FILE_SIZE {
            assert!(buffer.is_null());
            unsafe { *size = mock.size; }
        } else {
            assert_eq!(unsafe { *size }, mock.size);
            for index in 0..mock.size as usize { unsafe { *buffer.cast::<u8>().add(index) = index as u8; } }
            unsafe { *size = mock.reported_read_size; }
        }
        Status::SUCCESS
    }

    fn mock(size: u64) -> (Box<Mock>, Box<Mode>) {
        let mut mode = Box::new(mode());
        mode.started = Boolean::TRUE;
        mode.proxy_offer_received = Boolean::TRUE;
        mode.proxy_offer = packet([10, 1, 2, 3]);
        let protocol = Protocol {
            revision: 0x10000, start: 0, stop, dhcp: 0, discover: 0, mtftp,
            udp_write: 0, udp_read: 0, set_ip_filter: 0, arp: 0, set_parameters: 0,
            set_station_ip: 0, set_packets: 0, mode: &*mode,
        };
        (Box::new(Mock { protocol, calls: Vec::new(), size, reported_read_size: size,
            fail_operation: None, stopped: false, stop_status: Status::SUCCESS }), mode)
    }

    #[test]
    fn firmware_calls_download_exact_file_using_one_server() {
        let (mut mock, _mode) = mock(8);
        let network = unsafe { Network::from_protocol(&mut mock.protocol) }.unwrap();
        assert_eq!(network.server_ipv4(), [10, 1, 2, 3]);
        let data = unsafe { network.read("\\TRUEOS.elf", |bytes| {
            Ok(Box::leak(vec![0u8; bytes].into_boxed_slice()).as_mut_ptr() as u64)
        }) }.unwrap();
        assert_eq!(data, &[0, 1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(mock.calls, vec![
            (TFTP_GET_FILE_SIZE, [10, 1, 2, 3], b"TRUEOS.elf".to_vec()),
            (TFTP_READ_FILE, [10, 1, 2, 3], b"TRUEOS.elf".to_vec()),
        ]);
    }

    #[test]
    fn firmware_errors_invalid_sizes_and_changed_files_are_reported() {
        for size in [0, MAX_FILE_BYTES + 1] {
            let (mut mock, _mode) = mock(size);
            let network = unsafe { Network::from_protocol(&mut mock.protocol) }.unwrap();
            assert_eq!(unsafe { network.read("TRUEOS.elf", |_| panic!("invalid size must not allocate")) }, Err(Error::InvalidSize));
        }
        let (mut mock, _mode) = mock(8);
        let network = unsafe { Network::from_protocol(&mut mock.protocol) }.unwrap();
        mock.fail_operation = Some(TFTP_GET_FILE_SIZE);
        assert_eq!(unsafe { network.read("TRUEOS.elf", |_| panic!("size query failed")) },
            Err(Error::Firmware { operation: "TFTP file size", status: Status::TFTP_ERROR }));
        mock.fail_operation = Some(TFTP_READ_FILE);
        let allocate = |bytes| Ok(Box::leak(vec![0u8; bytes].into_boxed_slice()).as_mut_ptr() as u64);
        assert_eq!(unsafe { network.read("TRUEOS.elf", allocate) },
            Err(Error::Firmware { operation: "TFTP read file", status: Status::TFTP_ERROR }));
        mock.fail_operation = None;
        mock.reported_read_size = 7;
        assert_eq!(unsafe { network.read("TRUEOS.elf", allocate) }, Err(Error::ChangedSize));
    }

    #[test]
    fn inactive_ipv6_or_serverless_sessions_are_rejected() {
        let (mut mock, mut mode) = mock(8);
        mode.started = Boolean::FALSE;
        assert!(matches!(unsafe { Network::from_protocol(&mut mock.protocol) }, Err(Error::NotStarted)));
        mode.started = Boolean::TRUE;
        mode.using_ipv6 = Boolean::TRUE;
        assert!(matches!(unsafe { Network::from_protocol(&mut mock.protocol) }, Err(Error::Ipv6)));
        mode.using_ipv6 = Boolean::FALSE;
        mode.proxy_offer_received = Boolean::FALSE;
        assert!(matches!(unsafe { Network::from_protocol(&mut mock.protocol) }, Err(Error::NoServer)));
    }

    #[test]
    fn finishing_stops_network_and_reports_firmware_failure() {
        for status in [Status::SUCCESS, Status::NOT_STARTED, Status::DEVICE_ERROR] {
            let (mut mock, _mode) = mock(8);
            let network = unsafe { Network::from_protocol(&mut mock.protocol) }.unwrap();
            mock.stop_status = status;
            let result = unsafe { network.finish() };
            assert!(mock.stopped);
            if status == Status::DEVICE_ERROR {
                assert_eq!(result, Err(Error::Firmware { operation: "stop PXE Base Code", status }));
            } else {
                assert_eq!(result, Ok(()));
            }
        }
    }
}
