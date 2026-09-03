use crate::backend::kmod::hub::{Hub, HubInfo};
use crate::{Mmio, USBHost, recover::XhciDmaPolicy};

mod dwc;
mod hub;
mod kcore;
pub mod osal;
pub(crate) mod queue;
mod transfer;
mod xhci;

use crate::err::*;

use alloc::boxed::Box;

use alloc::collections::btree_map::BTreeMap;
use dwc::Dwc;
use id_arena::Id;
use kcore::*;
use usb_if::Speed;
use xhci::Xhci;

pub use dwc::{
    CruOp, DwcNewParams, DwcParams, UdphyParam, Usb2PhyParam, UsbPhyInterfaceMode,
    usb2phy::Usb2PhyPortId,
};
pub use osal::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XhciRootHubInitPolicy {
    SelectivePorts3And4Skip11,
    FullAllPorts,
    QuarantinedObserveOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct XhciConstructionPolicy {
    pub dma: XhciDmaPolicy,
    pub root_hub: XhciRootHubInitPolicy,
}

impl XhciConstructionPolicy {
    pub const fn new(dma: XhciDmaPolicy, root_hub: XhciRootHubInitPolicy) -> Self {
        Self { dma, root_hub }
    }

    pub const fn delivered(root_hub: XhciRootHubInitPolicy) -> Self {
        Self::new(XhciDmaPolicy::FromCapability, root_hub)
    }

    pub const fn quarantined() -> Self {
        Self::new(XhciDmaPolicy::Force32Bit, XhciRootHubInitPolicy::QuarantinedObserveOnly)
    }
}

impl USBHost {
    pub fn new_xhci(mmio: Mmio, kernel: &'static dyn KernelOp) -> Result<USBHost> {
        Ok(USBHost::new(Xhci::new(mmio, kernel)?))
    }

    pub fn new_xhci_with_root_hub_init_policy(
        mmio: Mmio,
        kernel: &'static dyn KernelOp,
        policy: XhciRootHubInitPolicy,
    ) -> Result<USBHost> {
        Ok(USBHost::new(Xhci::new_with_root_hub_init_policy(mmio, kernel, policy)?))
    }

    pub fn new_xhci_with_root_hub_init_policy_and_mmio_len(
        mmio: Mmio,
        mmio_len: usize,
        kernel: &'static dyn KernelOp,
        policy: XhciRootHubInitPolicy,
    ) -> Result<USBHost> {
        Ok(USBHost::new(Xhci::new_with_root_hub_init_policy_and_mmio_len(
            mmio, mmio_len, kernel, policy,
        )?))
    }

    pub fn new_xhci_with_construction_policy_and_mmio_len(
        mmio: Mmio,
        mmio_len: usize,
        kernel: &'static dyn KernelOp,
        policy: XhciConstructionPolicy,
    ) -> Result<USBHost> {
        Ok(USBHost::new(Xhci::new_with_construction_policy_and_mmio_len(
            mmio, mmio_len, kernel, policy,
        )?))
    }

    pub fn new_dwc(params: DwcNewParams<'_, impl CruOp>) -> Result<USBHost> {
        Ok(USBHost::new(Dwc::new(params)?))
    }

    pub(crate) fn new(backend: impl CoreOp) -> Self {
        let b = Core::new(backend);
        Self {
            backend: Box::new(b),
        }
    }
}

pub struct DeviceAddressInfo {
    pub root_port_id: u8,
    pub parent_hub: Option<Id<Hub>>,
    pub port_speed: Speed,
    pub port_id: u8,
    pub infos: BTreeMap<Id<Hub>, HubInfo>,
}
