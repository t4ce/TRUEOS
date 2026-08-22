use core::fmt;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::flags::{LogArea, LogAreaSet, LogLevel, LogLevelPolicy, level_enabled, target_log_area};

static GLOBAL_LOG_DISPATCH: spin::Once<&'static dyn GlobalLogDispatch> = spin::Once::new();

/// Installs the process-wide native dispatcher used by the drop-in logging macros.
///
/// Installation is intentionally one-shot. Calls made before installation are
/// discarded, matching the conventional facade's pre-initialization behavior.
pub fn install_global_log_dispatch(dispatch: &'static dyn GlobalLogDispatch) {
    GLOBAL_LOG_DISPATCH.call_once(|| dispatch);
}

#[doc(hidden)]
pub fn global_log_enabled(target: &str, level: LogLevel) -> bool {
    let Some(dispatch) = GLOBAL_LOG_DISPATCH.get() else {
        return false;
    };
    dispatch.enabled(target_log_area(target), level)
}

#[doc(hidden)]
pub fn global_log_with_target_level(target: &str, level: LogLevel, args: fmt::Arguments<'_>) {
    let Some(dispatch) = GLOBAL_LOG_DISPATCH.get() else {
        return;
    };
    dispatch.emit(
        target_log_area(target),
        level,
        Some(purpose_for_level(level)),
        format_args!("{}: {}\n", target, args),
    );
}

/// Stable identity for a source-level `ONCE` logging site.
///
/// IDs are supplied explicitly by the caller and must remain stable between
/// activations. Zero is reserved for empty registry slots.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct LogSiteId(u64);

impl LogSiteId {
    pub const fn new(id: u64) -> Option<Self> {
        if id == 0 || id > (u64::MAX >> 2) {
            None
        } else {
            Some(Self(id))
        }
    }

    pub const fn get(self) -> u64 {
        self.0
    }

