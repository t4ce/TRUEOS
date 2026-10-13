//! Live acceptance policies. Probe instrumentation remains a build choice.
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use log_os_core::{LogArea, LogLevelFilter as F, LogLevelPolicy as P, LogLevelSet};

// Keep policy mode and threshold as well as the mask, so the editor can show
// whether this is a severity threshold, verbosity threshold, or exact set.
const fn encode(policy: P) -> u32 {
    match policy {
        P::Up(level) => (1 << 16) | level as u32,
        P::Down(level) => (2 << 16) | level as u32,
        P::Only(levels) => levels.bits() as u32,
    }
}
fn decode(value: u32) -> P {
    match value >> 16 {
        1 => P::up(F::ALL[value as u16 as usize]),
        2 => P::down(F::ALL[value as u16 as usize]),
        _ => P::only(LogLevelSet::from_bits(value as u16)),
    }
}
const fn seeded() -> [AtomicU32; LogArea::ALL.len()] {
    let mut policies = [const { AtomicU32::new(0) }; LogArea::ALL.len()];
    let mut index = 0;
    while index < policies.len() {
        policies[index] =
            AtomicU32::new(encode(super::flags::build_area_log_policy(LogArea::ALL[index])));
        index += 1;
    }
    policies
}
static POLICIES: [AtomicU32; LogArea::ALL.len()] = seeded();
static PROFILE: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn area_log_policy(area: LogArea) -> P {
    decode(POLICIES[area.index() as usize].load(Ordering::Acquire))
}
pub(crate) fn set_area_log_policy(area: LogArea, policy: P) {
    POLICIES[area.index() as usize].store(encode(policy), Ordering::Release);
    PROFILE.store(usize::MAX, Ordering::Release);
}

#[derive(Clone, Copy)]
pub(crate) enum Profile {
    Build,
    Normal,
    TglGpu,
    Archive,
    BlueprintHv,
    FontWarm,
    UsbRuntime,
    UsbFull,
    EarlyBoot,
    HttpFetch,
    WifiPci,
    LumenPerf,
    Ui4,
    Shell2Render,
    QuadTexture,
    HelioGfx,
}
impl Profile {
    pub(crate) const ALL: [Self; 16] = [
        Self::Build,
        Self::Normal,
        Self::TglGpu,
        Self::Archive,
        Self::BlueprintHv,
        Self::FontWarm,
        Self::UsbRuntime,
        Self::UsbFull,
        Self::EarlyBoot,
        Self::HttpFetch,
        Self::WifiPci,
        Self::LumenPerf,
        Self::Ui4,
        Self::Shell2Render,
        Self::QuadTexture,
        Self::HelioGfx,
    ];
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Build => "Build defaults",
            Self::Normal => "Normal",
            Self::TglGpu => "TGL GPU",
            Self::Archive => "Archive",
            Self::BlueprintHv => "Blueprint / HV",
            Self::FontWarm => "Font warm",
            Self::UsbRuntime => "USB runtime",
            Self::UsbFull => "USB full / UAS",
            Self::EarlyBoot => "EarlyBoot",
            Self::HttpFetch => "HTTP fetch",
            Self::WifiPci => "Wi-Fi PCI",
            Self::LumenPerf => "Lumen performance",
            Self::Ui4 => "UI4 planes",
            Self::Shell2Render => "Shell2 render",
            Self::QuadTexture => "QuadTexture",
            Self::HelioGfx => "Helio graphics",
        }
    }
    /// Standalone resolved matrices; selecting one replaces the previous preset.
    pub(crate) const fn policy(self, area: LogArea) -> P {
        use LogArea as A;
        if matches!(self, Self::Build) {
            return super::flags::build_area_log_policy(area);
        }
        let level = match (self, area) {
            (Self::Normal, A::Storage | A::Net | A::Usb | A::Apps | A::Blueprint) => F::Info,
            (Self::TglGpu | Self::FontWarm, A::Gfx | A::Gpgpu) => F::Info,
            (Self::TglGpu | Self::FontWarm, A::Render | A::Boot | A::Service) => F::Once,
            (Self::Archive, A::Apps | A::Storage) => F::Info,
            (Self::BlueprintHv, A::Hv | A::Blueprint) => F::Trace,
            (Self::BlueprintHv, A::Global) => F::Info,
            (Self::UsbRuntime, A::Usb) => F::Info,
            (Self::UsbFull, A::Usb | A::Global) => F::Trace,
            (Self::UsbFull, A::Boot | A::Storage) => F::Info,
            (Self::EarlyBoot, A::Boot) => F::Trace,
            (Self::EarlyBoot, A::Global | A::Net | A::Service | A::Usb | A::Storage) => F::Info,
            (Self::HttpFetch | Self::WifiPci, A::Net) => F::Info,
            (Self::LumenPerf, A::Global | A::Service | A::Gpgpu) => F::Info,
            (Self::Ui4, A::Global) => F::Trace,
            (Self::Shell2Render | Self::QuadTexture, A::Render | A::Global) => F::Info,
            (Self::HelioGfx, A::Gfx | A::Gpgpu | A::Render) => F::Debug,
            _ => F::Warn,
        };
        P::up(level)
    }
}
pub(crate) fn profile_name() -> &'static str {
    Profile::ALL
        .get(PROFILE.load(Ordering::Acquire))
        .map_or("Custom", |p| p.name())
}
pub(crate) fn apply_profile(index: usize) {
    let Some(profile) = Profile::ALL.get(index) else {
        return;
    };
    for area in LogArea::ALL {
        POLICIES[area.index() as usize].store(encode(profile.policy(area)), Ordering::Release);
    }
    PROFILE.store(index, Ordering::Release);
}
