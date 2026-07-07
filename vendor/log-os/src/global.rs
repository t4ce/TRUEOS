use core::fmt;
use log::{Metadata, Record};

use crate::flags::{LogArea, module_path_log_area, target_log_area};

pub trait GlobalLogSink: Sync {
    fn enabled(&self, area: LogArea, level: log::Level) -> bool;
    fn write(&self, purpose: Option<&str>, args: fmt::Arguments<'_>);
}

pub fn log<S: GlobalLogSink>(sink: &S, args: fmt::Arguments<'_>) {
    log_with_area_level(sink, LogArea::Global, log::Level::Info, args);
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

pub fn log_with_area_level<S: GlobalLogSink>(
    sink: &S,
    area: LogArea,
    level: log::Level,
    args: fmt::Arguments<'_>,
) {
    log_with_area_purpose(sink, area, level, Some(purpose_for_level(level)), args);
}

pub fn log_with_area_purpose<S: GlobalLogSink>(
    sink: &S,
    area: LogArea,
    level: log::Level,
    purpose: Option<&str>,
    args: fmt::Arguments<'_>,
) {
    if !sink.enabled(area, level) {
        return;
    }
    sink.write(purpose, args);
}

pub fn log_with_target_level<S: GlobalLogSink>(
    sink: &S,
    target: &str,
    level: log::Level,
    args: fmt::Arguments<'_>,
) {
    let area = target_log_area(target);
    log_with_area_level(sink, area, level, args);
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

pub struct GlobalLogFacade<S: GlobalLogSink + 'static> {
    sink: &'static S,
}

impl<S: GlobalLogSink> GlobalLogFacade<S> {
    pub const fn new(sink: &'static S) -> Self {
        Self { sink }
    }
}

impl<S: GlobalLogSink> log::Log for GlobalLogFacade<S> {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        self.sink
            .enabled(metadata_log_area(metadata), metadata.level())
    }

    fn log(&self, record: &Record<'_>) {
        let area = record_log_area(record);
        if !self.sink.enabled(area, record.level()) {
            return;
        }
        let purpose = purpose_for_level(record.level());
        self.sink
            .write(Some(purpose), format_args!("{}: {}\n", record.target(), record.args()));
    }

    fn flush(&self) {}
}
