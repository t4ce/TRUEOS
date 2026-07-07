#![no_std]

pub mod flags;
pub mod global;

pub use flags::{
    LogArea, LogLevelPolicy, LogLevelSet, level_enabled, module_path_log_area, target_log_area,
    threshold_down_set, threshold_up_set,
};
pub use global::{
    GlobalLogFacade, GlobalLogSink, log, log_with_area_level, log_with_area_purpose,
    log_with_target_level, metadata_log_area, purpose_for_level, record_log_area,
};
