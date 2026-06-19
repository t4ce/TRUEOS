//! Central kernel policy caps and soft limits.
//!
//! Keep hardware register offsets, protocol opcodes, bit flags, and wire-format
//! constants in their driver/protocol modules. Put tunable resource budgets,
//! queue depths, ring sizes, stack sizes, retry limits, and service timing here.

pub mod boot {
    pub const BSP_BOOT_STACK_BYTES: usize = 8 * 1024 * 1024;
}

pub mod probes {
    pub const MIO_BOOT_PROBE: bool = false;
    pub const HYPER_BOOT_PROBE: bool = false;
    pub const HYPER_HTTP1_NET_PROBE: bool = false;
    pub const INTEL_GPGPU_PREFLIGHT_BOOT_PROBE: bool = false;
    pub const INTEL_GPGPU_ARTIFACT_BOOT_SMOKETESTS: bool = false;
    pub const UAS_SKHYNIX_ROUTE_BOOT_PROBE: bool = true;
    pub const TOKIO_NET_WRITABLE_TIMEOUT_MS: u64 = 1000;
}

pub mod blueprint {
    pub const PORTAL_IMAGE_CAP_BYTES: usize = 16 * 1024 * 1024;
}

pub mod gfx {
    pub const SCREENSHOT_CAPTURE_ENABLED: bool = false;
}

pub mod stackkeeper {
    pub const TOKIO_LANE_COUNT: usize = 16;
    pub const TOKIO_LANE_SCRATCH_BYTES: usize = 16 * 1024;
    pub const TOKIO_TLS_CPU_TRACK_COUNT: usize = 64;
    pub const TOKIO_BLOCKING_VTHREAD_BACKING: bool = true;
}

pub mod hv {
    pub const LOG_LINE_BYTES: usize = 200;

    pub const VM_ID_LIMIT: usize = 32;
    pub const VM_CPU_SLOT_LIMIT: usize = 256;

    pub const GUEST_STACK_MIN_MIB: usize = 8;
    pub const GUEST_STACK_DEFAULT_MIB: usize = 64;
    pub const GUEST_STACK_MAX_MIB: usize = 512;

    pub const EPT_DYNAMIC_PD_CAP: usize = 16;
    pub const EPT_DYNAMIC_PT_CAP: usize = 1024;
}

pub mod net {
    pub const VNET_CMD_QUEUE_DEPTH: usize = 256;
    pub const VNET_EVENT_QUEUE_DEPTH_DEFAULT: usize = 16_384;

    pub const RX_QUEUE_SOFT_CAP: usize = 64;
    pub const PACKET_POOL_MAX: usize = 1024;
    pub const RX_BUF_SIZE: usize = 2048;

    pub const MAX_SOCKETS: usize = 512;
    pub const MAX_DRAIN_PER_LOOP: usize = 128;
    pub const TCP_RX_BUF_BYTES: usize = 1024 * 1024;
    pub const TCP_TX_BUF_BYTES: usize = 1024 * 1024;
    pub const ICMP_VNET_MAX_INFLIGHT: usize = 32;
    pub const ICMP_VNET_TIMEOUT_MS: i64 = 2000;
    pub const NET_POLL_SLEEP_US: u64 = 100;
    pub const NET_SERVICE_SLEEP_US: u64 = 100;
    pub const CABI_NET_FETCH_TASK_POOL_SIZE: usize = 16;
    pub const CABI_NET_FETCH_MAX_CONCURRENCY: usize = 8;

    pub const DNS_SERVER_MAX: usize = 4;
    pub const IPV6_RS_RETRY_MS: i64 = 5_000;
    pub const MAX_NET_DEVICES: usize = 8;

    pub const TRUEOS_SWARM_HOST_CAP: usize = 16;
    pub const TRUEOS_SWARM_PEER_LINK_CAP: usize = TRUEOS_SWARM_HOST_CAP;
    pub const TRUEOS_SWARM_PEER_RX_BUF_BYTES: usize = 1024;
}

pub mod storage {
    pub const NVME_ADMIN_TIMEOUT_MS: u64 = 1_500;
    pub const NVME_IO_TIMEOUT_MS: u64 = 5_000;
    pub const NVME_READY_TIMEOUT_MS: u64 = 5_000;
    pub const NVME_CAP_TO_GRANULARITY_MS: u64 = 500;
    pub const NVME_IO_HOT_POLL_LIMIT: usize = 16;
    pub const NVME_IO_POLL_INTERVAL_MS: u64 = 1;
    pub const NVME_QUEUE_DEPTH_CAP: u16 = 64;
    pub const NVME_IO_TRANSFER_PAGES_CAP: u64 = 128;

    pub const USB_MASS_MAX_RUNTIMES: usize = 8;
    pub const USB_MASS_MAX_ACTIVE_STREAMS: usize = 8;
    pub const USB_MASS_BOT_KEEPALIVE_ENABLED: bool = false;
    pub const USB_MASS_FORCE_CONSERVATIVE_BOT: bool = true;
    pub const USB_MASS_BOT_IO_TIMEOUT_MS: u64 = 2_000;
    pub const USB_MASS_UAS_IO_TIMEOUT_MS: u64 = 10_000;
    pub const USB_MASS_BOT_RECOVERY_SETTLE_MS: u64 = 1_000;
    pub const USB_MASS_KEEPALIVE_MS: u64 = 2_000;
    pub const USB_MASS_IO_RETRY_LIMIT: u8 = 8;
    pub const USB_MASS_IO_RETRY_DELAY_MS: u64 = 25;
    pub const USB_MASS_RUNTIME_WAIT_LIMIT: u16 = 500;
    pub const USB_MASS_RUNTIME_WAIT_DELAY_MS: u64 = 10;
    pub const USB_MASS_MIN_IO_BYTES: usize = 8 * 1024;
    pub const USB_MASS_MAX_IO_BYTES: usize = 1024 * 1024;
    pub const USB_MASS_IO_GROW_SUCCESS_TARGET: u16 = 16;
    pub const USB_MASS_IO_GROW_SUCCESS_TARGET_FAST_BOT: u16 = 4;
    pub const USB_MASS_FAST_BOT_INITIAL_IO_BYTES: usize = 1024 * 1024;
    pub const USB_MASS_FAST_BOT_WRITE_MAX_IO_BYTES: usize = 128 * 1024;
    pub const USB_MASS_UAS_SKHYNIX_ROUTE_PROBE_WRITE_ENABLED: bool = false;
    pub const USB_MASS_UAS_SKHYNIX_ROUTE_PROBE_WRITE_X2: bool = false;
    pub const USB_MASS_UAS_SKHYNIX_ROUTE_PROBE_CHUNK_BYTES: usize = 1024 * 1024;
    pub const USB_MASS_UAS_SKHYNIX_ROUTE_PROBE_MAX_INFLIGHT: usize = 1;
    pub const USB_MASS_UAS_SKHYNIX_ROUTE_PROBE_WRITE_LBA: u64 = 0;
}

pub mod input {
    pub const HID_CURSOR_EVENT_RING_CAP: usize = 2048;
    pub const HID_MOUSE_RING_CAP: usize = 2048;
    pub const HID_KEYBOARD_RING_CAP: usize = 512;
    pub const HID_TABLET_RING_CAP: usize = 1024;
    pub const HID_UDP_DEVICE_STATE_CAP: usize = 128;
}
