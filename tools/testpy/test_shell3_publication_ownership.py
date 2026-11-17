#!/usr/bin/env python3
"""Exercise real Show::present and frame-pool ownership on a host.

Hardware painters/display scanout are replaced. Production buffer acquisition,
publication and read-lease transfers retain the two-buffer deadlock contract.
"""
from pathlib import Path
import subprocess
import tempfile
from test_clip_position3_uv_texture import ROOT, item


def harness_source() -> str:
    source = (ROOT / "src/shell3/show/show.rs").read_text()
    start = source.index("    pub(super) async fn present(")
    end = source.index("\n    }\n", start) + len("\n    }\n")
    method = source[start:end]
    frame_items = "\n".join(item("src/ui4/frame_pool.rs", name) for name in (
        "acquire_frame_buffer", "acquire_published_frame", "release_published_frame",
        "publish_checked_frame",
    ))
    template = r'''#![allow(dead_code)]
extern crate alloc;
extern crate self as trueos_terminal;
pub mod edit_life { pub struct EditLife; }
use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};
fn block_on<T>(f: impl Future<Output=T>) -> T {
    match pin!(f).as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("fake hardware must complete synchronously"),
    }
}
mod r { pub mod services { pub mod shell3_font_service {
    pub async fn prepare() -> Result<(), &'static str> { Ok(()) }
} } }
mod ui4 {
    use std::sync::{LazyLock, Mutex, MutexGuard};
    #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub struct FrameHandle(pub u32);
    #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub struct FrameWriteLease { pub frame: FrameHandle, pub buffer_index:u8, token:u64 }
    #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub struct FrameReadLease { pub frame: FrameHandle, pub buffer_index:u8 }
    #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub struct PublishedFrame { pub frame:FrameHandle, pub buffer_index:u8, pub publish_serial:u64 }
    #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum FramePoolError { InvalidHandle, InvalidLease, Busy, ImmutablePublished, NotPublished }
    #[derive(Clone, Copy)] struct AcquiredBuffer { index:u8, token:u64 }
    #[derive(Clone)] pub struct FrameRecord {
        buffer_count:u8, front_buffer:Option<u8>, next_buffer:u8,
        acquired:[Option<AcquiredBuffer>;2], readers:[u16;2], surfaces:[Option<()>;2],
        gpu_authored:[bool;2], gpu_release:[Option<()>;2], fully_opaque:[bool;2],
        next_token:u64, publish_serial:u64,
    }
    pub struct Pool { frame:FrameRecord }
    impl Pool { fn checked_mut(&mut self, handle:FrameHandle) -> Result<&mut FrameRecord,FramePoolError> {
        if handle != FrameHandle(1) { Err(FramePoolError::InvalidHandle) } else { Ok(&mut self.frame) }
    } }
    struct DirectMutex(Mutex<Pool>);
    impl DirectMutex { fn lock(&self) -> MutexGuard<'_,Pool> { self.0.lock().unwrap() } }
    static FRAME_POOL: LazyLock<DirectMutex> = LazyLock::new(|| DirectMutex(Mutex::new(Pool { frame:record() })));
    fn record() -> FrameRecord { FrameRecord {
        buffer_count:2, front_buffer:Some(0), next_buffer:1,
        acquired:[None;2], readers:[1,0], surfaces:[Some(());2],
        gpu_authored:[false;2], gpu_release:[None;2], fully_opaque:[false;2],
        next_token:0, publish_serial:1,
    } }
    fn next_serial(value:u64) -> u64 { value + 1 }
@FRAME_ITEMS@
    pub fn reset() { FRAME_POOL.lock().frame = record(); }
    pub fn paint() {
        let lease = acquire_frame_buffer(FrameHandle(1)).unwrap();
        let mut pool = FRAME_POOL.lock();
        publish_checked_frame(pool.checked_mut(lease.frame).unwrap(), lease).unwrap();
    }
    pub fn display_transfer() {
        // Existing scanout still reads buffer A. A broker transaction replaces
        // it with the newly published exact buffer B, then releases A.
        let next = acquire_published_frame(FrameHandle(1)).unwrap();
        assert_eq!(next.buffer_index, 1);
        release_published_frame(FrameReadLease { frame:FrameHandle(1), buffer_index:0 }).unwrap();
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq)] pub struct DamageRect { pub width:u32, pub height:u32 }
    impl DamageRect { pub const FULL:Self = Self { width:u32::MAX,height:u32::MAX }; }
    pub mod text_area { pub struct RasterBudget; impl RasterBudget { pub fn new(_:usize)->Self {Self} } }
}
mod allcaps { pub mod shell3 { pub const SH3_PANBUFFER_CAP_BYTES:usize=1; } }
mod shell3 {
    #[derive(Clone, Copy)] pub struct RgbaColor;
    #[derive(Default)] pub struct SegmentUpdate;
    #[derive(Default)] pub struct UpdateBatch { pub segments:Vec<SegmentUpdate> }
    pub mod update { pub struct StripReveal; pub struct MatrixAreaSnapshot; }
    pub mod presentation_probe { pub fn stage(_:u64) {} }
    pub mod show {
        use super::RgbaColor;
        use crate::ui4::{DamageRect,FrameHandle};
        use std::cell::{Cell,RefCell};
        thread_local! {
            static PUBLISHES: RefCell<Vec<DamageRect>> = const { RefCell::new(Vec::new()) };
            static DAMAGE: Cell<Option<DamageRect>> = const { Cell::new(None) };
            static PAINTS: Cell<usize> = const { Cell::new(0) };
        }
        #[derive(Clone,Copy,PartialEq,Eq)] pub enum Backend { Cpu,Copy,Network }
        #[derive(Clone,Copy)] struct Geometry { width:u32,height:u32 }
        struct Ui4Surface {
            frame:FrameHandle, window:u32, width:u32, height:u32, backend:Backend,
            broker_resized:bool, scale:u32, clear_buffers:[bool;2],
        }
        impl Ui4Surface { fn set_hex_region(&mut self,_:Option<&trueos_terminal::edit_life::EditLife>) {} }
        pub struct Show {
            backend:Backend, poisoned:bool, font_scale:u32, surface:Option<Ui4Surface>,
            initial_geometry:Option<Geometry>, pan:Option<()>,
            pan_budget:Option<crate::ui4::text_area::RasterBudget>, strips:[Option<()>;2],
        }
        impl Show {
            fn release_surface(&mut self) { panic!("unexpected surface replacement") }
@PRESENT_METHOD@
        }
        fn show_extent(columns:usize,rows:usize,_:u32)->Result<(u32,u32),&'static str> { Ok((columns as u32,rows as u32)) }
        fn create_surface(_:u32,_:u32,_:Backend,_:Option<Geometry>)->Result<Ui4Surface,&'static str> { panic!("unexpected new surface") }
        const OWNER:()=();
        fn publish_window_frame(_:(),_:u32,damage:DamageRect)->Result<u64,()> {
            PUBLISHES.with(|p|p.borrow_mut().push(damage));
            crate::ui4::display_transfer(); Ok(1)
        }
        fn paint()->Result<Option<DamageRect>,&'static str> {
            crate::ui4::paint(); PAINTS.set(PAINTS.get()+1); Ok(DAMAGE.get())
        }
        mod cpu {
            pub(super) fn present(_: &mut super::Ui4Surface,_:&[&[(char,Option<super::RgbaColor>)]],_:&[super::super::SegmentUpdate])
              ->Result<Option<super::DamageRect>,&'static str> { super::paint() }
        }
        mod copy {
            pub(super) async fn present(_: &mut super::Ui4Surface,_:&[&[(char,Option<super::RgbaColor>)]],_:&[super::super::SegmentUpdate],
                _:&mut bool,_:&mut Option<()>,_:&crate::ui4::text_area::RasterBudget,_:Option<&super::super::update::MatrixAreaSnapshot>,
                _:&mut [Option<()>;2],_:&[super::super::update::StripReveal])
              ->Result<Option<super::DamageRect>,&'static str> { super::paint() }
        }
        mod edit_life {
            pub(super) fn present(_: &mut super::Ui4Surface,_:&[&[(char,Option<super::RgbaColor>)]],_:Option<&trueos_terminal::edit_life::EditLife>,_:bool) {}
        }
        fn shell(backend:Backend)->Show { Show {
            backend, poisoned:false, font_scale:1, initial_geometry:None,
            pan:None,pan_budget:None,strips:[None;2],surface:Some(Ui4Surface {
                frame:FrameHandle(1),window:7,width:64,height:32,backend,broker_resized:false,scale:1,clear_buffers:[false;2],
            }),
        } }
        pub fn exercise() {
            use crate::ui4::{acquire_frame_buffer,FrameHandle,FramePoolError};
            // Reproduce the old failure using the real frame-pool functions:
            // swapping B without a broker update leaves A pinned and B front.
            crate::ui4::reset(); crate::ui4::paint();
            assert_eq!(acquire_frame_buffer(FrameHandle(1)), Err(FramePoolError::Busy));
            for backend in [Backend::Cpu,Backend::Copy] {
                crate::ui4::reset(); PUBLISHES.with(|p|p.borrow_mut().clear());
                DAMAGE.set(None); PAINTS.set(0);
                let mut shell=shell(backend);
                let batch=super::UpdateBatch { segments:vec![super::SegmentUpdate] };
                crate::block_on(shell.present(&[],64,32,&batch,&[],None,None)).unwrap();
                assert_eq!(PAINTS.get(),1);
                PUBLISHES.with(|p|assert_eq!(*p.borrow(), vec![DamageRect::FULL]));
                assert_eq!(acquire_frame_buffer(FrameHandle(1)).unwrap().buffer_index,0);
                // Preserve partial damage when it exists.
                crate::ui4::reset(); PUBLISHES.with(|p|p.borrow_mut().clear());
                DAMAGE.set(Some(DamageRect {width:4,height:7}));
                crate::block_on(shell.present(&[],64,32,&batch,&[],None,None)).unwrap();
                PUBLISHES.with(|p|assert_eq!(*p.borrow(), vec![DamageRect {width:4,height:7}]));
                // A true no-op never calls the painter and never swaps buffers.
                crate::ui4::reset(); PUBLISHES.with(|p|p.borrow_mut().clear()); PAINTS.set(0);
                crate::block_on(shell.present(&[],64,32,&super::UpdateBatch::default(),&[],None,None)).unwrap();
                assert_eq!(PAINTS.get(),0); PUBLISHES.with(|p|assert!(p.borrow().is_empty()));
                assert_eq!(acquire_frame_buffer(FrameHandle(1)).unwrap().buffer_index,1);
            }
        }
    }
}
#[test] fn swapped_front_always_reaches_broker_and_releases_old_display_reader() { shell3::show::exercise(); }
'''
    return template.replace("@FRAME_ITEMS@", frame_items).replace("@PRESENT_METHOD@", method)


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="shell3-publication-") as directory:
        path = Path(directory)
        (path / "harness.rs").write_text(harness_source())
        subprocess.run(["rustc", "--edition=2024", "--test", str(path / "harness.rs"),
                        "-o", str(path / "tests")], check=True)
        subprocess.run([str(path / "tests")], check=True)


if __name__ == "__main__":
    main()
