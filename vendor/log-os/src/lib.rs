#![no_std]

#[cfg(test)]
extern crate std;

pub mod flags;
pub mod global;
mod macros;

pub use flags::{
    DEFAULT_AREA_LOG_POLICY, LogArea, LogAreaSet, LogLevel, LogLevelFilter, LogLevelPolicy,
    LogLevelSet, area_tag, default_area_log_policy, level_enabled, module_path_log_area,
    target_log_area, threshold_down_set, threshold_up_set,
};
pub use global::{
    GlobalLogDispatch, GlobalLogRouter, GlobalLogSink, GlobalLogSinkSpec, LogOnceObservation,
    LogOnceState, LogRateLimitObservation, LogRateLimitState, LogSiteId, global_log_enabled,
    global_log_with_target_level, install_global_log_dispatch, log, log_once_with_area_purpose,
    log_with_area_level, log_with_area_purpose, log_with_target_level, log_with_target_purpose,
    purpose_for_level,
};
