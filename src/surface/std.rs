extern crate alloc;

/// Shared prelude exports for no_std compatibility wiring.
pub mod prelude {
    pub use core::option::Option::{self, None, Some};
    pub use core::result::Result::{self, Err, Ok};
    pub use core::{cmp, convert, iter, mem, ops, str};
}

/// Re-export alloc types commonly expected from std.
pub mod alloc_types {
    pub use alloc::string::String;
    pub use alloc::vec::Vec;
}

/// Surface-backed filesystem facade.
pub mod fs {
    pub use crate::surface::fs::{
        Error, Result, exists, read, read_to_string, remove_file, rename, write,
    };
}

/// Surface-backed path facade.
pub mod path {
    pub use crate::surface::path::{Path, PathBuf};
}

/// Surface-backed io facade.
pub mod io {
    pub use crate::surface::io::{Error, ErrorKind, Result, Write};
}
