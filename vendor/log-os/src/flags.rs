use log::{Level, LevelFilter};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogArea {
    Global,
    Boot,
    Service,
    Net,
    Usb,
    Storage,
    Gfx,
    Gpgpu,
    Render,
    Hda,
    Hv,
    Apps,
    ExecutorRealm,
    ExecutorCache,
    IntelMediaNgin,
    Blueprint,
}

impl LogArea {
    pub const fn set(self) -> LogAreaSet {
        LogAreaSet(1 << self.index())
    }

    const fn index(self) -> u32 {
        match self {
            Self::Global => 0,
            Self::Boot => 1,
            Self::Service => 2,
            Self::Net => 3,
            Self::Usb => 4,
            Self::Storage => 5,
            Self::Gfx => 6,
            Self::Gpgpu => 7,
            Self::Render => 8,
            Self::Hda => 9,
            Self::Hv => 10,
            Self::Apps => 11,
            Self::ExecutorRealm => 12,
            Self::ExecutorCache => 13,
            Self::IntelMediaNgin => 14,
            Self::Blueprint => 15,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LogAreaSet(u32);

impl LogAreaSet {
    pub const NONE: Self = Self(0);
    pub const GLOBAL: Self = LogArea::Global.set();
    pub const BOOT: Self = LogArea::Boot.set();
    pub const SERVICE: Self = LogArea::Service.set();
    pub const NET: Self = LogArea::Net.set();
    pub const USB: Self = LogArea::Usb.set();
    pub const STORAGE: Self = LogArea::Storage.set();
    pub const GFX: Self = LogArea::Gfx.set();
    pub const GPGPU: Self = LogArea::Gpgpu.set();
    pub const RENDER: Self = LogArea::Render.set();
    pub const HDA: Self = LogArea::Hda.set();
    pub const HV: Self = LogArea::Hv.set();
    pub const APPS: Self = LogArea::Apps.set();
    pub const EXECUTOR_REALM: Self = LogArea::ExecutorRealm.set();
    pub const EXECUTOR_CACHE: Self = LogArea::ExecutorCache.set();
    pub const INTEL_MEDIA_NGIN: Self = LogArea::IntelMediaNgin.set();
    pub const BLUEPRINT: Self = LogArea::Blueprint.set();
    pub const ALL: Self = Self((1 << 16) - 1);

    pub const fn one(area: LogArea) -> Self {
        area.set()
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn contains(self, area: LogArea) -> bool {
        (self.0 & area.set().0) != 0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LogLevelSet(u8);

impl LogLevelSet {
    pub const NONE: Self = Self(0);
    pub const ERROR: Self = Self(1 << 0);
    pub const WARN: Self = Self(1 << 1);
    pub const INFO: Self = Self(1 << 2);
    pub const DEBUG: Self = Self(1 << 3);
    pub const TRACE: Self = Self(1 << 4);

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn contains(self, level: Level) -> bool {
        (self.0 & level_bit(level).0) != 0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogLevelPolicy {
    Up(LevelFilter),
    Down(LevelFilter),
    Only(LogLevelSet),
}

impl LogLevelPolicy {
    pub const fn up(level: LevelFilter) -> Self {
        Self::Up(level)
    }

    pub const fn down(level: LevelFilter) -> Self {
        Self::Down(level)
    }

    pub const fn only(levels: LogLevelSet) -> Self {
        Self::Only(levels)
    }
}

const fn level_bit(level: Level) -> LogLevelSet {
    match level {
        Level::Error => LogLevelSet::ERROR,
        Level::Warn => LogLevelSet::WARN,
        Level::Info => LogLevelSet::INFO,
        Level::Debug => LogLevelSet::DEBUG,
        Level::Trace => LogLevelSet::TRACE,
    }
}

pub const fn threshold_up_set(filter: LevelFilter) -> LogLevelSet {
    match filter {
        LevelFilter::Off => LogLevelSet::NONE,
        LevelFilter::Error => LogLevelSet::ERROR,
        LevelFilter::Warn => LogLevelSet::ERROR.union(LogLevelSet::WARN),
        LevelFilter::Info => LogLevelSet::ERROR
            .union(LogLevelSet::WARN)
            .union(LogLevelSet::INFO),
        LevelFilter::Debug => LogLevelSet::ERROR
            .union(LogLevelSet::WARN)
            .union(LogLevelSet::INFO)
            .union(LogLevelSet::DEBUG),
        LevelFilter::Trace => LogLevelSet::ERROR
            .union(LogLevelSet::WARN)
            .union(LogLevelSet::INFO)
            .union(LogLevelSet::DEBUG)
            .union(LogLevelSet::TRACE),
    }
}

pub const fn threshold_down_set(filter: LevelFilter) -> LogLevelSet {
    match filter {
        LevelFilter::Off => LogLevelSet::NONE,
        LevelFilter::Error => threshold_up_set(LevelFilter::Trace),
        LevelFilter::Warn => LogLevelSet::WARN
            .union(LogLevelSet::INFO)
            .union(LogLevelSet::DEBUG)
            .union(LogLevelSet::TRACE),
        LevelFilter::Info => LogLevelSet::INFO
            .union(LogLevelSet::DEBUG)
            .union(LogLevelSet::TRACE),
        LevelFilter::Debug => LogLevelSet::DEBUG.union(LogLevelSet::TRACE),
        LevelFilter::Trace => LogLevelSet::TRACE,
    }
}

pub fn target_log_area(target: &str) -> LogArea {
    match target {
        "boot" | "cpu" | "tokio" | "rapl" | "tga" => LogArea::Boot,
        "service" | "spawn-svc" | "http" => LogArea::Service,
        "net" | "dns" | "dhcp" | "tls" | "icmp" => LogArea::Net,
        "usb" | "crabusb" | "crab-usb" => LogArea::Usb,
        "fs" | "storage" | "trueosfs" | "nvme" => LogArea::Storage,
        "gfx" | "intel" | "display" | "ui3" => LogArea::Gfx,
        "gpgpu" | "intel/gpgpu" | "adls" => LogArea::Gpgpu,
        "render" | "intel/render" | "scratch" => LogArea::Render,
        "media" | "intel/media" | "intel/media2" | "intel/hw_pic" | "intel/hw_pic-stage" => {
            LogArea::IntelMediaNgin
        }
        "hda" | "audio" => LogArea::Hda,
        "hv" => LogArea::Hv,
        "apps" => LogArea::Apps,
        "blueprint" | "bp" => LogArea::Blueprint,
        "executor-cache" => LogArea::ExecutorCache,
        "executor-realm" => LogArea::ExecutorRealm,
        _ => module_path_log_area(target),
    }
}

pub fn module_path_log_area(path: &str) -> LogArea {
    let path = path.strip_prefix("TRUEOS::").unwrap_or(path);

    if path_prefix(path, "aud") {
        return LogArea::Hda;
    }
    if path_prefix(path, "usb3")
        || path_prefix(path, "usb")
        || path_prefix(path, "crab_usb")
        || path_prefix(path, "crab-usb")
    {
        return LogArea::Usb;
    }
    if path_prefix(path, "r::net") || path_prefix(path, "net") || path_prefix(path, "v") {
        return LogArea::Net;
    }
    if path_prefix(path, "r::fs")
        || path_prefix(path, "r::io")
        || path_prefix(path, "disc")
        || path_prefix(path, "pci::nvme")
    {
        return LogArea::Storage;
    }
    if path_prefix(path, "intel::media") {
        return LogArea::IntelMediaNgin;
    }
    if path_prefix(path, "intel::gpgpu") {
        return LogArea::Gpgpu;
    }
    if path_prefix(path, "intel::render") {
        return LogArea::Render;
    }
    if path_prefix(path, "intel") || path_prefix(path, "gfx") || path_prefix(path, "ui3") {
        return LogArea::Gfx;
    }
    if path_prefix(path, "hv") {
        return LogArea::Hv;
    }
    if path_prefix(path, "executor_cache") {
        return LogArea::ExecutorCache;
    }
    if path_prefix(path, "r::spawn_service") || path_prefix(path, "stackkeeper") {
        return LogArea::Service;
    }
    if path_prefix(path, "shell2::cmds::run") || path_prefix(path, "gb_demo") {
        return LogArea::Apps;
    }

    LogArea::Global
}

fn path_prefix(path: &str, prefix: &str) -> bool {
    path == prefix
        || path.strip_prefix(prefix).is_some_and(|rest| {
            rest.starts_with("::") || rest.starts_with('/') || rest.starts_with('-')
        })
}

pub fn level_enabled(policy: LogLevelPolicy, level: Level) -> bool {
    match policy {
        LogLevelPolicy::Up(filter) => threshold_up_set(filter).contains(level),
        LogLevelPolicy::Down(filter) => threshold_down_set(filter).contains(level),
        LogLevelPolicy::Only(levels) => levels.contains(level),
    }
}
