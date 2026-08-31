//! Kernel-internal compatibility imports for synchronous filesystem users.
//!
//! These link to `trueos_kernel_sync_*` implementation symbols. They are not
//! part of the Blueprint CABI declaration file and cannot satisfy the removed
//! `trueos_cabi_fs_*` imports of an application Blueprint.

unsafe extern "C" {
    #[link_name = "trueos_kernel_sync_fs_read_file"]
    pub fn trueos_cabi_fs_read_file(
        path_ptr: *const u8,
        path_len: usize,
        out_ptr: *mut u8,
        out_cap: usize,
    ) -> isize;
    #[link_name = "trueos_kernel_sync_fs_write_begin"]
    pub fn trueos_cabi_fs_write_begin(
        path_ptr: *const u8,
        path_len: usize,
        total_len: u64,
        out_handle: *mut u32,
    ) -> i32;
    #[link_name = "trueos_kernel_sync_fs_typed_write_begin"]
    pub fn trueos_cabi_fs_typed_write_begin(
        path_ptr: *const u8,
        path_len: usize,
        total_len: u64,
        content_type: u32,
        out_handle: *mut u32,
    ) -> i32;
    #[link_name = "trueos_kernel_sync_fs_typed_stat"]
    pub fn trueos_cabi_fs_typed_stat(
        path_ptr: *const u8,
        path_len: usize,
        out_kind: *mut u32,
        out_len: *mut u64,
        out_content_type: *mut u32,
    ) -> i32;
    #[link_name = "trueos_kernel_sync_fs_create_dir_all"]
    pub fn trueos_cabi_fs_create_dir_all(path_ptr: *const u8, path_len: usize) -> i32;
    #[link_name = "trueos_kernel_sync_fs_write_chunk"]
    pub fn trueos_cabi_fs_write_chunk(handle: u32, data_ptr: *const u8, data_len: usize) -> i32;
    #[link_name = "trueos_kernel_sync_fs_write_finish"]
    pub fn trueos_cabi_fs_write_finish(handle: u32) -> i32;
    #[link_name = "trueos_kernel_sync_fs_write_abort"]
    pub fn trueos_cabi_fs_write_abort(handle: u32) -> i32;
    #[link_name = "trueos_kernel_sync_fs_exists"]
    pub fn trueos_cabi_fs_exists(path_ptr: *const u8, path_len: usize) -> i32;
    #[link_name = "trueos_kernel_sync_fs_stat"]
    pub fn trueos_cabi_fs_stat(
        path_ptr: *const u8,
        path_len: usize,
        out_kind: *mut u32,
        out_len: *mut u64,
    ) -> i32;
    #[link_name = "trueos_kernel_sync_fs_list_dir"]
    pub fn trueos_cabi_fs_list_dir(
        path_ptr: *const u8,
        path_len: usize,
        out_ptr: *mut u8,
        out_cap: usize,
    ) -> isize;
    #[link_name = "trueos_kernel_sync_fs_remove"]
    pub fn trueos_cabi_fs_remove(path_ptr: *const u8, path_len: usize) -> i32;
    #[link_name = "trueos_kernel_sync_trueosfs_primary_html_tree"]
    pub fn trueos_cabi_trueosfs_primary_html_tree(
        max_entries: u32,
        out_ptr: *mut u8,
        out_cap: usize,
    ) -> isize;
    #[link_name = "trueos_kernel_sync_trueosfs_json_all"]
    pub fn trueos_cabi_trueosfs_json_all(
        max_entries: u32,
        out_ptr: *mut u8,
        out_cap: usize,
    ) -> isize;
}
