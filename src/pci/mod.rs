pub mod bar_alloc;
#[cfg(target_arch = "x86_64")]
pub mod mmio;
#[cfg(not(target_arch = "x86_64"))]
#[path = "mmio_disabled.rs"]
pub mod mmio;
pub mod nic_fpga_dma;
#[cfg(target_arch = "x86_64")]
mod nvme_backend;
#[cfg(target_arch = "x86_64")]
pub mod nvme;
#[cfg(not(target_arch = "x86_64"))]
#[path = "nvme_disabled.rs"]
pub mod nvme;
#[cfg(target_arch = "x86_64")]
pub(crate) mod nvme_backend;
mod pci;
pub mod pciids;
pub mod vrng;
pub use pci::*;
