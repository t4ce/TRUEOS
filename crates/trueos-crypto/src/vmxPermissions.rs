//! VMX operation classifications. The runtime makes authorization decisions.

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum RiskLevel {
    No,
    Low,
    Med,
    High,
}

/// Permissions describe the operation requested by a VM. Docking uses both
/// geometry permissions when it changes a frame's position and size.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VmxPermission {
    /// Change a UI4 frame's position, including dragging a collapsed frame.
    MoveWindow,
    /// Change a UI4 frame's size, including collapsing or expanding it.
    ResizeWindow,
    /// An operation outside the explicitly classified frame geometry effects.
    Other,
}

impl VmxPermission {
    pub const fn risk_level(self) -> RiskLevel {
        match self {
            Self::MoveWindow | Self::ResizeWindow => RiskLevel::No,
            Self::Other => RiskLevel::High,
        }
    }
}
