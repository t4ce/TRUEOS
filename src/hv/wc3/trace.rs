use core::fmt;

pub(super) fn info(args: fmt::Arguments<'_>) {
    super::super::hvlogf(format_args!("wc3: {args}"));
}

pub(super) fn fail(args: fmt::Arguments<'_>) {
    super::super::hverrorf(format_args!("wc3: gate-0 failed: {args}"));
}
