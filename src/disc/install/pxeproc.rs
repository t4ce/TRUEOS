//! Persistent disk boot mode; networking and live replacement remain ordinary OS operations.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstallMode {
    Regular,
    Pxeproc,
}

impl InstallMode {
    pub(super) fn limine_conf(self) -> &'static [u8] {
        match self {
            Self::Regular => b"timeout: 0\ndefault_entry: 1\n\n/TRUEOS\nprotocol: limine\nkernel_path: boot():/TRUEOS.ELF\ncmdline: timezone=Europe/Berlin keyboard=de\nresolution: 2560x1440x32\n\n",
            Self::Pxeproc => b"timeout: 0\ndefault_entry: 1\n\n/TRUEOS\nprotocol: limine\nkernel_path: boot():/TRUEOS.ELF\ncmdline: timezone=Europe/Berlin keyboard=de pxeproc=1\nresolution: 2560x1440x32\n\n",
        }
    }
}

pub(crate) fn enabled_in_cmdline(cmdline: &str) -> bool {
    cmdline
        .split_ascii_whitespace()
        .any(|arg| arg == "pxeproc=1")
}

pub(crate) fn boot_enabled() -> bool {
    crate::limine::executable_cmdline().is_some_and(enabled_in_cmdline)
}
