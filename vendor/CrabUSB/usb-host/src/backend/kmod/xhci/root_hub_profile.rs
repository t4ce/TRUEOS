use crate::backend::kmod::XhciRootHubInitPolicy;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum XhciRootHubPhysics {
    BaremetalSelective,
    EmulatedFullReset,
}

impl XhciRootHubPhysics {
    pub(crate) const BAREMETAL_SKIPPED_ROOT_PORT: u8 = 11;

    pub(crate) fn from_init_policy(policy: XhciRootHubInitPolicy) -> Self {
        match policy {
            XhciRootHubInitPolicy::SelectivePorts3And4Skip11 => Self::BaremetalSelective,
            XhciRootHubInitPolicy::FullAllPorts => Self::EmulatedFullReset,
        }
    }

    pub(crate) const fn ignores_root_port(self, port_id: u8) -> bool {
        matches!(self, Self::BaremetalSelective) && port_id == Self::BAREMETAL_SKIPPED_ROOT_PORT
    }
}
