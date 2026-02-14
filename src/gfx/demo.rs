use trueos_gfx_core::{
    BufferDesc, BufferUsage, ColorFormat, Command, CommandList, Extent2D, MemoryType, PipelineDesc,
    SwapchainDesc, VertexLayout, Viewport,
};

use core::sync::atomic::{AtomicBool, Ordering};

static DEMO_READY: AtomicBool = AtomicBool::new(false);
static mut PIPELINE: trueos_gfx_core::PipelineId = trueos_gfx_core::PipelineId::invalid();
static mut VERTEX_BUF: trueos_gfx_core::BufferId = trueos_gfx_core::BufferId::invalid();

#[repr(C)]
#[derive(Clone, Copy)]
struct Vertex {
    pos: [f32; 2],
    rgb: [u8; 3],
    _pad: u8,
}

fn ensure_resources(
    dev: &mut dyn trueos_gfx_core::GfxDevice,
    pres: &mut dyn trueos_gfx_core::GfxPresent,
) {
    if DEMO_READY.load(Ordering::Acquire) {
        return;
    }

    let swap = pres.swapchain_desc();
    if swap.extent.width == 0 || swap.extent.height == 0 {
        return;
    }

    // (Re)configure swapchain to the current extent/format.
    let _ = pres.configure_swapchain(SwapchainDesc {
        format: swap.format,
        extent: swap.extent,
    });

    let layout = VertexLayout {
        stride: core::mem::size_of::<Vertex>() as u16,
        pos_offset: 0,
        color_offset: 8,
        color_format: ColorFormat::RgbU8,
    };

    let pipeline = match dev.create_pipeline(PipelineDesc {
        vertex_layout: layout,
        vs: None,
        fs: None,
    }) {
        Ok(p) => p,
        Err(_) => return,
    };

    let vbuf = match dev.create_buffer(BufferDesc {
        size: core::mem::size_of::<[Vertex; 3]>() as u64,
        usage: BufferUsage::Vertex,
        memory: MemoryType::HostVisible,
    }) {
        Ok(b) => b,
        Err(_) => {
            dev.destroy_pipeline(pipeline);
            return;
        }
    };

    let verts = [
        Vertex {
            pos: [0.0, 0.65],
            rgb: [255, 0, 0],
            _pad: 0,
        },
        Vertex {
            pos: [-0.7, -0.55],
            rgb: [0, 255, 0],
            _pad: 0,
        },
        Vertex {
            pos: [0.7, -0.55],
            rgb: [0, 0, 255],
            _pad: 0,
        },
    ];

    let bytes: &[u8] = unsafe {
        core::slice::from_raw_parts(
            verts.as_ptr() as *const u8,
            core::mem::size_of_val(&verts),
        )
    };

    if dev.write_buffer(vbuf, 0, bytes).is_err() {
        dev.destroy_buffer(vbuf);
        dev.destroy_pipeline(pipeline);
        return;
    }

    unsafe {
        PIPELINE = pipeline;
        VERTEX_BUF = vbuf;
    }

    DEMO_READY.store(true, Ordering::Release);
}

pub fn tick(dev: &mut dyn trueos_gfx_core::GfxDevice, pres: &mut dyn trueos_gfx_core::GfxPresent) {
    ensure_resources(dev, pres);
    if !DEMO_READY.load(Ordering::Acquire) {
        return;
    }

    let swap = pres.swapchain_desc();
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

    let mut list = CommandList::new();
    list.push(Command::ClearColor { rgb: 0x00_08_18_30 });
    list.push(Command::SetViewport(vp));
    list.push(Command::BindPipeline(pipeline));
    list.push(Command::BindVertexBuffer { buffer: vbuf, offset: 0 });
    list.push(Command::Draw {
        vertex_count: 3,
        first_vertex: 0,
    });
    list.push(Command::Present);

    let _ = dev.submit(list.as_buffer());
}

pub fn default_extent(pres: &mut dyn trueos_gfx_core::GfxPresent) -> Extent2D {
    pres.swapchain_desc().extent
}
