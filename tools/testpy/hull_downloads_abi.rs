//! Host boundary stubs; the read entrypoint and path predicates are production code.
#![allow(dead_code)]
extern crate self as trueos_ide;
extern crate self as trueos_vm;
mod codes { include!("@CABI_CODES@"); }
pub mod store_protocol {
    pub const REQUEST: &str = "vFile:ide/request";
    pub const REPLY: &str = "vFile:ide/reply";
}
pub mod vmcall { pub const OP_BP_ASYNC_FS_READ_START: u32 = 1; }
thread_local! {
    static GUEST: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
    static LAST: core::cell::RefCell<Option<(bool, String)>> = const { core::cell::RefCell::new(None) };
}
fn record(guest: bool, path: &str) -> i32 {
    LAST.with(|last| *last.borrow_mut() = Some((guest, path.to_owned())));
    123
}
mod hv {
    pub fn current_hull_guest_context_vm_id() -> Option<u8> {
        crate::GUEST.with(|guest| guest.get().then_some(1))
    }
}
mod r {
    pub mod services {
        pub mod hull_download_service { pub const PREFIX: &str = "vFile:hulldownloads/"; }
        pub mod codex_input_service { pub const VFILE_PREFIX: &str = "vFile:codex-input/"; }
        pub mod ide_workspace_service {
            pub fn is_read(_: &str) -> bool { false }
            pub fn is_write(_: &str) -> bool { false }
        }
    }
    pub mod io {
        pub mod env {
            // Deliberately changes normal paths: a virtual read must bypass it.
            pub fn resolve_fs_path(path: &str, _: bool) -> Option<String> {
                Some(format!("disk/{}", path.replace("//", "/")))
            }
        }
        pub mod async_fs_cabi {
            use crate::codes::*;
            fn direct_owner() -> u32 { 1 }
            fn start_read(_: u32, path: String) -> i32 { crate::record(false, &path) }
            fn guest_start(_: u32, path: &str) -> i32 { crate::record(true, path) }
            // @PRODUCTION@
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use r::io::async_fs_cabi::trueos_cabi_async_fs_read_start as read;
    #[test]
    fn github_urls_reach_virtual_provider_verbatim_on_both_lanes() {
        for guest in [false, true] {
            GUEST.with(|mode| mode.set(guest));
            for url in [
                "https://github.com/ip7z/7zip/releases/download/26.05/7z2605-x64.exe".to_owned(),
                "https://github.com/ip7z/7zip/releases/download/26.05/7z2605-arm64.exe".to_owned(),
                format!("https://example.test/{}", "x".repeat(4096 - 21)),
            ] {
                let path = format!("vFile:hull-navigation/{url}");
                assert!(path.len() > 64);
                assert_eq!(unsafe { read(path.as_ptr(), path.len()) }, 123);
                LAST.with(|last| assert_eq!(*last.borrow(), Some((guest, path))));
            }
            for path in ["vFile:launch", "vFile:hulldownloads/next/18446744073709551615"] {
                assert_eq!(unsafe { read(path.as_ptr(), path.len()) }, 123);
                LAST.with(|last| assert_eq!(*last.borrow(), Some((guest, path.to_owned()))));
            }
        }
    }
    #[test]
    fn real_paths_still_normalize_and_invalid_inputs_stay_bounded() {
        GUEST.with(|mode| mode.set(false));
        let path = b"common/file.txt";
        assert_eq!(unsafe { read(path.as_ptr(), path.len()) }, 123);
        LAST.with(|last| assert_eq!(*last.borrow(), Some((false, "disk/common/file.txt".to_owned()))));
        assert_eq!(unsafe { read(core::ptr::null(), 1) }, codes::FS_ERR_BAD_PARAM);
        assert_eq!(unsafe { read(core::ptr::null(), 0) }, codes::FS_ERR_BAD_PARAM);
        assert_eq!(unsafe { read([255].as_ptr(), 1) }, codes::FS_ERR_BAD_UTF8);
        let path = format!("vFile:hull-navigation/{}", "x".repeat(4097));
        assert_eq!(unsafe { read(path.as_ptr(), path.len()) }, codes::FS_ERR_TOO_LARGE);
        let path = vec![b'a'; codes::BLUEPRINT_ASYNC_FS_MAX_PATH + 1];
        assert_eq!(unsafe { read(path.as_ptr(), path.len()) }, codes::FS_ERR_TOO_LARGE);
    }
}
