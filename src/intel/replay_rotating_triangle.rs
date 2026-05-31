// Metadata for the rotating-triangle replay artifact.
//
// The captured BO bytes are intentionally not embedded with include_bytes! here:
// the artifact is large enough to make rustc consume tens of GiB.  Boot it as a
// Limine module named `trueos.intel.replay.rotating_triangle` instead.
use crate::intel::replay::{ReplayBoSpec, ReplayPatch, ReplayPresent, ReplaySubmit};

pub(crate) const ROTATING_TRIANGLE_MODULE_STRING: &[u8] = b"trueos.intel.replay.rotating_triangle";

pub(crate) const ROTATING_TRIANGLE_BO_SPECS: &[ReplayBoSpec] = &[
    ReplayBoSpec {
        handle: 7,
        gpu_va: 0x00000004BFFFF000,
        size: 0x200000,
        flags: 0xD8,
    },
    ReplayBoSpec {
        handle: 10,
        gpu_va: 0xFFFFEFFEFF400000,
        size: 0x800000,
        flags: 0x58,
    },
    ReplayBoSpec {
        handle: 14,
        gpu_va: 0xFFFFEFFEF6C00000,
        size: 0x8000000,
        flags: 0x58,
    },
    ReplayBoSpec {
        handle: 2,
        gpu_va: 0x00000003C0000000,
        size: 0x200000,
        flags: 0xD8,
    },
    ReplayBoSpec {
        handle: 5,
        gpu_va: 0x00000000C0000000,
        size: 0x200000,
        flags: 0xD8,
    },
    ReplayBoSpec {
        handle: 13,
        gpu_va: 0xFFFFEFFEFEC00000,
        size: 0x800000,
        flags: 0x58,
    },
    ReplayBoSpec {
        handle: 4,
        gpu_va: 0x0000000140000000,
        size: 0x200000,
        flags: 0xD8,
    },
    ReplayBoSpec {
        handle: 6,
        gpu_va: 0x00000002C0000000,
        size: 0x200000,
        flags: 0xD8,
    },
    ReplayBoSpec {
        handle: 3,
        gpu_va: 0x0000000100000000,
        size: 0x200000,
        flags: 0xD8,
    },
    ReplayBoSpec {
        handle: 1,
        gpu_va: 0x0000000000200000,
        size: 0x200000,
        flags: 0xD8,
    },
    ReplayBoSpec {
        handle: 11,
        gpu_va: 0x0000000300000000,
        size: 0x200000,
        flags: 0xD8,
    },
    ReplayBoSpec {
        handle: 8,
        gpu_va: 0xFFFFEFFEFFE00000,
        size: 0x200000,
        flags: 0xD8,
    },
];

pub(crate) const ROTATING_TRIANGLE_BASE_PATCHES: &[ReplayPatch] = &[];
const ROTATING_TRIANGLE_SEQ_2582_PATCHES: &[ReplayPatch] = &[];
const ROTATING_TRIANGLE_SEQ_2845_PATCHES: &[ReplayPatch] = &[];
const ROTATING_TRIANGLE_SEQ_2936_PATCHES: &[ReplayPatch] = &[];

pub(crate) const ROTATING_TRIANGLE_SUBMITS: &[ReplaySubmit] = &[
    ReplaySubmit {
        seq: 2582,
        batch_gpu: 0xFFFFEFFEFFE42000,
        batch_start: 0x22000,
        flags: 0x201800,
        patches: ROTATING_TRIANGLE_SEQ_2582_PATCHES,
    },
    ReplaySubmit {
        seq: 2845,
        batch_gpu: 0xFFFFEFFEFFE62000,
        batch_start: 0x62000,
        flags: 0x201800,
        patches: ROTATING_TRIANGLE_SEQ_2845_PATCHES,
    },
    ReplaySubmit {
        seq: 2936,
        batch_gpu: 0xFFFFEFFEFFE62000,
        batch_start: 0x22000,
        flags: 0x201800,
        patches: ROTATING_TRIANGLE_SEQ_2936_PATCHES,
    },
];

pub(crate) const ROTATING_TRIANGLE_PRESENT: ReplayPresent = ReplayPresent {
    handle: 13,
    offset: 0,
    width: 512,
    height: 512,
    pitch_bytes: 2048,
};
