#![allow(dead_code)]
extern crate alloc;
use std::sync::Mutex;

#[derive(Clone, Copy, PartialEq, Eq)]
enum WindowOwner {
    Vm(u8),
    Shell,
    Kernel,
}
impl WindowOwner {
    const SHELL3_SERVICE: Self = Self::Shell;
    fn name(self) -> &'static str {
        "kernel"
    }
}
#[derive(Clone, Copy)]
struct WindowId(u32);
#[derive(Debug, PartialEq, Eq)]
enum CollapseError {
    NotFound,
    Busy,
}
const COLLAPSED_SIZE: u32 = 64;
static LABEL: Mutex<Option<String>> = Mutex::new(None);
static TITLE: Mutex<String> = Mutex::new(String::new());
static INSTALLED: Mutex<bool> = Mutex::new(false);
static COPIED: Mutex<Vec<u8>> = Mutex::new(Vec::new());
static LETTER: Mutex<String> = Mutex::new(String::new());
static FONT_READY: Mutex<bool> = Mutex::new(true);
static REGISTER_READY: Mutex<bool> = Mutex::new(true);
static REGISTRATIONS: Mutex<usize> = Mutex::new(0);

mod microfont {
    pub const FWIDTH: usize = 6;
    pub const FHEIGHT: usize = 11;
}
mod hv {
    pub fn app_vm_display_label(_: u8) -> Option<String> {
        super::LABEL.lock().unwrap().clone()
    }
}
mod shell3 {
    pub mod tui {
        pub fn vm_for_window(_: super::super::WindowId) -> Option<u8> {
            Some(1)
        }
    }
}
mod window_broker {
    use super::*;
    pub const MAX_WINDOW_TITLE_BYTES: usize = 120;
    pub fn collapse_state(_: WindowOwner, _: WindowId) -> Option<bool> {
        (*INSTALLED.lock().unwrap()).then_some(false)
    }
    pub fn window_snapshot(_: WindowOwner, window: WindowId) -> Option<()> {
        (window.0 != 0).then_some(())
    }
    pub fn window_title(
        _: WindowOwner,
        _: WindowId,
        out: &mut [u8; MAX_WINDOW_TITLE_BYTES],
    ) -> Result<usize, ()> {
        let title = TITLE.lock().unwrap();
        out[..title.len()].copy_from_slice(title.as_bytes());
        Ok(title.len())
    }
}
mod r {
    pub mod services {
        pub mod microfont_mask_service {
            pub fn copy_text_mask(
                pixels: &mut [u8],
                width: usize,
                _: usize,
                x: usize,
                y: usize,
                text: &str,
                _: usize,
                _: usize,
            ) -> Result<(), &'static str> {
                if !*crate::FONT_READY.lock().unwrap() {
                    return Err("microfont-not-ready");
                }
                *crate::LETTER.lock().unwrap() = text.to_owned();
                pixels[y * width + x] = 127;
                Ok(())
            }
        }
    }
}
fn enable_window_collapse(
    _: WindowOwner,
    _: WindowId,
    width: u32,
    height: u32,
    pitch: usize,
    rgba: &[u8],
) -> Result<(), CollapseError> {
    assert_eq!((width, height, pitch, rgba.len()), (64, 64, 256, 16_384));
    if !*REGISTER_READY.lock().unwrap() {
        return Err(CollapseError::Busy);
    }
    *COPIED.lock().unwrap() = rgba.to_owned();
    *INSTALLED.lock().unwrap() = true;
    *REGISTRATIONS.lock().unwrap() += 1;
    Ok(())
}
mod collapse {
    use super::*;
    include!("fallback.rs");

    // One test keeps the mocked window and atlas lifecycle sequential.
    #[test]
    fn fallback_names_registration_and_retry_contract() {
        assert_eq!(collapse_initial(" \t\nÉdit"), Some('É'));
        assert_eq!(collapse_initial("\u{2003}λambda"), Some('λ'));
        assert_eq!(collapse_initial(" \t\0"), None);
        assert_eq!(
            ensure_window_collapse(WindowOwner::Vm(1), WindowId(0)),
            Err(CollapseError::NotFound)
        );
        assert_eq!(*REGISTRATIONS.lock().unwrap(), 0);

        *LABEL.lock().unwrap() = Some("  Édit".into());
        *TITLE.lock().unwrap() = "window".into();
        ensure_window_collapse(WindowOwner::Vm(1), WindowId(1)).unwrap();
        assert_eq!(&*LETTER.lock().unwrap(), "É");
        let copied = COPIED.lock().unwrap();
        assert_eq!(copied.chunks_exact(4).filter(|pixel| pixel[3] != 0).count(), 1);
        assert!(copied.chunks_exact(4).any(|pixel| pixel == [255, 255, 255, 127]));
        let registered_image = copied.clone();
        drop(copied);

        // An application image must survive fallback checks, even without an atlas.
        *LABEL.lock().unwrap() = Some("OSM".into());
        *FONT_READY.lock().unwrap() = false;
        ensure_window_collapse(WindowOwner::Vm(1), WindowId(1)).unwrap();
        assert_eq!(*REGISTRATIONS.lock().unwrap(), 1);
        assert_eq!(*COPIED.lock().unwrap(), registered_image);

        *INSTALLED.lock().unwrap() = false;
        assert_eq!(
            ensure_window_collapse(WindowOwner::SHELL3_SERVICE, WindowId(1)),
            Err(CollapseError::Busy)
        );
        assert!(!*INSTALLED.lock().unwrap());
        *FONT_READY.lock().unwrap() = true;
        ensure_window_collapse(WindowOwner::SHELL3_SERVICE, WindowId(1)).unwrap();
        assert_eq!(&*LETTER.lock().unwrap(), "O");

        *INSTALLED.lock().unwrap() = false;
        *LABEL.lock().unwrap() = Some("\t".into());
        ensure_window_collapse(WindowOwner::Vm(1), WindowId(1)).unwrap();
        assert_eq!(&*LETTER.lock().unwrap(), "w");

        *INSTALLED.lock().unwrap() = false;
        *TITLE.lock().unwrap() = "".into();
        ensure_window_collapse(WindowOwner::Kernel, WindowId(1)).unwrap();
        assert_eq!(&*LETTER.lock().unwrap(), "k");

        *INSTALLED.lock().unwrap() = false;
        *REGISTER_READY.lock().unwrap() = false;
        assert_eq!(
            ensure_window_collapse(WindowOwner::Kernel, WindowId(1)),
            Err(CollapseError::Busy)
        );
        assert!(!*INSTALLED.lock().unwrap());
        *REGISTER_READY.lock().unwrap() = true;
        ensure_window_collapse(WindowOwner::Kernel, WindowId(1)).unwrap();
    }
}
