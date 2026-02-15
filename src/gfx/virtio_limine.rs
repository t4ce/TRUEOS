#![cfg(feature = "gfx_virgl")]

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

use crate::gfx::virtio_gpu_3d::{
    gpu_get_display_info, gpu_resource_attach_backing, gpu_resource_create_2d, gpu_resource_flush,
    gpu_set_scanout, gpu_transfer_to_host_2d,
};

// Virtio-gpu scanout expects B8G8R8X8 for our current pipeline.
const FORMAT_B8G8R8X8_UNORM: u32 = 2;

static STARTED: AtomicBool = AtomicBool::new(false);
static ENABLED: AtomicBool = AtomicBool::new(false);
static NEXT_RES_ID: AtomicU32 = AtomicU32::new(0xA000);

fn alloc_res_id() -> u32 {
    let id = NEXT_RES_ID.fetch_add(1, Ordering::Relaxed);
    if id == 0 {
        NEXT_RES_ID.store(0xA000, Ordering::Relaxed);
        0xA000
    } else {
        id
    }
}

struct MirrorState {
    scanout_id: u32,
    res_id: u32,
    present_w: u32,
    present_h: u32,
}

impl MirrorState {
    const fn new() -> Self {
        Self {
            scanout_id: 0,
            res_id: 0,
            present_w: 0,
            present_h: 0,
        }
    }

    fn disable(&mut self) {
        self.scanout_id = 0;
        self.res_id = 0;
        self.present_w = 0;
        self.present_h = 0;
    }
}

static STATE: Mutex<MirrorState> = Mutex::new(MirrorState::new());

fn limine_fb_phys_addr(ptr: *mut u8) -> Option<u64> {
    let addr = ptr as u64;
    if let Some(phys) = crate::limine::try_as_phys_addr(addr) {
        crate::log!(
            "virtio-limine: phys via try_as_phys_addr virt=0x{:X} phys=0x{:X}\n",
            ptr as usize,
            phys
        );
        return Some(phys);
    }
    let phys = crate::phys::virt_to_phys_checked(ptr as *const u8);
    if let Some(p) = phys {
        crate::log!(
            "virtio-limine: phys via virt_to_phys_checked virt=0x{:X} phys=0x{:X}\n",
            ptr as usize,
            p
        );
    } else {
        crate::log!(
            "virtio-limine: phys translate failed virt=0x{:X}\n",
            ptr as usize
        );
    }
    phys
}

pub fn disable() {
    ENABLED.store(false, Ordering::Release);
    STATE.lock().disable();
}

