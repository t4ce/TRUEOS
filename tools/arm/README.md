# Archived ARM experiment

This directory preserves the incomplete ARM port work without making it part
of the active TRUEOS build.

The top-level files are the ARM target, Limine, build, and disabled-module
artifacts that previously lived in active source/configuration paths. The
`snapshots/` tree preserves the ARM-related Rust `cfg` branches, stubs,
cross-compiler setup, comments, and surrounding implementation context as they
existed immediately before the x86-only cleanup.

Nothing in this directory is wired into the current build. If ARM support is
revisited, treat these files as reference material rather than a supported or
partially supported target.
