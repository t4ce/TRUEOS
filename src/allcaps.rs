//! Central kernel policy caps and soft limits.
//!
//! Keep hardware register offsets, protocol opcodes, bit flags, and wire-format
//! constants in their driver/protocol modules. Put tunable resource budgets,
//! queue depths, ring sizes, stack sizes, retry limits, and service timing here.

pub mod boot {
    pub const BSP_BOOT_STACK_BYTES: usize = 8 * 1024 * 1024;
}

pub mod probes {
    pub const TOKIO_BOOT_PROBE: bool = true;
    pub const TOKIO_SECURE_DNS_BOOT_PROBE: bool = false;
    pub const TOKIO_NET_WRITABLE_TIMEOUT_MS: u64 = 1000;
}

pub mod lumen {
    pub const RUNTIME_STATIC_HI_WARM_PROBE: bool = false;
}

pub mod blueprint {
    pub const PORTAL_IMAGE_CAP_BYTES: usize = 16 * 1024 * 1024;
    pub const PORTAL_IMAGE_COMPAT_SLOT_BYTES: usize = 4 * 1024 * 1024;
    pub const PORTAL_IMAGE_COMPAT_SLOT_COUNT: usize = 4;
}

pub mod stackkeeper {
    pub const TOKIO_LANE_COUNT: usize = 16;
    pub const TOKIO_LANE_SCRATCH_BYTES: usize = 16 * 1024;
    pub const TOKIO_TLS_CPU_TRACK_COUNT: usize = 64;
}

pub mod hv {
    pub const LOG_CAP: usize = 64;
    pub const LOG_LINE_BYTES: usize = 200;

    pub const VM_ID_LIMIT: usize = 32;
    pub const VM_TASK_POOL_SIZE: usize = VM_ID_LIMIT;
    pub const VM_CPU_SLOT_LIMIT: usize = 256;

    pub const GUEST_STACK_MIN_MIB: usize = 64;
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

    pub const MAX_SOCKETS: usize = 128;
    pub const MAX_DRAIN_PER_LOOP: usize = 128;
    pub const TCP_RX_BUF_BYTES: usize = 1024 * 1024;
    pub const TCP_TX_BUF_BYTES: usize = 1024 * 1024;
    pub const ICMP_VNET_MAX_INFLIGHT: usize = 32;
    pub const ICMP_VNET_TIMEOUT_MS: i64 = 2000;
    pub const NET_POLL_SLEEP_US: u64 = 100;
    pub const NET_SERVICE_SLEEP_US: u64 = 100;

    pub const DNS_SERVER_MAX: usize = 4;
    pub const IPV6_RS_RETRY_MS: i64 = 5_000;
    pub const MAX_NET_DEVICES: usize = 8;
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
    pub const USB_MASS_KEEPALIVE_MS: u64 = 2_000;
    pub const USB_MASS_IO_RETRY_LIMIT: u8 = 8;
    pub const USB_MASS_IO_RETRY_DELAY_MS: u64 = 25;
    pub const USB_MASS_RUNTIME_WAIT_LIMIT: u16 = 500;
    pub const USB_MASS_RUNTIME_WAIT_DELAY_MS: u64 = 10;
    pub const USB_MASS_MIN_IO_BYTES: usize = 8 * 1024;
    pub const USB_MASS_MAX_IO_BYTES: usize = 128 * 1024;
    pub const USB_MASS_IO_GROW_SUCCESS_TARGET: u16 = 16;
    pub const USB_MASS_IO_GROW_SUCCESS_TARGET_FAST_BOT: u16 = 4;
    pub const USB_MASS_FAST_BOT_INITIAL_IO_BYTES: usize = 128 * 1024;
    pub const USB_MASS_FAST_BOT_WRITE_MAX_IO_BYTES: usize = 128 * 1024;
}

pub mod input {
    pub const HID_CURSOR_EVENT_RING_CAP: usize = 2048;
    pub const HID_MOUSE_RING_CAP: usize = 2048;
    pub const HID_KEYBOARD_RING_CAP: usize = 512;
    pub const HID_TABLET_RING_CAP: usize = 1024;
}

pub mod ui_shell {
    pub const LOCALCODER_SERVICE_QUEUE_CAP: usize = 32;
    pub const LOCALCODER_SERVICE_IDLE_MS: u64 = 8;
    pub const LOCALCODER_SERVICE_STEP_MS: u64 = 12;

    pub const SHELL_MAX_LINE: usize = 192;
    pub const SHELL_SECTION_STATUS_HOLD_MS: u64 = 1000;
    pub const SHELL_SECTION_RAINBOW_FRAME_MS: u64 = 120;
}
