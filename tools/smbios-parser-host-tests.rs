//! Small host-side harness for the pure SMBIOS structure walker tests.
//!
//! TRUEOS disables tests for its freestanding kernel binary. This wrapper
//! supplies inert discovery/mapping stubs so `src/efi/smbios.rs` can still be
//! compiled and its bounded parser tests can run on the development host:
//!
//! `rustc --edition=2024 --test tools/smbios-parser-host-tests.rs -o /tmp/trueos-smbios-tests`

#![allow(dead_code)]

mod limine {
    pub fn smbios_entry_point_addresses() -> Option<(u64, u64)> {
        None
    }

    pub fn try_as_phys_addr(_raw: u64) -> Option<u64> {
        None
    }

    pub fn memmap_contains_phys_range(_phys: u64, _byte_len: usize) -> bool {
        false
    }
}

mod pci {
    pub mod mmio {
        use core::ptr::NonNull;

        #[derive(Debug)]
        pub struct MapError;

        pub fn map_mmio_region_exact(
            _phys: u64,
            _byte_len: usize,
        ) -> Result<NonNull<u8>, MapError> {
            Err(MapError)
        }
    }
}

#[path = "../src/efi/smbios.rs"]
mod smbios;
