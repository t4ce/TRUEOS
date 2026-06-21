//! Central registry for network port numbers used by the kernel.
//!
//! Keep protocol field offsets and MMIO "port" register names local to their
//! modules. This file is for TCP/UDP service ports and local network endpoints.

pub mod well_known {
    pub const DNS: u16 = 53;
    pub const HTTP: u16 = 80;
    pub const MDNS: u16 = 5353;
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
    pub const CHROME_UNSAFE_HTTP_TCP_PORTS: [u16; 2] = [7, 9];

    const fn chrome_safe_http_port(port: u16) -> u16 {
        match port {
            7 | 9 => panic!("Chrome blocks this HTTP TCP port"),
            _ => port,
        }
    }

    pub const CHAT_HTTP_TCP_PORT: u16 = 3;
    pub const MAIL_HTTP_TCP_PORT: u16 = 4;
    pub const AXUM_BOOT_TCP_PORT: u16 = 5;
    pub const WEBDEVICES_HTTP_TCP_PORT: u16 = chrome_safe_http_port(10);
    pub const FILEEXPLORER_HTTP_TCP_PORT: u16 = chrome_safe_http_port(8);
    pub const FILEEXPLORER_HTTP_TCP_PORTS: [u16; 2] = [FILEEXPLORER_HTTP_TCP_PORT, 6];
    pub const LOGTOTCP_TCP_PORT: u16 = 1;
    pub const WS_TIME_TCP_PORT: u16 = 2;
    pub const TRUEOS_RDP_TCP_PORT: u16 = 100;
    pub const TRUEOS_HID_UDP_PORT: u16 = TRUEOS_RDP_TCP_PORT;
    pub const NET_SHELL_TCP_PORT: u16 = 4245;
    pub const FTP_SERVER_PORT: u16 = 21;
    pub const FTP_SERVER_PASV_MIN: u16 = 40_000;
    pub const FTP_SERVER_PASV_MAX: u16 = 40_127;
    pub const VM_STORE_REPL_PORT: u16 = 32_123;
    pub const TRUEOS_DISCOVERY_UDP_PORT: u16 = 32_343;
    pub const HTTP_TRUEOSFS_TCP_PORT: u16 = well_known::HTTP;
    pub const LOCALCODER_WEB_TCP_PORT: u16 = 81;
    pub const TINYAUDIO_LIVE_HTTP_TCP_PORT: u16 = 82;
    pub const GAMESERVER_TACTICS_TCP_PORT: u16 = 1337;
    pub const SPOTIFY_CONNECT_HTTP_TCP_PORT: u16 = 57_621;
}

pub mod mail {
    use super::well_known;

    pub const ACCOUNT_EMAIL: &str = "jonasb@post.com";
    pub const ACCOUNT_PASSWORD: &str = "Ttest1001";

    pub const SMTP_HOST: &str = "smtp.mail.com";
    pub const SMTP_PORT: u16 = well_known::SMTP_SUBMISSION;
    pub const SMTP_EHLO_DOMAIN: &str = "post.com";

    pub const POP3_HOST: &str = "pop.mail.com";
    pub const POP3_PORT: u16 = well_known::POP3_TLS;
}

pub mod esp {
    pub const UDP_BROADCAST_PORT: u16 = super::services::TRUEOS_DISCOVERY_UDP_PORT;
    pub const HTTP_UPLOAD_PORT: u16 = 8080;
    pub const TRUEOS_PEER_TCP_PORT: u16 = 32_344;
}

pub mod local_assets {
    pub const HTTP_HOST: &str = "192.168.178.111";
    pub const HTTP_PORT: u16 = super::esp::HTTP_UPLOAD_PORT;
    pub const HTTP_BASE_URL: &str = "http://192.168.178.111:8080";

    pub const DEMO_YELLY_MP4_URL: &str =
        "http://192.168.178.111:8080/tools/vid/trueos_h264_diag_mbgrid_2560x1440.mp4";
    pub const AUDIO_DEMO_URL: &str = "http://192.168.178.111:8080/tools/aud/demo.wav";
    pub const AUDIO_DEMO_CACHE_PATH: &str = "audio/demo.wav";
}

pub mod probes {
    pub const VNET_PROBE_PORT: u16 = 48_123;
    pub const MIO_NET_PROBE_PORT: u16 = 48_124;
    pub const TOKIO_NET_PROBE_PORT: u16 = 48_125;
}
