//! Central registry for network port numbers used by the kernel.
//!
//! Keep protocol field offsets and MMIO "port" register names local to their
//! modules. This file is for TCP/UDP service ports and local network endpoints.

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
    pub const CHAT_HTTP_TCP_PORT: u16 = 3;
    pub const MAIL_HTTP_TCP_PORT: u16 = 4;
    pub const LOGTOTCP_TCP_PORT: u16 = 1;
    pub const WS_TIME_TCP_PORT: u16 = 2;
    pub const NET_SHELL_TCP_PORT: u16 = 4245;
    pub const FTP_SERVER_PORT: u16 = 21;
    pub const FTP_SERVER_PASV_MIN: u16 = 40_000;
    pub const FTP_SERVER_PASV_MAX: u16 = 40_127;
    pub const VM_STORE_REPL_PORT: u16 = 32_123;
    pub const HTTP_TRUEOSFS_TCP_PORT: u16 = well_known::HTTP;
    pub const LOCALCODER_WEB_TCP_PORT: u16 = 81;
}

pub mod esp {
    pub const UDP_BROADCAST_PORT: u16 = 32_343;
    pub const HTTP_UPLOAD_PORT: u16 = 8080;
    pub const TRUEOS_PEER_TCP_PORT: u16 = 32_344;
}

pub mod local_assets {
    pub const HTTP_HOST: &str = "192.168.178.111";
    pub const HTTP_PORT: u16 = super::esp::HTTP_UPLOAD_PORT;
    pub const HTTP_BASE_URL: &str = "http://192.168.178.111:8080";

    pub const TINYLLAMA_MODEL_URL: &str =
        "http://192.168.178.111:8080/tools/tinyllama/model.safetensors";
    pub const TINYLLAMA_TOKENIZER_URL: &str =
        "http://192.168.178.111:8080/tools/tinyllama/tokenizer.json";
    pub const DEMO_YELLY_MP4_URL: &str = "http://192.168.178.111:8080/tools/vid/demo_yelly.mp4";
    pub const AUDIO_DEMO_URL: &str = "http://192.168.178.111:8080/tools/aud/demo.wav";
}

pub mod probes {
    pub const VNET_PROBE_PORT: u16 = 48_123;
    pub const MIO_NET_PROBE_PORT: u16 = 48_124;
    pub const TOKIO_NET_PROBE_PORT: u16 = 48_125;
}