pub fn enable(framebuffers: Option<&'static ::limine::response::FramebufferResponse>) -> bool {
    let Some(resp) = framebuffers else {
        return false;
    };
    let Some(fb) = resp.framebuffers().next() else {
        return false;
    };

    if fb.bpp() != 32 {
        crate::log!("virtio-limine: fb bpp={} unsupported\n", fb.bpp());
        return false;
    }

    let addr = fb.addr();
    if addr.is_null() {
        crate::log!("virtio-limine: fb addr null\n");
        return false;
    }

    let pitch = fb.pitch() as usize;
    if (pitch % 4) != 0 {
        crate::log!("virtio-limine: fb pitch {} not multiple of 4\n", pitch);
        return false;
    }

    let fb_w = fb.width() as u32;
    let fb_h = fb.height() as u32;
    if fb_w == 0 || fb_h == 0 {
        return false;
    }

    crate::log!(
        "virtio-limine: fb virt=0x{:X} {}x{} pitch={} bytes={}\n",
        addr as usize,
        fb_w,
        fb_h,
        pitch,
        pitch.saturating_mul(fb_h as usize)
    );

    let Some(backing_phys) = limine_fb_phys_addr(addr) else {
        crate::log!("virtio-limine: virt->phys failed addr=0x{:X}\n", addr as usize);
        return false;
    };

    let stride_pixels = (pitch / 4) as u32;
    let backing_len = pitch
        .saturating_mul(fb_h as usize)
        .min(u32::MAX as usize) as u32;

    let Some((scanout_id, disp_w, disp_h)) = gpu_get_display_info(2000) else {
        crate::log!("virtio-limine: get_display_info failed\n");
        return false;
    };

    crate::log!(
        "virtio-limine: display_info scanout={} {}x{}\n",
        scanout_id,
        disp_w,
        disp_h
    );

    let present_w = disp_w.min(fb_w).max(1);
    let present_h = disp_h.min(fb_h).max(1);

    let res_id = alloc_res_id();
    let ok_create = gpu_resource_create_2d(res_id, FORMAT_B8G8R8X8_UNORM, stride_pixels, fb_h, 2000);
    if !ok_create {
        crate::log!("virtio-limine: resource_create_2d failed\n");
        return false;
    }
    let ok_attach = gpu_resource_attach_backing(res_id, backing_phys, backing_len, 2000);
    if !ok_attach {
        crate::log!("virtio-limine: attach_backing failed\n");
        return false;
    }
    let ok_scanout = gpu_set_scanout(scanout_id, res_id, present_w, present_h, 2000);
    if !ok_scanout {
        crate::log!("virtio-limine: set_scanout failed\n");
        return false;
    }

    crate::log!(
        "virtio-limine: set_scanout ok res={} present={}x{} stride_px={} backing_phys=0x{:X} backing_len={}\n",
        res_id,
        present_w,
        present_h,
        stride_pixels,
        backing_phys,
        backing_len
    );

    // Make sure the host resource is initialized from guest memory at least once.
    // Do not hold the global virtio-gpu lock across multiple ctrlq submissions.
    // This reduces contention with backend switching and avoids apparent deadlocks/timeouts.
    let ok_tth = gpu_transfer_to_host_2d(res_id, present_w, present_h, 1000);
    let ok_flush = gpu_resource_flush(res_id, present_w, present_h, 1000);
    crate::log!(
        "virtio-limine: initial transfer_to_host={} flush={}\n",
        ok_tth as u8,
        ok_flush as u8
    );

    {
        let mut st = STATE.lock();
        st.scanout_id = scanout_id;
        st.res_id = res_id;
        st.present_w = present_w;
        st.present_h = present_h;
    }

    ENABLED.store(true, Ordering::Release);
    crate::log!(
        "virtio-limine: enabled fb={}x{} pitch={} scanout={} present={}x{} res={}\n",
        fb_w,
        fb_h,
        pitch,
        scanout_id,
        present_w,
        present_h,
        res_id
    );
    true
}

pub fn ensure_task_started(spawner: &Spawner) {
    if STARTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    let _ = spawner.spawn(virtio_limine_mirror_task());
}

#[embassy_executor::task]
async fn virtio_limine_mirror_task() {
    // 30Hz is plenty for console + simple animations; keeps overhead low.
    let period = EmbassyDuration::from_millis(33);

    let mut tick: u32 = 0;
    let mut last_ok: i8 = -1;

    loop {
        if ENABLED.load(Ordering::Acquire) {
            let guard = STATE.lock();
            let w = guard.present_w;
            let h = guard.present_h;
            let res = guard.res_id;
            // Drop lock before issuing virtio commands.
            core::mem::drop(guard);

            // Keep global lock hold times short (see note in enable()).
            let ok_tth = gpu_transfer_to_host_2d(res, w, h, 1000);
            let ok_flush = gpu_resource_flush(res, w, h, 1000);

            let ok = (ok_tth && ok_flush) as i8;

            // Only log on state change or occasional heartbeat.
            tick = tick.wrapping_add(1);
            if ok != last_ok {
                last_ok = ok;
                crate::log!(
                    "virtio-limine: mirror state ok={} (tth={} flush={}) res={} {}x{}\n",
                    ok,
                    ok_tth as u8,
                    ok_flush as u8,
                    res,
                    w,
                    h
                );
            } else if ok == 0 {
                // If failing continuously, log at ~1Hz.
                if (tick % 30) == 0 {
                    crate::log!(
                        "virtio-limine: mirror failing (tth={} flush={}) res={} {}x{}\n",
                        ok_tth as u8,
                        ok_flush as u8,
                        res,
                        w,
                        h
                    );
                }
            }
        }
        Timer::after(period).await;
    }
}
