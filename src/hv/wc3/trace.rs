use core::fmt;

pub(super) fn info(args: fmt::Arguments<'_>) {
    crate::log_important!(target: "hv"; "wc3: {args}");
}

pub(super) fn fail(args: fmt::Arguments<'_>) {
    super::super::hverrorf(format_args!("wc3: {args}"));
}
