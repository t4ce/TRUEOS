use trueos_gfx_core::{
    BufferDesc, BufferUsage, ColorFormat, Command, CommandList, Extent2D, GfxContext, MemoryType,
    PipelineDesc, SwapchainDesc, VertexLayout, Viewport,
};

use core::sync::atomic::{AtomicBool, Ordering};

static DEMO_READY: AtomicBool = AtomicBool::new(false);
static DEMO_SPIN_RUNNING: AtomicBool = AtomicBool::new(false);
static mut PIPELINE: trueos_gfx_core::PipelineId = trueos_gfx_core::PipelineId::invalid();
static mut VERTEX_BUF: trueos_gfx_core::BufferId = trueos_gfx_core::BufferId::invalid();

#[repr(C)]
#[derive(Clone, Copy)]
struct Vertex {
    pos: [f32; 2],
    rgb: [u8; 3],
    _pad: u8,
}

fn ensure_resources(ctx: &mut dyn GfxContext) {
    if DEMO_READY.load(Ordering::Acquire) {
        return;
    }

    let swap = ctx.swapchain_desc();
    if swap.extent.width == 0 || swap.extent.height == 0 {
        return;
    }

    // (Re)configure swapchain to the current extent/format.
    let _ = ctx.configure_swapchain(SwapchainDesc {
        format: swap.format,
        extent: swap.extent,
    });

    let layout = VertexLayout {
        stride: core::mem::size_of::<Vertex>() as u16,
        pos_offset: 0,
        color_offset: 8,
        color_format: ColorFormat::RgbU8,
    };

    let pipeline = match ctx.create_pipeline(PipelineDesc {
        vertex_layout: layout,
        vs: None,
        fs: None,
    }) {
        Ok(p) => p,
        Err(_) => return,
    };

    let vbuf = match ctx.create_buffer(BufferDesc {
        size: core::mem::size_of::<[Vertex; 6]>() as u64,
        usage: BufferUsage::Vertex,
        memory: MemoryType::HostVisible,
    }) {
        Ok(b) => b,
        Err(_) => {
            ctx.destroy_pipeline(pipeline);
            return;
        }
    };

    let verts = rect_verts(0.0);

    let bytes: &[u8] = unsafe {
        core::slice::from_raw_parts(
            verts.as_ptr() as *const u8,
            core::mem::size_of_val(&verts),
        )
    };

    if ctx.write_buffer(vbuf, 0, bytes).is_err() {
        ctx.destroy_buffer(vbuf);
        ctx.destroy_pipeline(pipeline);
        return;
    }

    unsafe {
        PIPELINE = pipeline;
        VERTEX_BUF = vbuf;
    }

    DEMO_READY.store(true, Ordering::Release);
}

pub fn tick(ctx: &mut dyn GfxContext) {
    tick_rotating_rect(ctx, 0.0);
}

pub fn tick_rotating_rect(ctx: &mut dyn GfxContext, angle_rad: f32) {
    ensure_resources(ctx);
    if !DEMO_READY.load(Ordering::Acquire) {
        return;
    }

    let swap = ctx.swapchain_desc();
    if swap.extent.width == 0 || swap.extent.height == 0 {
        return;
    }

    let vp = Viewport {
        x: 0,
        y: 0,
        width: swap.extent.width as i32,
        height: swap.extent.height as i32,
    };

    let (pipeline, vbuf) = unsafe { (PIPELINE, VERTEX_BUF) };

    let verts = rect_verts(angle_rad);
    let bytes: &[u8] = unsafe {
        core::slice::from_raw_parts(
            verts.as_ptr() as *const u8,
            core::mem::size_of_val(&verts),
        )
    };
    let _ = ctx.write_buffer(vbuf, 0, bytes);

    let mut list = CommandList::new();
    list.push(Command::SetViewport(vp));
    list.push(Command::ClearRect {
        rgb: 0x00_08_18_30,
        x: 0,
        y: 0,
        width: swap.extent.width,
        height: swap.extent.height,
    });
    list.push(Command::BindPipeline(pipeline));
    list.push(Command::BindVertexBuffer { buffer: vbuf, offset: 0 });
    list.push(Command::Draw {
        vertex_count: 6,
        first_vertex: 0,
    });
    list.push(Command::Present);

    let _ = ctx.submit(list.as_buffer());
}

fn rect_verts(angle_rad: f32) -> [Vertex; 6] {
    // Rectangle centered at origin in NDC.
    let hw = 0.55f32;
    let hh = 0.35f32;

    let c = libm::cosf(angle_rad);
    let s = libm::sinf(angle_rad);

    let rot = |x: f32, y: f32| -> [f32; 2] {
        let xr = x * c - y * s;
        let yr = x * s + y * c;
        [xr, yr]
    };

    let p0 = rot(-hw, -hh);
    let p1 = rot(hw, -hh);
    let p2 = rot(hw, hh);
    let p3 = rot(-hw, hh);

    // Two triangles: (p0,p1,p2) and (p0,p2,p3)
    [
        Vertex { pos: p0, rgb: [255, 0, 255], _pad: 0 },
        Vertex { pos: p1, rgb: [0, 255, 255], _pad: 0 },
        Vertex { pos: p2, rgb: [255, 255, 0], _pad: 0 },
        Vertex { pos: p0, rgb: [255, 0, 255], _pad: 0 },
        Vertex { pos: p2, rgb: [255, 255, 0], _pad: 0 },
        Vertex { pos: p3, rgb: [0, 255, 0], _pad: 0 },
    ]
}

pub fn spawn_spin_rect_60hz(spawner: &embassy_executor::Spawner) {
    if DEMO_SPIN_RUNNING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        crate::log!("gfx: spin already running\n");
        return;
    }

    if let Some(ap) = crate::runtime::first_ap_spawner() {
        if ap.spawn(gfx_spin_task()).is_ok() {
            crate::log!("gfx: spin task spawned on AP1\n");
            return;
        }
    }

    if spawner.spawn(gfx_spin_task()).is_err() {
        DEMO_SPIN_RUNNING.store(false, Ordering::Release);
        crate::log!("gfx: spin task spawn failed\n");
    }
}

#[embassy_executor::task]
async fn gfx_spin_task() {
    use embassy_time::{Duration as EmbassyDuration, Timer};

    let start_ticks = embassy_time_driver::now();
    let tick_hz = embassy_time_driver::TICK_HZ as u64;
    let omega = (2.0 * core::f32::consts::PI) / 3.0; // rad/s

    crate::log!("gfx: spinning rect @60Hz (cw, 360deg/3s)\n");
    let mut frame: u64 = 0;
    loop {
        frame = frame.wrapping_add(1);
        let target = start_ticks.saturating_add(frame.saturating_mul(tick_hz) / 60);
        while embassy_time_driver::now() < target {
            Timer::after(EmbassyDuration::from_millis(1)).await;
        }

        let t = (frame as f32) * (1.0 / 60.0);
        let angle = -omega * t;
        let _ = crate::gfx::with_context(|ctx| {
            tick_rotating_rect(ctx, angle);
        });
    }
}

pub fn default_extent(ctx: &mut dyn GfxContext) -> Extent2D {
    ctx.swapchain_desc().extent
}
