// One ordered declaration generates the record levels, filters, sets and lists.
// Off is a filter state only; it can never be emitted as a record.
macro_rules! log_levels {
    ($($level:ident => $set:ident, $tag:literal);+ $(;)?) => {
        #[repr(u8)]
        #[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
        pub enum LogLevel { $($level),+ }
        #[repr(u8)]
        #[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
        pub enum LogLevelFilter { Off, $($level),+ }
        impl LogLevel {
            pub const ALL: &'static [Self] = &[$(Self::$level),+];
            pub const fn tag(self) -> &'static str {
                match self { $(Self::$level => $tag),+ }
            }
            pub const fn filter(self) -> LogLevelFilter {
                match self { $(Self::$level => LogLevelFilter::$level),+ }
            }
            pub const fn set(self) -> LogLevelSet { LogLevelSet(1 << self as u8) }
        }
        impl LogLevelFilter {
            pub const ALL: &'static [Self] = &[Self::Off, $(Self::$level),+];
            pub const fn level(self) -> Option<LogLevel> {
                match self { Self::Off => None, $(Self::$level => Some(LogLevel::$level)),+ }
            }
            pub const fn tag(self) -> &'static str {
                match self { Self::Off => "none", $(Self::$level => $tag),+ }
            }
        }
        impl LogLevelSet { $(pub const $set: Self = LogLevel::$level.set();)+ }
    };
}
log_levels! {
    Error => ERROR, "error";
    User => USER, "user";
    Ai => AI, "ai";
    Important => IMPORTANT, "important";
    Warn => WARN, "warn";
    Once => ONCE, "once";
    Info => INFO, "info";
    Debug => DEBUG, "debug";
    Trace => TRACE, "trace";
}

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
    pub const ALL: [Self; 16] = [
        Self::Global,
        Self::Boot,
        Self::Service,
        Self::Net,
        Self::Usb,
        Self::Storage,
        Self::Gfx,
        Self::Gpgpu,
        Self::Render,
        Self::Hda,
        Self::Hv,
        Self::Apps,
        Self::ExecutorRealm,
        Self::ExecutorCache,
        Self::IntelMediaNgin,
        Self::Blueprint,
    ];

    pub const fn set(self) -> LogAreaSet {
        LogAreaSet(1 << self.index())
    }

    pub const fn tag(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Boot => "boot",
            Self::Service => "service",
            Self::Net => "net",
            Self::Usb => "usb",
            Self::Storage => "storage",
            Self::Gfx => "gfx",
            Self::Gpgpu => "gpgpu",
            Self::Render => "render",
            Self::Hda => "hda",
            Self::Hv => "hv",
            Self::Apps => "apps",
            Self::ExecutorRealm => "executor-realm",
            Self::ExecutorCache => "executor-cache",
            Self::IntelMediaNgin => "intel-media",
            Self::Blueprint => "blueprint",
        }
    }

    pub const fn index(self) -> u32 {
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
pub struct LogLevelSet(u16);

impl LogLevelSet {
    pub const NONE: Self = Self(0);
    pub const ALL: Self = Self((1 << LogLevel::ALL.len()) - 1);

    pub const fn bits(self) -> u16 {
        self.0
    }
    pub const fn from_bits(bits: u16) -> Self {
        Self(bits & Self::ALL.0)
    }
    pub const fn toggle(self, level: LogLevel) -> Self {
        Self(self.0 ^ level.set().0)
    }

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn contains(self, level: LogLevel) -> bool {
        (self.0 & level.set().0) != 0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogLevelPolicy {
    Up(LogLevelFilter),
    Down(LogLevelFilter),
    Only(LogLevelSet),
}

impl LogLevelPolicy {
    pub const fn up(level: LogLevelFilter) -> Self {
        Self::Up(level)
    }

    pub const fn down(level: LogLevelFilter) -> Self {
        Self::Down(level)
    }

    pub const fn only(levels: LogLevelSet) -> Self {
        Self::Only(levels)
    }
}

pub const DEFAULT_AREA_LOG_POLICY: LogLevelPolicy = LogLevelPolicy::up(LogLevelFilter::Info);

pub const fn default_area_log_policy(_area: LogArea) -> LogLevelPolicy {
    DEFAULT_AREA_LOG_POLICY
}

pub const fn area_tag(area: LogArea) -> &'static str {
    area.tag()
}

pub const fn threshold_up_set(filter: LogLevelFilter) -> LogLevelSet {
    match filter.level() {
        None => LogLevelSet::NONE,
        Some(level) => LogLevelSet::from_bits((1 << (level as u8 + 1)) - 1),
    }
}

pub const fn threshold_down_set(filter: LogLevelFilter) -> LogLevelSet {
    match filter.level() {
        None => LogLevelSet::NONE,
        Some(level) => LogLevelSet::from_bits(!((1 << level as u8) - 1)),
    }
}

pub fn target_log_area(target: &str) -> LogArea {
    match target {
        "global" | "ui4" => LogArea::Global,
        "boot" | "cpu" | "tokio" | "rapl" | "acpi" | "aml" => LogArea::Boot,
        "service" | "spawn-svc" | "http" => LogArea::Service,
        "net" | "dns" | "dhcp" | "tls" | "icmp" => LogArea::Net,
        "usb" | "usb-if" | "usb_if" | "crabusb" | "crab-usb" => LogArea::Usb,
        "fs" | "storage" | "trueosfs" | "nvme" => LogArea::Storage,
        "gfx" | "intel" | "display" | "ui3" | "png" => LogArea::Gfx,
        "gpgpu" | "intel/gpgpu" | "opencl" | "intel/opencl" | "adls" => LogArea::Gpgpu,
        "render" | "intel/render" | "scratch" => LogArea::Render,
        "media" | "intel-media" | "intel/media" | "intel/media2" | "intel/media-encode"
        | "intel/hw_pic" | "intel/hw_pic-stage" => LogArea::IntelMediaNgin,
        "hda" => LogArea::Hda,
        "audio" => LogArea::Apps,
        "hv" | "hyperv" | "hypervisor" => LogArea::Hv,
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
    if path_prefix(path, "acpi") || path_prefix(path, "aml") {
        return LogArea::Boot;
    }
    if path_prefix(path, "png") {
        return LogArea::Gfx;
    }
    if path_prefix(path, "usb3")
        || path_prefix(path, "usb")
        || path_prefix(path, "usb_if")
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
    if path_prefix(path, "intel::gpgpu") || path_prefix(path, "intel::opencl") {
        return LogArea::Gpgpu;
    }
    if path_prefix(path, "intel::render") {
        return LogArea::Render;
    }
    if path_prefix(path, "intel") || path_prefix(path, "gfx") || path_prefix(path, "ui3") {
        return LogArea::Gfx;
    }
    if path_prefix(path, "hv") || path_prefix(path, "hyperv") || path_prefix(path, "hypervisor") {
        return LogArea::Hv;
    }
    if path_prefix(path, "blueprint") || path_prefix(path, "bp") {
        return LogArea::Blueprint;
    }
    if path_prefix(path, "executor_cache") {
        return LogArea::ExecutorCache;
    }
    if path_prefix(path, "r::spawn_service") || path_prefix(path, "stackkeeper") {
        return LogArea::Service;
    }
    if path_prefix(path, "shell2::cmds::run")
        || path_prefix(path, "gb_demo")
        || path_prefix(path, "unix_fd_probe")
    {
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

#[cfg(test)]
mod tests {
    use super::{
        LogArea, LogLevel, LogLevelFilter, LogLevelSet, module_path_log_area, target_log_area,
        threshold_down_set, threshold_up_set,
    };

    #[test]
    fn every_level_has_a_unique_bit_and_both_threshold_directions() {
        let mut union = LogLevelSet::NONE;
        for &level in LogLevel::ALL {
            assert!(!union.contains(level));
            union = union.union(level.set());
            assert_eq!(level.filter().level(), Some(level));
            for &other in LogLevel::ALL {
                assert_eq!(threshold_up_set(level.filter()).contains(other), other <= level);
                assert_eq!(threshold_down_set(level.filter()).contains(other), other >= level);
            }
        }
        assert_eq!(union, LogLevelSet::ALL);
        assert_eq!(LogLevel::ALL.len(), 9);
        assert!(LogLevel::Error < LogLevel::User);
        assert!(LogLevel::User < LogLevel::Ai);
        assert!(LogLevel::Ai < LogLevel::Important);
        assert_eq!(
            LogLevelSet::ALL
                .toggle(LogLevel::Trace)
                .toggle(LogLevel::Trace),
            LogLevelSet::ALL
        );
        assert_eq!(LogLevelSet::from_bits(u16::MAX), LogLevelSet::ALL);
    }

    #[test]
    fn upward_thresholds_follow_native_level_order() {
        let warn = threshold_up_set(LogLevelFilter::Warn);
        assert!(warn.contains(LogLevel::Error));
        assert!(warn.contains(LogLevel::Important));
        assert!(warn.contains(LogLevel::Warn));
        assert!(!warn.contains(LogLevel::Once));

        let info = threshold_up_set(LogLevelFilter::Info);
        assert!(info.contains(LogLevel::Once));
        assert!(info.contains(LogLevel::Info));
        assert!(!info.contains(LogLevel::Debug));
        assert_eq!(threshold_up_set(LogLevelFilter::Trace), LogLevelSet::ALL);
    }

    #[test]
    fn downward_thresholds_follow_native_level_order() {
        let once = threshold_down_set(LogLevelFilter::Once);
        assert!(!once.contains(LogLevel::Warn));
        assert!(once.contains(LogLevel::Once));
        assert!(once.contains(LogLevel::Info));
        assert!(once.contains(LogLevel::Trace));
        assert_eq!(threshold_down_set(LogLevelFilter::Error), LogLevelSet::ALL);
        assert_eq!(threshold_down_set(LogLevelFilter::Off), LogLevelSet::NONE);
    }

    #[test]
    fn routes_hypervisor_aliases_to_hv_area() {
        assert_eq!(target_log_area("hv"), LogArea::Hv);
        assert_eq!(target_log_area("hyperv"), LogArea::Hv);
        assert_eq!(target_log_area("hypervisor"), LogArea::Hv);
        assert_eq!(module_path_log_area("TRUEOS::hyperv::vmx"), LogArea::Hv);
    }

    #[test]
    fn routes_blueprint_aliases_to_blueprint_area() {
        assert_eq!(target_log_area("blueprint"), LogArea::Blueprint);
        assert_eq!(target_log_area("bp"), LogArea::Blueprint);
        assert_eq!(module_path_log_area("TRUEOS::blueprint::launcher"), LogArea::Blueprint);
    }

    #[test]
    fn routes_opencl_aliases_to_gpgpu_area() {
        assert_eq!(target_log_area("opencl"), LogArea::Gpgpu);
        assert_eq!(target_log_area("intel/opencl"), LogArea::Gpgpu);
        assert_eq!(module_path_log_area("TRUEOS::intel::opencl"), LogArea::Gpgpu);
        assert_eq!(module_path_log_area("TRUEOS::intel::opencl::registry"), LogArea::Gpgpu);
    }

    #[test]
    fn routes_ui4_to_global_area() {
        assert_eq!(target_log_area("ui4"), LogArea::Global);
    }

    #[test]
    fn routes_crabusb_crates_to_usb_area() {
        assert_eq!(target_log_area("usb_if"), LogArea::Usb);
        assert_eq!(module_path_log_area("usb_if::descriptor::parser"), LogArea::Usb);
        assert_eq!(module_path_log_area("crab_usb::backend::kmod"), LogArea::Usb);
    }

    #[test]
    fn routes_media_aliases_to_intel_media_area() {
        assert_eq!(target_log_area("intel-media"), LogArea::IntelMediaNgin);
        assert_eq!(target_log_area("intel/media"), LogArea::IntelMediaNgin);
        assert_eq!(target_log_area("intel/media-encode"), LogArea::IntelMediaNgin);
    }

    #[test]
    fn routes_acpi_and_aml_to_boot_area() {
        assert_eq!(target_log_area("acpi"), LogArea::Boot);
        assert_eq!(target_log_area("aml"), LogArea::Boot);
        assert_eq!(module_path_log_area("acpi::aml::namespace"), LogArea::Boot);
        assert_eq!(module_path_log_area("aml::parser"), LogArea::Boot);
    }

    #[test]
    fn routes_png_to_gfx_area() {
        assert_eq!(target_log_area("png"), LogArea::Gfx);
        assert_eq!(module_path_log_area("png::filter"), LogArea::Gfx);
    }
}

pub fn level_enabled(policy: LogLevelPolicy, level: LogLevel) -> bool {
    match policy {
        LogLevelPolicy::Up(filter) => threshold_up_set(filter).contains(level),
        LogLevelPolicy::Down(filter) => threshold_down_set(filter).contains(level),
        LogLevelPolicy::Only(levels) => levels.contains(level),
    }
}
