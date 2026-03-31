extern crate alloc;

pub use crate::env;
pub use crate::vcabi as cabi;
pub use crate::vshell as shell;

pub mod kfs {
    use alloc::string::String;
    use alloc::vec::Vec;

    #[inline]
    pub fn read_file(path: &str) -> Result<Vec<u8>, i32> {
        crate::vfs::read_file(path.as_bytes())
    }

    #[inline]
    pub fn read_file_utf8(path: &str) -> Result<String, i32> {
        crate::vfs::read_file_utf8(path.as_bytes())
    }

    #[inline]
    pub fn write_file_begin(path: &str, total_len: u64) -> Result<u32, i32> {
        crate::vfs::write_begin(path.as_bytes(), total_len)
    }

    #[inline]
    pub fn write_file_chunk(handle: u32, data: &[u8]) -> Result<(), i32> {
        crate::vfs::write_chunk(handle, data)
    }

    #[inline]
    pub fn write_file_finish(handle: u32) -> Result<(), i32> {
        crate::vfs::write_finish(handle)
    }

    #[inline]
    pub fn write_file_abort(handle: u32) -> Result<(), i32> {
        crate::vfs::write_abort(handle)
    }

    #[inline]
    pub fn remove(path: &str) -> Result<(), i32> {
        crate::vfs::remove(path.as_bytes())
    }

    #[inline]
    pub fn html_tree(max_entries: usize) -> Result<String, i32> {
        let bytes = crate::vfs::trueosfs_primary_html_tree(max_entries as u32)?;
        String::from_utf8(bytes).map_err(|_| -1)
    }
}
