use spin::Mutex;

const FORCEWAKE_RENDER: usize = 0x0A278;
const FORCEWAKE_GT: usize = 0x0A188;
const FORCEWAKE_ACK_RENDER: usize = 0x0D84;
const FORCEWAKE_ACK_GT: usize = 0x130044;
const FORCEWAKE_KERNEL: u32 = 1 << 0;
const FORCEWAKE_FALLBACK: u32 = 1 << 15;
const FORCEWAKE_POLL_ITERS: usize = 20_000;
const RCS_RING_BASE: usize = 0x0000_2000;
const RCS_RING_IMR: usize = RCS_RING_BASE + 0xA8;
const CURSOR_A_OFFSET: usize = 0x70080;
const CURSOR_B_OFFSET: usize = 0x700C0;
const CURSOR_C_OFFSET: usize = 0x700E0;
const CURSOR_D_OFFSET: usize = 0x73080;

#[derive(Copy, Clone, Debug)]
pub struct RenderWarmState {
    pub device_id: u16,
    pub revision_id: u8,
    pub mmio_base: usize,
    pub mmio_len: usize,
}

static WARM_STATE: Mutex<Option<RenderWarmState>> = Mutex::new(None);

pub(crate) fn warm_once(dev: crate::intel::Dev) -> RenderWarmState {
    let warm = RenderWarmState {
        device_id: dev.device_id,
        revision_id: dev.revision_id,
        mmio_base: dev.mmio as usize,
        mmio_len: dev.mmio_len,
    };
    *WARM_STATE.lock() = Some(warm);
    warm
}

pub fn warm_state() -> Option<RenderWarmState> {
    *WARM_STATE.lock()
}

pub fn log_cursor_plane_info(warm: RenderWarmState) {
    let caps = cursor_plane_caps(warm.device_id);
    crate::log!(
        "intel/display: cursor-plane platform={} rev=0x{:02X} max={}x{} pipes={} layout={} regs=A:0x{:X},B:0x{:X},C:0x{:X},D:0x{:X}\n",
        caps.platform,
        warm.revision_id,
        caps.max_width,
        caps.max_height,
        caps.pipe_count,
        caps.layout,
        CURSOR_A_OFFSET,
        CURSOR_B_OFFSET,
        CURSOR_C_OFFSET,
        CURSOR_D_OFFSET
    );
}

pub fn log_sprite_plane_info(warm: RenderWarmState) {
    let caps = sprite_plane_caps(warm.device_id);
    crate::log!(
        "intel/display: sprite-planes platform={} display_ver={} pipes={} overlays/pipe={} type=universal props=rotation:{} reflect_x:{} alpha:1 blend:pixel-none|premulti|coverage zpos:immutable csc:{} range:limited|full scaler:{} damage_clips:{}\n",
        caps.platform,
        caps.display_ver,
        caps.pipe_count,
        caps.overlays_per_pipe,
        caps.rotation,
        caps.reflect_x as u8,
        caps.csc,
        caps.scaling_filter,
        caps.damage_clips as u8
    );
}

pub fn forcewake_render_acquire(warm: RenderWarmState) -> bool {
    let dev = crate::intel::Dev {
        bus: 0,
        slot: 0,
        function: 0,
        device_id: warm.device_id,
        revision_id: warm.revision_id,
        mmio: warm.mmio_base as *mut u8,
        mmio_len: warm.mmio_len,
    };

    crate::intel::mmio_write(
        dev,
        FORCEWAKE_RENDER,
        crate::intel::mask_dis(FORCEWAKE_KERNEL | FORCEWAKE_FALLBACK),
    );
    let render_cleared = wait_eq(
        dev,
        FORCEWAKE_ACK_RENDER,
        FORCEWAKE_KERNEL | FORCEWAKE_FALLBACK,
        0,
        FORCEWAKE_POLL_ITERS,
    );

    crate::intel::mmio_write(dev, FORCEWAKE_RENDER, crate::intel::mask_en(FORCEWAKE_KERNEL));
    let render_ok = wait_eq(
        dev,
        FORCEWAKE_ACK_RENDER,
        FORCEWAKE_KERNEL,
        FORCEWAKE_KERNEL,
        FORCEWAKE_POLL_ITERS,
    );

    crate::intel::mmio_write(dev, FORCEWAKE_GT, crate::intel::mask_en(FORCEWAKE_KERNEL));
    let gt_ok = wait_eq(
        dev,
        FORCEWAKE_ACK_GT,
        FORCEWAKE_KERNEL,
        FORCEWAKE_KERNEL,
        FORCEWAKE_POLL_ITERS,
    );

    crate::log!(
        "intel/render: forcewake render_cleared={} render_ack=0x{:08X} gt_ack=0x{:08X} ok={}\n",
        render_cleared as u8,
        crate::intel::mmio_read(dev, FORCEWAKE_ACK_RENDER),
        crate::intel::mmio_read(dev, FORCEWAKE_ACK_GT),
        (render_ok && gt_ok) as u8
    );

    render_ok && gt_ok
}

