//! Filesystem API constants, translated into `bitflags` constants.

use crate::backend;

pub use crate::timespec::{Nsecs, Secs, Timespec};
pub use backend::fs::types::*;

impl FileType {
    /// Returns `true` if this `FileType` is a regular file.
    pub fn is_file(self) -> bool {
        self == Self::RegularFile
    }

    /// Returns `true` if this `FileType` is a directory.
    pub fn is_dir(self) -> bool {
        self == Self::Directory
    }

    /// Returns `true` if this `FileType` is a symlink.
    pub fn is_symlink(self) -> bool {
        self == Self::Symlink
    }

    /// Returns `true` if this `FileType` is a fifo.
    #[cfg(not(target_os = "wasi"))]
    pub fn is_fifo(self) -> bool {
        self == Self::Fifo
    }

    /// Returns `true` if this `FileType` is a socket.
    #[cfg(not(target_os = "wasi"))]
    pub fn is_socket(self) -> bool {
        self == Self::Socket
    }

    /// Returns `true` if this `FileType` is a character device.
    pub fn is_char_device(self) -> bool {
        self == Self::CharacterDevice
    }

    /// Returns `true` if this `FileType` is a block device.
    pub fn is_block_device(self) -> bool {
        self == Self::BlockDevice
    }
}
