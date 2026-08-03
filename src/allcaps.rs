//! Central kernel policy caps and soft limits.
//!
//! Keep hardware register offsets, protocol opcodes, bit flags, and wire-format
//! constants in their driver/protocol modules. Put tunable resource budgets,
//! queue depths, ring sizes, stack sizes, retry limits, and service timing here.

pub mod boot {
    pub const BSP_BOOT_STACK_BYTES: usize = 8 * 1024 * 1024;
}

pub mod executor {
    pub const BSP_TASK_PROFILE_ENABLED: bool = true;
    pub const BSP_TASK_PROFILE_REPORT_MS: u64 = 1_000;
    pub const BSP_TASK_PROFILE_SLOW_POLL_US: u64 = 2_000;
    pub const BSP_TASK_PROFILE_SLOTS: usize = 128;
    pub const BSP_TASK_PROFILE_HISTORY_SLOTS: usize = 1_024;
    pub const BSP_TASK_PROFILE_WATCHERS: usize = 8;
}

pub mod probes {
    pub const MIO_BOOT_PROBE: bool = false;
    pub const UNIX_FD_PROBE: bool = false;
    pub const INTEL_GPGPU_ARTIFACT_BOOT_SMOKETESTS: bool = false;
    pub const TOKIO_NET_WRITABLE_TIMEOUT_MS: u64 = 1000;
}

pub mod blueprint {
    pub const PORTAL_IMAGE_CAP_BYTES: usize = 128 * 1024 * 1024;
    pub const ASSET_ARCHIVE_CAP_BYTES: usize = 1024 * 1024 * 1024;
    pub const ASSET_DECODE_CAP_BYTES: usize = 2 * 1024 * 1024 * 1024;
    pub const ASSET_7Z_DICTIONARY_CAP_BYTES: usize = 256 * 1024 * 1024;
    pub const ASSET_FILE_CAP_BYTES: usize = 512 * 1024 * 1024;
    pub const ASSET_ENTRY_COUNT_CAP: usize = 65_536;
    pub const ASSET_PATH_BYTES_CAP: usize = 4096;
    pub const ASSET_PATH_COMPONENT_BYTES_CAP: usize = 255;
    pub const ASSET_WRITE_CHUNK_BYTES: usize = 256 * 1024;
}

pub mod gfx {
    pub const SCREENSHOT_CAPTURE_ENABLED: bool = false;
}

pub mod lumen {
    /// Current single-model intensive-test profile. Disable this one switch
    /// when boot-resident inference assets are no longer the desired policy.
    pub const BOOT_RESIDENT_WARM_ENABLED: bool = false;

    /// Let the compositor enter its steady service loop before model I/O and
    /// in-place Q8 packing begin on a background performance core.
    pub const BOOT_RESIDENT_WARM_SETTLE_MS: u64 = 2_000;
}

pub mod ttstt {
    /// Keep the service available for command-driven lazy startup without
    /// reading the complete model set during normal BSP boot.
    pub const BOOT_RESIDENT_WARM_ENABLED: bool = false;
}

pub mod media_encode {
    // Experimental live-stream cadence soft cap. The fixed AVC SPS VUI timing
    // in `intel/media/avc_encode_probe.rs` must advertise the same rate.
    pub const REALTIME_HZ: usize = 40;
    pub const STREAM_MAX_ACCESS_UNIT_BYTES: usize = 4 * 1024 * 1024;
    pub const VALIDATION_SESSION_SECONDS: usize = 10;
    pub const VALIDATION_SESSION_ACCESS_UNITS: usize = REALTIME_HZ * VALIDATION_SESSION_SECONDS;
}

pub mod wls {
    pub const WORKER_SLOT_COUNT: usize = 16;
    pub const CPU_TRACK_COUNT: usize = 64;
    pub const TLS_SLOT_COUNT: usize = 4096;
}

pub mod hv {
    pub const LOG_LINE_BYTES: usize = 200;

    pub const VM_ID_LIMIT: usize = 64;
    pub const VM_CPU_SLOT_LIMIT: usize = 256;
    pub const VMX_LIFECYCLE_PREEMPTION_QUANTUM_MS: u64 = 125;

    pub const GUEST_STACK_MIN_MIB: usize = 8;
    pub const GUEST_STACK_DEFAULT_MIB: usize = 64;
    pub const GUEST_STACK_MAX_MIB: usize = 512;

    // Each 4 GiB guest heap can occupy up to five 1 GiB EPT PD slots when the
    // physical arena is not 1 GiB aligned. Keep enough metadata for the declared
    // VM limit plus fixed kernel/stack/comm/table spans.
    pub const EPT_DYNAMIC_PD_CAP: usize = 8 + VM_ID_LIMIT * 5;
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
    pub const CHECKED_UDP_SEND_RESULTS_CAP: usize = 256;
    // Idle NICs need no sub-millisecond polling. Busy polls still request an
    // immediate follow-up so this cap does not throttle active RX/TX drains.
    pub const NET_POLL_IDLE_SOFTCAP_MS: u64 = 5;
    pub const NET_SERVICE_SLEEP_US: u64 = 100;

    pub const THROUGHPUT_BENCH_AUTOSTART: bool = false;
    pub const THROUGHPUT_BENCH_START_DELAY_MS: u64 = 15_000;
    pub const THROUGHPUT_BENCH_DURATION_MS: u64 = 30_000;
    pub const THROUGHPUT_BENCH_CONNECT_TIMEOUT_MS: u64 = 10_000;
    pub const THROUGHPUT_BENCH_TX_INFLIGHT_BYTES: usize = 4 * 1024 * 1024;

    pub const DNS_SERVER_MAX: usize = 4;
    pub const IPV6_RS_RETRY_MS: i64 = 5_000;
    pub const MAX_NET_DEVICES: usize = 8;
}

pub mod usb {
    // Keep the xHCI event pump responsive, but give controller-owner maintenance
    // only a small cooperative window at a deliberately low idle cadence.
    pub const CONTROLLER_MAINTENANCE_CADENCE_MS: u64 = 100;
    pub const CONTROLLER_MAINTENANCE_BUDGET_MS: u64 = 5;
    pub const CONTROLLER_SNAPSHOT_CADENCE_MS: u64 = 5_000;
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

    pub const USB_MASS_UAS_IO_TIMEOUT_MS: u64 = 10_000;

    // BSP-owned TRUEOSFS mount/index work is throughput-insensitive at boot.
    // Deliberately trade completion time for predictable executor fairness.
    pub const TRUEOSFS_BOOT_WORK_YIELD_MS: u64 = 1;
    pub const TRUEOSFS_INDEX_CHECKPOINT_ENTRIES_PER_YIELD: usize = 64;
}

pub mod input {
    pub const HID_CURSOR_EVENT_RING_CAP: usize = 2048;
    pub const HID_MOUSE_RING_CAP: usize = 2048;
    pub const HID_KEYBOARD_RING_CAP: usize = 512;
    pub const HID_TABLET_RING_CAP: usize = 1024;
    pub const HID_UDP_DEVICE_STATE_CAP: usize = 128;
}