    /// Deterministically derives a stable ID for a source callsite.
    ///
    /// Logging macros should pass `module_path!()`, `file!()`, `line!()`, and
    /// `column!()` directly. FNV-1a is used only for stable identity, not for
    /// security; the registry still compares complete derived IDs exactly.
    pub const fn from_location(module_path: &str, file: &str, line: u32, column: u32) -> Self {
        const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

        const fn hash_bytes(mut hash: u64, bytes: &[u8]) -> u64 {
            let mut index = 0;
            while index < bytes.len() {
                hash ^= bytes[index] as u64;
                hash = hash.wrapping_mul(FNV_PRIME);
                index += 1;
            }
            hash
        }

        let mut hash = hash_bytes(FNV_OFFSET, module_path.as_bytes());
        hash = hash_bytes(hash, &[0xff]);
        hash = hash_bytes(hash, file.as_bytes());
        hash = hash_bytes(hash, &[0xfe]);
        hash = hash_bytes(hash, &line.to_le_bytes());
        hash = hash_bytes(hash, &column.to_le_bytes());
        let id = hash & (u64::MAX >> 2);
        Self(if id == 0 { 1 } else { id })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogOnceObservation {
    First,
    Duplicate,
    Repeated,
    RegistryFull,
}

/// Fixed-capacity, allocation-free state for `ONCE` log sites.
///
/// Each exact site ID is stored in an atomic slot. Hash collisions probe other
/// slots and therefore cannot merge unrelated sites.
pub struct LogOnceState<const N: usize> {
    slots: [AtomicU64; N],
}

impl<const N: usize> LogOnceState<N> {
    pub const fn new() -> Self {
        Self {
            slots: [const { AtomicU64::new(0) }; N],
        }
    }

    pub fn observe(&self, site: LogSiteId) -> LogOnceObservation {
        const SEEN: u64 = 1;
        const WARNED: u64 = 2;

        if N == 0 {
            return LogOnceObservation::RegistryFull;
        }

        let key = site.get() << 2;
        let start = (site.get().wrapping_mul(0x9e37_79b9_7f4a_7c15) as usize) % N;
        for offset in 0..N {
            let slot = &self.slots[(start + offset) % N];
            let value = slot.load(Ordering::Acquire);
            if value == 0 {
                if slot
                    .compare_exchange(0, key | SEEN, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
                {
                    return LogOnceObservation::First;
                }
                // A concurrent writer changed this slot; inspect it again.
            }

            let value = slot.load(Ordering::Acquire);
            if value >> 2 == site.get() {
                if value & 3 == SEEN
                    && slot
                        .compare_exchange(
                            key | SEEN,
                            key | WARNED,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        )
                        .is_ok()
                {
                    return LogOnceObservation::Duplicate;
                }
                return LogOnceObservation::Repeated;
            }
        }
        LogOnceObservation::RegistryFull
    }
}

impl<const N: usize> Default for LogOnceState<N> {
    fn default() -> Self {
        Self::new()
    }
}

pub trait GlobalLogSink: Sync {
    fn spec(&self) -> GlobalLogSinkSpec;

    fn level_policy(&self, _area: LogArea) -> LogLevelPolicy {
        self.spec().level
    }

    fn accepts(&self, area: LogArea, level: LogLevel) -> bool {
        self.spec().areas.contains(area) && level_enabled(self.level_policy(area), level)
    }

    fn write_accepted(
        &self,
        area: LogArea,
        level: LogLevel,
        purpose: Option<&str>,
        args: fmt::Arguments<'_>,
    );
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GlobalLogSinkSpec {
    pub areas: LogAreaSet,
    pub level: LogLevelPolicy,
}

impl GlobalLogSinkSpec {
    pub const fn new(areas: LogAreaSet, level: LogLevelPolicy) -> Self {
        Self { areas, level }
    }

    pub fn accepts(self, area: LogArea, level: LogLevel) -> bool {
        self.areas.contains(area) && level_enabled(self.level, level)
    }
}

pub trait GlobalLogDispatch: Sync {
    fn enabled(&self, area: LogArea, level: LogLevel) -> bool;
    fn emit(&self, area: LogArea, level: LogLevel, purpose: Option<&str>, args: fmt::Arguments<'_>);
}

impl<T: GlobalLogSink> GlobalLogDispatch for T {
    fn enabled(&self, area: LogArea, level: LogLevel) -> bool {
        self.accepts(area, level)
    }

    fn emit(
        &self,
        area: LogArea,
        level: LogLevel,
        purpose: Option<&str>,
        args: fmt::Arguments<'_>,
    ) {
        if self.accepts(area, level) {
            self.write_accepted(area, level, purpose, args);
        }
    }
}

pub struct GlobalLogRouter {
    sinks: &'static [&'static dyn GlobalLogSink],
}

impl GlobalLogRouter {
    pub const fn new(sinks: &'static [&'static dyn GlobalLogSink]) -> Self {
        Self { sinks }
    }
}

impl GlobalLogDispatch for GlobalLogRouter {
    fn enabled(&self, area: LogArea, level: LogLevel) -> bool {
        self.sinks.iter().any(|sink| sink.accepts(area, level))
    }

    fn emit(
        &self,
        area: LogArea,
        level: LogLevel,
        purpose: Option<&str>,
        args: fmt::Arguments<'_>,
    ) {
        for sink in self.sinks {
            if sink.accepts(area, level) {
                sink.write_accepted(area, level, purpose, args);
            }
        }
    }
}

pub fn log<D: GlobalLogDispatch>(dispatch: &D, args: fmt::Arguments<'_>) {
    log_with_area_level(dispatch, LogArea::Global, LogLevel::Info, args);
}

pub const fn purpose_for_level(level: LogLevel) -> &'static str {
    match level {
        LogLevel::Error => "error",
        LogLevel::Important => "important",
        LogLevel::Warn => "warn",
        LogLevel::Once => "once",
        LogLevel::Info => "info",
        LogLevel::Debug => "debug",
        LogLevel::Trace => "trace",
    }
}

pub fn log_with_area_level<D: GlobalLogDispatch>(
    dispatch: &D,
    area: LogArea,
    level: LogLevel,
    args: fmt::Arguments<'_>,
) {
    log_with_area_purpose(dispatch, area, level, Some(purpose_for_level(level)), args);
}

pub fn log_with_area_purpose<D: GlobalLogDispatch>(
    dispatch: &D,
    area: LogArea,
    level: LogLevel,
    purpose: Option<&str>,
    args: fmt::Arguments<'_>,
) {
    dispatch.emit(area, level, purpose, args);
}

pub fn log_with_target_purpose<D: GlobalLogDispatch>(
    dispatch: &D,
    target: &str,
    level: LogLevel,
    purpose: Option<&str>,
    args: fmt::Arguments<'_>,
) {
    let area = target_log_area(target);
    log_with_area_purpose(dispatch, area, level, purpose, args);
}

pub fn log_with_target_level<D: GlobalLogDispatch>(
    dispatch: &D,
    target: &str,
    level: LogLevel,
    args: fmt::Arguments<'_>,
) {
    let area = target_log_area(target);
    log_with_area_level(dispatch, area, level, args);
}

pub fn log_once_with_area_purpose<D: GlobalLogDispatch, const N: usize>(
    dispatch: &D,
    state: &LogOnceState<N>,
    site: LogSiteId,
    area: LogArea,
    purpose: Option<&str>,
    args: fmt::Arguments<'_>,
) -> LogOnceObservation {
    let observation = state.observe(site);
    match observation {
        LogOnceObservation::First => dispatch.emit(area, LogLevel::Once, purpose, args),
        LogOnceObservation::Duplicate => dispatch.emit(
            area,
            LogLevel::Warn,
            Some(purpose_for_level(LogLevel::Warn)),
            format_args!("ONCE site activated twice site=0x{:X} original={}", site.get(), args),
        ),
        LogOnceObservation::RegistryFull => dispatch.emit(
            area,
            LogLevel::Warn,
            Some(purpose_for_level(LogLevel::Warn)),
            format_args!("ONCE registry full site=0x{:X} original={}", site.get(), args),
        ),
        LogOnceObservation::Repeated => {}
    }
    observation
}

#[cfg(test)]
mod tests {
    use core::fmt;
    use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

    use super::{
        GlobalLogRouter, GlobalLogSink, GlobalLogSinkSpec, LogOnceObservation, LogOnceState,
        LogSiteId, log_once_with_area_purpose,
    };
    use crate::flags::{LogArea, LogAreaSet, LogLevel, LogLevelFilter, LogLevelPolicy};
    use std::thread;

    struct CountingSink {
        count: AtomicUsize,
        last_level: AtomicU8,
    }

    impl CountingSink {
        const fn new() -> Self {
            Self {
                count: AtomicUsize::new(0),
                last_level: AtomicU8::new(0),
            }
        }

        fn reset(&self) {
            self.count.store(0, Ordering::Relaxed);
            self.last_level.store(0, Ordering::Relaxed);
        }
    }

    impl GlobalLogSink for CountingSink {
        fn spec(&self) -> GlobalLogSinkSpec {
            GlobalLogSinkSpec::new(LogAreaSet::ALL, LogLevelPolicy::up(LogLevelFilter::Trace))
        }

        fn write_accepted(
            &self,
            _area: LogArea,
            level: LogLevel,
            _purpose: Option<&str>,
            _args: fmt::Arguments<'_>,
        ) {
            self.last_level.store(level as u8 + 1, Ordering::Relaxed);
            self.count.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn callsite_ids_are_stable_and_location_sensitive() {
        const FIRST: LogSiteId = LogSiteId::from_location("module", "file.rs", 10, 2);
        const SAME: LogSiteId = LogSiteId::from_location("module", "file.rs", 10, 2);
        const NEXT_LINE: LogSiteId = LogSiteId::from_location("module", "file.rs", 11, 2);
        assert_eq!(FIRST, SAME);
        assert_ne!(FIRST, NEXT_LINE);
        assert_ne!(FIRST.get(), 0);
    }

    #[test]
    fn once_decision_precedes_and_preserves_sink_fanout() {
        static LEFT: CountingSink = CountingSink::new();
        static RIGHT: CountingSink = CountingSink::new();
        static SINKS: [&'static dyn GlobalLogSink; 2] = [&LEFT, &RIGHT];
        static ROUTER: GlobalLogRouter = GlobalLogRouter::new(&SINKS);

        LEFT.reset();
        RIGHT.reset();
        let state = LogOnceState::<4>::new();
        let site = LogSiteId::new(7).unwrap();

        assert_eq!(
            log_once_with_area_purpose(
                &ROUTER,
                &state,
                site,
                LogArea::Global,
                Some("once"),
                format_args!("message"),
            ),
            LogOnceObservation::First
        );
        assert_eq!(LEFT.count.load(Ordering::Relaxed), 1);
        assert_eq!(RIGHT.count.load(Ordering::Relaxed), 1);
        assert_eq!(LEFT.last_level.load(Ordering::Relaxed), LogLevel::Once as u8 + 1);

        assert_eq!(
            log_once_with_area_purpose(
                &ROUTER,
                &state,
                site,
                LogArea::Global,
                Some("once"),
                format_args!("message"),
            ),
            LogOnceObservation::Duplicate
        );
        assert_eq!(LEFT.count.load(Ordering::Relaxed), 2);
        assert_eq!(RIGHT.count.load(Ordering::Relaxed), 2);
        assert_eq!(LEFT.last_level.load(Ordering::Relaxed), LogLevel::Warn as u8 + 1);

        assert_eq!(state.observe(site), LogOnceObservation::Repeated);
        assert_eq!(LEFT.count.load(Ordering::Relaxed), 2);
        assert_eq!(RIGHT.count.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn full_registry_is_reported() {
        let state = LogOnceState::<1>::new();
        assert_eq!(state.observe(LogSiteId::new(1).unwrap()), LogOnceObservation::First);
        assert_eq!(state.observe(LogSiteId::new(2).unwrap()), LogOnceObservation::RegistryFull);
    }

    #[test]
    fn concurrent_activation_has_one_first_and_one_duplicate() {
        static STATE: LogOnceState<4> = LogOnceState::new();
        static FIRSTS: AtomicUsize = AtomicUsize::new(0);
        static DUPLICATES: AtomicUsize = AtomicUsize::new(0);
        static REPEATED: AtomicUsize = AtomicUsize::new(0);
        const SITE: LogSiteId = LogSiteId::from_location("concurrent", "test.rs", 1, 1);

        let mut threads = std::vec::Vec::new();
        for _ in 0..16 {
            threads.push(thread::spawn(|| match STATE.observe(SITE) {
                LogOnceObservation::First => {
                    FIRSTS.fetch_add(1, Ordering::Relaxed);
                }
                LogOnceObservation::Duplicate => {
                    DUPLICATES.fetch_add(1, Ordering::Relaxed);
                }
                LogOnceObservation::Repeated => {
                    REPEATED.fetch_add(1, Ordering::Relaxed);
                }
                LogOnceObservation::RegistryFull => panic!("registry unexpectedly full"),
            }));
        }
        for thread in threads {
            thread.join().unwrap();
        }

        assert_eq!(FIRSTS.load(Ordering::Relaxed), 1);
        assert_eq!(DUPLICATES.load(Ordering::Relaxed), 1);
        assert_eq!(REPEATED.load(Ordering::Relaxed), 14);
    }

    #[test]
    fn registry_full_dispatches_a_visible_warning() {
        static SINK: CountingSink = CountingSink::new();
        static SINKS: [&'static dyn GlobalLogSink; 1] = [&SINK];
        static ROUTER: GlobalLogRouter = GlobalLogRouter::new(&SINKS);

        SINK.reset();
        let state = LogOnceState::<0>::new();
        assert_eq!(
            log_once_with_area_purpose(
                &ROUTER,
                &state,
                LogSiteId::new(9).unwrap(),
                LogArea::Global,
                Some("once"),
                format_args!("message"),
            ),
            LogOnceObservation::RegistryFull
        );
        assert_eq!(SINK.count.load(Ordering::Relaxed), 1);
        assert_eq!(SINK.last_level.load(Ordering::Relaxed), LogLevel::Warn as u8 + 1);
    }
}
