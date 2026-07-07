use core::fmt;
use log::{Metadata, Record};

use crate::flags::{
    LogArea, LogAreaSet, LogLevelPolicy, level_enabled, module_path_log_area, target_log_area,
};

pub trait GlobalLogSink: Sync {
    fn spec(&self) -> GlobalLogSinkSpec;

    fn level_policy(&self, _area: LogArea) -> LogLevelPolicy {
        self.spec().level
    }

    fn accepts(&self, area: LogArea, level: log::Level) -> bool {
        self.spec().areas.contains(area) && level_enabled(self.level_policy(area), level)
    }

    fn write_accepted(
        &self,
        area: LogArea,
        level: log::Level,
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

    pub fn accepts(self, area: LogArea, level: log::Level) -> bool {
        self.areas.contains(area) && level_enabled(self.level, level)
    }
}

pub trait GlobalLogDispatch: Sync {
    fn enabled(&self, area: LogArea, level: log::Level) -> bool;
    fn emit(
        &self,
        area: LogArea,
        level: log::Level,
        purpose: Option<&str>,
        args: fmt::Arguments<'_>,
    );
}

impl<T: GlobalLogSink> GlobalLogDispatch for T {
    fn enabled(&self, area: LogArea, level: log::Level) -> bool {
        self.accepts(area, level)
    }

    fn emit(
        &self,
        area: LogArea,
        level: log::Level,
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
    fn enabled(&self, area: LogArea, level: log::Level) -> bool {
        self.sinks.iter().any(|sink| sink.accepts(area, level))
    }

    fn emit(
        &self,
        area: LogArea,
        level: log::Level,
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
    log_with_area_level(dispatch, LogArea::Global, log::Level::Info, args);
}

pub fn purpose_for_level(level: log::Level) -> &'static str {
    match level {
        log::Level::Trace => "trace",
        log::Level::Debug => "debug",
        log::Level::Info => "info",
        log::Level::Warn => "warn",
        log::Level::Error => "error",
    }
}

pub fn log_with_area_level<D: GlobalLogDispatch>(
    dispatch: &D,
    area: LogArea,
    level: log::Level,
    args: fmt::Arguments<'_>,
) {
    log_with_area_purpose(dispatch, area, level, Some(purpose_for_level(level)), args);
}

pub fn log_with_area_purpose<D: GlobalLogDispatch>(
    dispatch: &D,
    area: LogArea,
    level: log::Level,
    purpose: Option<&str>,
    args: fmt::Arguments<'_>,
) {
    dispatch.emit(area, level, purpose, args);
}

pub fn log_with_target_purpose<D: GlobalLogDispatch>(
    dispatch: &D,
    target: &str,
    level: log::Level,
    purpose: Option<&str>,
    args: fmt::Arguments<'_>,
) {
    let area = target_log_area(target);
    log_with_area_purpose(dispatch, area, level, purpose, args);
}

pub fn log_with_target_level<D: GlobalLogDispatch>(
    dispatch: &D,
    target: &str,
    level: log::Level,
    args: fmt::Arguments<'_>,
) {
    let area = target_log_area(target);
    log_with_area_level(dispatch, area, level, args);
}

pub fn metadata_log_area(metadata: &Metadata<'_>) -> LogArea {
    target_log_area(metadata.target())
}

pub fn record_log_area(record: &Record<'_>) -> LogArea {
    let area = metadata_log_area(record.metadata());
    if area != LogArea::Global {
        return area;
    }
    record
        .module_path()
        .map(module_path_log_area)
        .unwrap_or(LogArea::Global)
}

pub struct GlobalLogFacade<D: GlobalLogDispatch + 'static> {
    dispatch: &'static D,
}

impl<D: GlobalLogDispatch> GlobalLogFacade<D> {
    pub const fn new(dispatch: &'static D) -> Self {
        Self { dispatch }
    }
}

impl<D: GlobalLogDispatch> log::Log for GlobalLogFacade<D> {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        self.dispatch
            .enabled(metadata_log_area(metadata), metadata.level())
    }

    fn log(&self, record: &Record<'_>) {
        let area = record_log_area(record);
        let purpose = purpose_for_level(record.level());
        self.dispatch.emit(
            area,
            record.level(),
            Some(purpose),
            format_args!("{}: {}\n", record.target(), record.args()),
        );
    }

    fn flush(&self) {}
}
