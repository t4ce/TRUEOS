//! Central registry for network port numbers used by the kernel.
//!
//! Keep protocol field offsets and MMIO "port" register names local to their
//! modules. This file is only for TCP/UDP service and probe ports.

pub mod well_known {
    pub const DNS: u16 = 53;
    pub const HTTP: u16 = 80;
    pub const SNTP: u16 = 123;
    pub const POP3_TLS: u16 = 995;
    pub const SMTP_SUBMISSION: u16 = 587;
    pub const IRC: u16 = 6667;
    pub const IRC_TLS: u16 = 6697;
    pub const DHCPV6_CLIENT: u16 = 546;
    pub const DHCPV6_SERVER: u16 = 547;
}

pub mod services {
    use super::well_known;

    pub const LOGTOTCP_TCP_PORT: u16 = 1;
    pub const WS_TIME_TCP_PORT: u16 = 2;
    pub const NET_SHELL_TCP_PORT: u16 = 4245;
    pub const FTP_SERVER_PORT: u16 = 21;
    pub const FTP_SERVER_PASV_MIN: u16 = 40_000;
    pub const FTP_SERVER_PASV_MAX: u16 = 40_127;
    pub const VM_STORE_REPL_PORT: u16 = 32_123;
    pub const HTTP_TRUEOSFS_TCP_PORT: u16 = well_known::HTTP;
}

pub mod esp {
    pub const UDP_BROADCAST_PORT: u16 = 32_343;
    pub const HTTP_UPLOAD_PORT: u16 = 8080;
}

pub mod probes {
    pub const VNET_PROBE_PORT: u16 = 48_123;
    pub const MIO_NET_PROBE_PORT: u16 = 48_124;
    pub const TOKIO_NET_PROBE_PORT: u16 = 48_125;
}
