/// Emits a native TRACE record through the installed global LogOs dispatcher.
#[macro_export]
macro_rules! trace {
    (target: $target:expr, $($arg:tt)+) => {{
        if $crate::global_log_enabled($target, $crate::LogLevel::Trace) {
            $crate::global_log_with_target_level(
                $target,
                $crate::LogLevel::Trace,
                format_args!($($arg)+),
            );
        }
    }};
    ($($arg:tt)+) => {{
        $crate::trace!(target: module_path!(), $($arg)+);
    }};
}

/// Emits a native DEBUG record through the installed global LogOs dispatcher.
#[macro_export]
macro_rules! debug {
    (target: $target:expr, $($arg:tt)+) => {{
        if $crate::global_log_enabled($target, $crate::LogLevel::Debug) {
            $crate::global_log_with_target_level(
                $target,
                $crate::LogLevel::Debug,
                format_args!($($arg)+),
            );
        }
    }};
    ($($arg:tt)+) => {{
        $crate::debug!(target: module_path!(), $($arg)+);
    }};
}

/// Emits a native INFO record through the installed global LogOs dispatcher.
#[macro_export]
macro_rules! info {
    (target: $target:expr, $($arg:tt)+) => {{
        if $crate::global_log_enabled($target, $crate::LogLevel::Info) {
            $crate::global_log_with_target_level(
                $target,
                $crate::LogLevel::Info,
                format_args!($($arg)+),
            );
        }
    }};
    ($($arg:tt)+) => {{
        $crate::info!(target: module_path!(), $($arg)+);
    }};
}

/// Emits a native WARN record through the installed global LogOs dispatcher.
#[macro_export]
macro_rules! warn {
    (target: $target:expr, $($arg:tt)+) => {{
        if $crate::global_log_enabled($target, $crate::LogLevel::Warn) {
            $crate::global_log_with_target_level(
                $target,
                $crate::LogLevel::Warn,
                format_args!($($arg)+),
            );
        }
    }};
    ($($arg:tt)+) => {{
        $crate::warn!(target: module_path!(), $($arg)+);
    }};
}

/// Emits a native ERROR record through the installed global LogOs dispatcher.
#[macro_export]
macro_rules! error {
    (target: $target:expr, $($arg:tt)+) => {{
        if $crate::global_log_enabled($target, $crate::LogLevel::Error) {
            $crate::global_log_with_target_level(
                $target,
                $crate::LogLevel::Error,
                format_args!($($arg)+),
            );
        }
    }};
    ($($arg:tt)+) => {{
        $crate::error!(target: module_path!(), $($arg)+);
    }};
}