pub fn forcewake_render_sanity(warm: RenderWarmState) {
    let dev = crate::intel::Dev {
        bus: 0,
        slot: 0,
        function: 0,
        device_id: warm.device_id,
        revision_id: warm.revision_id,
        mmio: warm.mmio_base as *mut u8,
        mmio_len: warm.mmio_len,
    };
    let before = crate::intel::mmio_read(dev, RCS_RING_IMR);
    let toggled = before ^ 0x0000_0001;
    crate::intel::mmio_write(dev, RCS_RING_IMR, toggled);
    let after = crate::intel::mmio_read(dev, RCS_RING_IMR);
    crate::intel::mmio_write(dev, RCS_RING_IMR, before);
    let restored = crate::intel::mmio_read(dev, RCS_RING_IMR);
    crate::log!(
        "intel/render: sanity reg=RCS_IMR before=0x{:08X} wrote=0x{:08X} after=0x{:08X} restored=0x{:08X}\n",
        before,
        toggled,
        after,
        restored
    );
}

fn wait_eq(dev: crate::intel::Dev, reg: usize, mask: u32, want: u32, n: usize) -> bool {
    for _ in 0..n {
        if (crate::intel::mmio_read(dev, reg) & mask) == want {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

struct CursorPlaneCaps {
    platform: &'static str,
    layout: &'static str,
    max_width: u16,
    max_height: u16,
    pipe_count: u8,
}

struct SpritePlaneCaps {
    platform: &'static str,
    display_ver: u8,
    pipe_count: u8,
    overlays_per_pipe: u8,
    rotation: &'static str,
    reflect_x: bool,
    csc: &'static str,
    scaling_filter: &'static str,
    damage_clips: bool,
}

fn cursor_plane_caps(device_id: u16) -> CursorPlaneCaps {
    match device_id {
        0x4680 | 0x4682 | 0x4688 | 0x468A | 0x468B | 0x4690 | 0x4692 | 0x4693 => {
            CursorPlaneCaps {
                platform: "ADL-S",
                layout: "TGL/XE_D",
                max_width: 256,
                max_height: 256,
                pipe_count: 4,
            }
        }
        0x46A0 | 0x46A1 | 0x46A2 | 0x46A3 | 0x46A6 | 0x46A8 | 0x46AA | 0x462A
        | 0x4626 | 0x4628 | 0x46B0 | 0x46B1 | 0x46B2 | 0x46B3 => CursorPlaneCaps {
            platform: "ADL-P/N",
            layout: "TGL/XE_LPD",
            max_width: 256,
            max_height: 256,
            pipe_count: 4,
        },
        _ => CursorPlaneCaps {
            platform: "unknown",
            layout: "generic",
            max_width: 256,
            max_height: 256,
            pipe_count: 4,
        },
    }
}

fn sprite_plane_caps(device_id: u16) -> SpritePlaneCaps {
    match device_id {
        0x4680 | 0x4682 | 0x4688 | 0x468A | 0x468B | 0x4690 | 0x4692 | 0x4693 => {
            SpritePlaneCaps {
                platform: "ADL-S",
                display_ver: 13,
                pipe_count: 4,
                overlays_per_pipe: 4,
                rotation: "0|180",
                reflect_x: true,
                csc: "BT601|BT709|BT2020",
                scaling_filter: "default|nearest",
                damage_clips: true,
            }
        }
        0x46A0 | 0x46A1 | 0x46A2 | 0x46A3 | 0x46A6 | 0x46A8 | 0x46AA | 0x462A
        | 0x4626 | 0x4628 | 0x46B0 | 0x46B1 | 0x46B2 | 0x46B3 => SpritePlaneCaps {
            platform: "ADL-P/N",
            display_ver: 13,
            pipe_count: 4,
            overlays_per_pipe: 4,
            rotation: "0|180",
            reflect_x: true,
            csc: "BT601|BT709|BT2020",
            scaling_filter: "default|nearest",
            damage_clips: true,
        },
        _ => SpritePlaneCaps {
            platform: "unknown",
            display_ver: 13,
            pipe_count: 4,
            overlays_per_pipe: 4,
            rotation: "0|180",
            reflect_x: true,
            csc: "BT601|BT709|BT2020",
            scaling_filter: "default|nearest",
            damage_clips: true,
        },
    }
}
