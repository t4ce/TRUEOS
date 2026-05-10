use alloc::vec;
use alloc::vec::Vec;
use core::ptr::NonNull;

use trueos_gfx_core::{
    BlendDesc, BufferDesc, BufferId, ColorFormat, Command, CommandBuffer, DeviceCaps, Error,
    Extent2D, FenceId, GfxDevice, GfxPresent, ImageDesc, ImageFormat, ImageId, MapMode,
    MappedRange, PipelineDesc, PipelineId, Result, SamplerDesc, SamplerFilter, SamplerWrap,
    ShaderDesc, ShaderId, SwapchainDesc, TexCoordFormat, Viewport,
};

pub struct IntelGfxBackend {
    framebuffers: Option<&'static ::limine::response::FramebufferResponse>,
    swapchain: SwapchainDesc,
    fence_seq: u64,
    buffers: Vec<Option<SwBuffer>>,
    pipelines: Vec<Option<PipelineDesc>>,
    images: Vec<Option<SwImage>>,
    state: DrawState,
    cursor: Option<HwCursorState>,
}

const CURSOR_W: usize = 64;
const CURSOR_H: usize = 64;
const CURSOR_BYTES: usize = CURSOR_W * CURSOR_H * 4;

const CURCNTR_A: usize = 0x70080;
const CURBASE_A: usize = 0x70084;
const CURPOS_A: usize = 0x70088;

const CURSOR_ENABLE: u32 = 1u32 << 31;
const MCURSOR_MODE_64_ARGB_AX: u32 = 0x20 | 0x07;
const CURSOR_POS_Y_SIGN: u32 = 1u32 << 31;
const CURSOR_POS_X_SIGN: u32 = 1u32 << 15;

struct HwCursorState {
    mmio_base: NonNull<u8>,
    mmio_len: usize,
    phys: u64,
    virt: *mut u8,
    hot_x: u32,
    hot_y: u32,
    defined: bool,
}

unsafe impl Send for HwCursorState {}
unsafe impl Sync for HwCursorState {}

struct SwBuffer {
    data: Vec<u8>,
    _desc: BufferDesc,
}

struct SwImage {
    width: u32,
    height: u32,
    format: ImageFormat,
    data: Vec<u8>,
}

#[derive(Clone, Copy)]
struct DrawState {
    viewport: Option<Viewport>,
    pipeline: Option<PipelineId>,
    vertex_buffer: Option<(BufferId, usize)>,
    image: Option<ImageId>,
    sampler: SamplerDesc,
    blend: BlendDesc,
}

impl Default for DrawState {
    fn default() -> Self {
        Self {
            viewport: None,
            pipeline: None,
            vertex_buffer: None,
            image: None,
            sampler: SamplerDesc::default_2d(),
            blend: BlendDesc::disabled(),
        }
    }
}

struct FramebufferTarget {
    addr: *mut u8,
    pitch: usize,
    width: usize,
    height: usize,
}

#[derive(Clone, Copy)]
struct SwVertex {
    x: f32,
    y: f32,
    u: f32,
    v: f32,
    r: f32,
    g: f32,
    b: f32,
    a: f32,
}

impl IntelGfxBackend {
    pub fn init(
        framebuffers: Option<&'static ::limine::response::FramebufferResponse>,
    ) -> Option<Self> {
        if !crate::gfx::intel::has_claimed_device() {
            return None;
        }

        let (w, h) = first_fb_dimensions(framebuffers)?;
        let swapchain = SwapchainDesc {
            format: ImageFormat::Rgbx8888,
            extent: Extent2D {
                width: w,
                height: h,
            },
        };

        crate::log!("gfx: using intel backend (software-raster path)\n");

        let cursor = Self::init_hw_cursor_state();
        if cursor.is_none() {
            crate::log!("gfx-intel: hw cursor init unavailable\n");
        }

        Some(Self {
            framebuffers,
            swapchain,
            fence_seq: 1,
            buffers: Vec::new(),
            pipelines: Vec::new(),
            images: Vec::new(),
            state: DrawState::default(),
            cursor,
        })
    }

    fn init_hw_cursor_state() -> Option<HwCursorState> {
        let info = crate::gfx::intel::first_claimed_device()?;
        if info.mmio_len <= CURPOS_A + 4 {
            return None;
        }

        let (phys, virt) = crate::pci::dma::alloc(CURSOR_BYTES, 4096)?;
        if virt.is_null() {
            return None;
        }
        unsafe { core::ptr::write_bytes(virt, 0, CURSOR_BYTES) };

        Some(HwCursorState {
            mmio_base: info.mmio_base,
            mmio_len: info.mmio_len,
            phys,
            virt,
            hot_x: 0,
            hot_y: 0,
            defined: false,
        })
    }

    #[inline]
    fn mmio_write32(base: NonNull<u8>, off: usize, value: u32) {
        let ptr = unsafe { base.as_ptr().add(off) as *mut u32 };
        unsafe { core::ptr::write_volatile(ptr, value) };
    }

    #[inline]
    fn mmio_read32(base: NonNull<u8>, off: usize) -> u32 {
        let ptr = unsafe { base.as_ptr().add(off) as *const u32 };
        unsafe { core::ptr::read_volatile(ptr) }
    }

    fn program_cursor(cur: &HwCursorState) {
        let base = cur.mmio_base;
        let ctl = CURSOR_ENABLE | MCURSOR_MODE_64_ARGB_AX;
        Self::mmio_write32(base, CURBASE_A, (cur.phys & !0xFFF) as u32);
        Self::mmio_write32(base, CURCNTR_A, ctl);
        let _ = Self::mmio_read32(base, CURCNTR_A);
    }

    fn encode_cursor_pos(x: i32, y: i32) -> u32 {
        let x_mag = x.unsigned_abs().min(0x7FFF);
        let y_mag = y.unsigned_abs().min(0x7FFF);
        let mut v = ((y_mag as u32) << 16) | (x_mag as u32);
        if x < 0 {
            v |= CURSOR_POS_X_SIGN;
        }
        if y < 0 {
            v |= CURSOR_POS_Y_SIGN;
        }
        v
    }

    fn move_cursor(cur: &HwCursorState, x: i32, y: i32) {
        let px = x.saturating_sub(cur.hot_x as i32);
        let py = y.saturating_sub(cur.hot_y as i32);
        let v = Self::encode_cursor_pos(px, py);
        Self::mmio_write32(cur.mmio_base, CURPOS_A, v);
        let _ = Self::mmio_read32(cur.mmio_base, CURPOS_A);
    }

    fn slot_to_id(slot: usize) -> u32 {
        (slot as u32).saturating_add(1)
    }

    fn id_to_slot(raw: u32) -> Option<usize> {
        if raw == 0 {
            return None;
        }
        Some((raw - 1) as usize)
    }

    fn alloc_slot<T>(list: &mut Vec<Option<T>>, value: T) -> u32 {
        if let Some((idx, _)) = list.iter().enumerate().find(|(_, e)| e.is_none()) {
            list[idx] = Some(value);
            return Self::slot_to_id(idx);
        }
        let idx = list.len();
        list.push(Some(value));
        Self::slot_to_id(idx)
    }

    fn buffer_mut(&mut self, id: BufferId) -> Option<&mut SwBuffer> {
        let slot = Self::id_to_slot(id.raw())?;
        self.buffers.get_mut(slot)?.as_mut()
    }

    fn buffer_ref(&self, id: BufferId) -> Option<&SwBuffer> {
        let slot = Self::id_to_slot(id.raw())?;
        self.buffers.get(slot)?.as_ref()
    }

    fn pipeline_ref(&self, id: PipelineId) -> Option<&PipelineDesc> {
        let slot = Self::id_to_slot(id.raw())?;
        self.pipelines.get(slot)?.as_ref()
    }

    fn image_ref(&self, id: ImageId) -> Option<&SwImage> {
        let slot = Self::id_to_slot(id.raw())?;
        self.images.get(slot)?.as_ref()
    }

    fn current_fb(&self) -> Option<FramebufferTarget> {
        use ::limine::framebuffer::MemoryModel;

        let fb = self
            .framebuffers
            .and_then(|r| r.framebuffers().next())
            .or_else(|| {
                crate::limine::framebuffer_response().and_then(|r| r.framebuffers().next())
            })?;

        if fb.memory_model() != MemoryModel::RGB || fb.bpp() != 32 {
            return None;
        }

        Some(FramebufferTarget {
            addr: fb.addr(),
            pitch: fb.pitch() as usize,
            width: fb.width() as usize,
            height: fb.height() as usize,
        })
    }

    fn viewport_or_full(&self, fb: &FramebufferTarget) -> Viewport {
        self.state.viewport.unwrap_or(Viewport {
            x: 0,
            y: 0,
            width: fb.width as i32,
            height: fb.height as i32,
        })
    }

    fn fill_fb(&self, fb: &FramebufferTarget, rgb: u32) {
        for y in 0..fb.height {
            let row_ptr = unsafe { fb.addr.add(y.saturating_mul(fb.pitch)) as *mut u32 };
            let row = unsafe { core::slice::from_raw_parts_mut(row_ptr, fb.width) };
            row.fill(rgb & 0x00FF_FFFF);
        }
    }

    fn clear_rect(&self, fb: &FramebufferTarget, rgb: u32, x: u32, y: u32, w: u32, h: u32) {
        if w == 0 || h == 0 {
            return;
        }
        let min_x = (x as usize).min(fb.width);
        let min_y = (y as usize).min(fb.height);
        let max_x = min_x.saturating_add(w as usize).min(fb.width);
        let max_y = min_y.saturating_add(h as usize).min(fb.height);
        if min_x >= max_x || min_y >= max_y {
            return;
        }

        for yy in min_y..max_y {
            let row_ptr = unsafe { fb.addr.add(yy.saturating_mul(fb.pitch)) as *mut u32 };
            let row = unsafe { core::slice::from_raw_parts_mut(row_ptr, fb.width) };
            row[min_x..max_x].fill(rgb & 0x00FF_FFFF);
        }
    }

    fn draw(&mut self, fb: &FramebufferTarget, vertex_count: u32, first_vertex: u32) -> Result<()> {
        let Some(pipeline_id) = self.state.pipeline else {
            return Err(Error::Invalid);
        };
        let Some((vbuf_id, vb_offset)) = self.state.vertex_buffer else {
            return Err(Error::Invalid);
        };
        let Some(pipeline) = self.pipeline_ref(pipeline_id) else {
            return Err(Error::NotFound);
        };
        let Some(vbuf) = self.buffer_ref(vbuf_id) else {
            return Err(Error::NotFound);
        };

        let vp = self.viewport_or_full(fb);
        if vp.width <= 0 || vp.height <= 0 {
            return Ok(());
        }

        let is_textured = matches!(
            pipeline.vertex_layout.texcoord_format,
            TexCoordFormat::UvF32
        );
        let bound_image = self.state.image.and_then(|id| self.image_ref(id));

        let mut i = 0u32;
        while i + 2 < vertex_count {
            let a = self.read_vertex(vbuf, pipeline, first_vertex + i, vb_offset)?;
            let b = self.read_vertex(vbuf, pipeline, first_vertex + i + 1, vb_offset)?;
            let c = self.read_vertex(vbuf, pipeline, first_vertex + i + 2, vb_offset)?;
            self.raster_triangle(fb, vp, a, b, c, is_textured, bound_image)?;
            i = i.saturating_add(3);
        }

        Ok(())
    }

    fn read_vertex(
        &self,
        vbuf: &SwBuffer,
        pipeline: &PipelineDesc,
        index: u32,
        vb_offset: usize,
    ) -> Result<SwVertex> {
        let layout = pipeline.vertex_layout;
        let stride = layout.stride as usize;
        if stride == 0 {
            return Err(Error::Invalid);
        }

        let base = vb_offset.saturating_add((index as usize).saturating_mul(stride));
        if base.saturating_add(stride) > vbuf.data.len() {
            return Err(Error::Invalid);
        }

        let read_f32 = |off: usize| -> Result<f32> {
            let p = base.saturating_add(off);
            if p.saturating_add(4) > vbuf.data.len() {
                return Err(Error::Invalid);
            }
            let b = &vbuf.data[p..p + 4];
            Ok(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        };

        let pos_off = layout.pos_offset as usize;
        let x = read_f32(pos_off)?;
        let y = read_f32(pos_off.saturating_add(4))?;

        let col_off = base.saturating_add(layout.color_offset as usize);
        if col_off >= vbuf.data.len() {
            return Err(Error::Invalid);
        }

        let (r, g, b, a) = match layout.color_format {
            ColorFormat::RgbU8 => {
                if col_off.saturating_add(3) > vbuf.data.len() {
                    return Err(Error::Invalid);
                }
                (
                    vbuf.data[col_off] as f32,
                    vbuf.data[col_off + 1] as f32,
                    vbuf.data[col_off + 2] as f32,
                    255.0,
                )
            }
            ColorFormat::RgbaU8 => {
                if col_off.saturating_add(4) > vbuf.data.len() {
                    return Err(Error::Invalid);
                }
                (
                    vbuf.data[col_off] as f32,
                    vbuf.data[col_off + 1] as f32,
                    vbuf.data[col_off + 2] as f32,
                    vbuf.data[col_off + 3] as f32,
                )
            }
        };

        let (u, v) = match layout.texcoord_format {
            TexCoordFormat::None => (0.0, 0.0),
            TexCoordFormat::UvF32 => {
                let uv_off = layout.texcoord_offset as usize;
                (read_f32(uv_off)?, read_f32(uv_off.saturating_add(4))?)
            }
        };

        Ok(SwVertex {
            x,
            y,
            u,
            v,
            r,
            g,
            b,
            a,
        })
    }

    fn raster_triangle(
        &self,
        fb: &FramebufferTarget,
        vp: Viewport,
        v0: SwVertex,
        v1: SwVertex,
        v2: SwVertex,
        textured: bool,
        image: Option<&SwImage>,
    ) -> Result<()> {
        let to_px = |x: f32, y: f32| -> (f32, f32) {
            let w = (vp.width - 1).max(1) as f32;
            let h = (vp.height - 1).max(1) as f32;
            let sx = vp.x as f32 + libm::roundf(((x * 0.5) + 0.5) * w);
            let sy = vp.y as f32 + libm::roundf(((y * 0.5) + 0.5) * h);
            (sx, sy)
        };

        let (x0, y0) = to_px(v0.x, v0.y);
        let (x1, y1) = to_px(v1.x, v1.y);
        let (x2, y2) = to_px(v2.x, v2.y);

        let min_x = (libm::floorf(x0.min(x1.min(x2))) as i32).max(0);
        let min_y = (libm::floorf(y0.min(y1.min(y2))) as i32).max(0);
        let max_x =
            (libm::ceilf(x0.max(x1.max(x2))) as i32).min((fb.width as i32).saturating_sub(1));
        let max_y =
            (libm::ceilf(y0.max(y1.max(y2))) as i32).min((fb.height as i32).saturating_sub(1));

        if min_x > max_x || min_y > max_y {
            return Ok(());
        }

        let edge = |ax: f32, ay: f32, bx: f32, by: f32, px: f32, py: f32| -> f32 {
            (px - ax) * (by - ay) - (py - ay) * (bx - ax)
        };

        let area = edge(x0, y0, x1, y1, x2, y2);
        if area == 0.0 {
            return Ok(());
        }

        for y in min_y..=max_y {
            let row_ptr = unsafe { fb.addr.add((y as usize).saturating_mul(fb.pitch)) as *mut u32 };
            let row = unsafe { core::slice::from_raw_parts_mut(row_ptr, fb.width) };

            for x in min_x..=max_x {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;

                let w0 = edge(x1, y1, x2, y2, px, py);
                let w1 = edge(x2, y2, x0, y0, px, py);
                let w2 = edge(x0, y0, x1, y1, px, py);

                let inside = if area > 0.0 {
                    w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0
                } else {
                    w0 <= 0.0 && w1 <= 0.0 && w2 <= 0.0
                };
                if !inside {
                    continue;
                }

                let l0 = w0 / area;
                let l1 = w1 / area;
                let l2 = w2 / area;

                let mut src_r = l0 * v0.r + l1 * v1.r + l2 * v2.r;
                let mut src_g = l0 * v0.g + l1 * v1.g + l2 * v2.g;
                let mut src_b = l0 * v0.b + l1 * v1.b + l2 * v2.b;
                let mut src_a = l0 * v0.a + l1 * v1.a + l2 * v2.a;

                if textured {
                    let Some(img) = image else {
                        continue;
                    };
                    let u = l0 * v0.u + l1 * v1.u + l2 * v2.u;
                    let v = l0 * v0.v + l1 * v1.v + l2 * v2.v;
                    let (tr, tg, tb, ta) = sample_texture(img, u, v, self.state.sampler);
                    src_r = tr * src_r / 255.0;
                    src_g = tg * src_g / 255.0;
                    src_b = tb * src_b / 255.0;
                    src_a = ta * src_a / 255.0;
                }

                let src_r = src_r.clamp(0.0, 255.0);
                let src_g = src_g.clamp(0.0, 255.0);
                let src_b = src_b.clamp(0.0, 255.0);
                let src_a = src_a.clamp(0.0, 255.0);

                let dst = row[x as usize] & 0x00FF_FFFF;
                let dst_r = ((dst >> 16) & 0xFF) as f32;
                let dst_g = ((dst >> 8) & 0xFF) as f32;
                let dst_b = (dst & 0xFF) as f32;

                let (out_r, out_g, out_b) = if self.state.blend.enabled {
                    blend_rgb(
                        self.state.blend,
                        src_r,
                        src_g,
                        src_b,
                        src_a,
                        dst_r,
                        dst_g,
                        dst_b,
                    )
                } else {
                    (src_r, src_g, src_b)
                };

                let out = ((out_r as u32) << 16) | ((out_g as u32) << 8) | (out_b as u32);
                row[x as usize] = out & 0x00FF_FFFF;
            }
        }

        Ok(())
    }
}

#[inline]
fn wrap_coord(v: f32, wrap: SamplerWrap) -> f32 {
    match wrap {
        SamplerWrap::ClampToEdge => v.clamp(0.0, 1.0),
        SamplerWrap::Repeat => {
            let mut t = v - libm::floorf(v);
            if t < 0.0 {
                t += 1.0;
            }
            t
        }
    }
}

#[inline]
fn sample_texel(img: &SwImage, x: usize, y: usize) -> (f32, f32, f32, f32) {
    let xx = x.min((img.width.saturating_sub(1)) as usize);
    let yy = y.min((img.height.saturating_sub(1)) as usize);
    let off = yy
        .saturating_mul(img.width as usize)
        .saturating_add(xx)
        .saturating_mul(4);
    if off.saturating_add(4) > img.data.len() {
        return (255.0, 255.0, 255.0, 255.0);
    }
    let r = img.data[off] as f32;
    let g = img.data[off + 1] as f32;
    let b = img.data[off + 2] as f32;
    let a = if img.format == ImageFormat::Rgbx8888 {
        255.0
    } else {
        img.data[off + 3] as f32
    };
    (r, g, b, a)
}

fn sample_texture(img: &SwImage, u: f32, v: f32, sampler: SamplerDesc) -> (f32, f32, f32, f32) {
    let uu = wrap_coord(u, sampler.wrap_s);
    let vv = wrap_coord(v, sampler.wrap_t);

    let w = img.width.max(1) as f32;
    let h = img.height.max(1) as f32;
    let x = uu * (w - 1.0);
    let y = vv * (h - 1.0);

    match sampler.mag_filter {
        SamplerFilter::Nearest => {
            let tx = libm::roundf(x).clamp(0.0, w - 1.0) as usize;
            let ty = libm::roundf(y).clamp(0.0, h - 1.0) as usize;
            sample_texel(img, tx, ty)
        }
        SamplerFilter::Linear => {
            let x0f = libm::floorf(x).clamp(0.0, w - 1.0);
            let y0f = libm::floorf(y).clamp(0.0, h - 1.0);
            let x1f = (x0f + 1.0).min(w - 1.0);
            let y1f = (y0f + 1.0).min(h - 1.0);

            let x0 = x0f as usize;
            let y0 = y0f as usize;
            let x1 = x1f as usize;
            let y1 = y1f as usize;

            let tx = (x - x0f).clamp(0.0, 1.0);
            let ty = (y - y0f).clamp(0.0, 1.0);

            let c00 = sample_texel(img, x0, y0);
            let c10 = sample_texel(img, x1, y0);
            let c01 = sample_texel(img, x0, y1);
            let c11 = sample_texel(img, x1, y1);

            let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
            let i0 = (
                lerp(c00.0, c10.0, tx),
                lerp(c00.1, c10.1, tx),
                lerp(c00.2, c10.2, tx),
                lerp(c00.3, c10.3, tx),
            );
            let i1 = (
                lerp(c01.0, c11.0, tx),
                lerp(c01.1, c11.1, tx),
                lerp(c01.2, c11.2, tx),
                lerp(c01.3, c11.3, tx),
            );

            (
                lerp(i0.0, i1.0, ty),
                lerp(i0.1, i1.1, ty),
                lerp(i0.2, i1.2, ty),
                lerp(i0.3, i1.3, ty),
            )
        }
    }
}

#[inline]
fn factor_value(factor: trueos_gfx_core::BlendFactor, src_c: f32, src_a: f32, dst_c: f32) -> f32 {
    match factor {
        trueos_gfx_core::BlendFactor::Zero => 0.0,
        trueos_gfx_core::BlendFactor::One => 1.0,
        trueos_gfx_core::BlendFactor::SrcAlpha => (src_a / 255.0).clamp(0.0, 1.0),
        trueos_gfx_core::BlendFactor::OneMinusSrcAlpha => 1.0 - (src_a / 255.0).clamp(0.0, 1.0),
        trueos_gfx_core::BlendFactor::DstColor => (dst_c / 255.0).clamp(0.0, 1.0),
        trueos_gfx_core::BlendFactor::OneMinusSrcColor => 1.0 - (src_c / 255.0).clamp(0.0, 1.0),
    }
}

fn blend_rgb(
    blend: BlendDesc,
    src_r: f32,
    src_g: f32,
    src_b: f32,
    src_a: f32,
    dst_r: f32,
    dst_g: f32,
    dst_b: f32,
) -> (f32, f32, f32) {
    let sf_r = factor_value(blend.src, src_r, src_a, dst_r);
    let sf_g = factor_value(blend.src, src_g, src_a, dst_g);
    let sf_b = factor_value(blend.src, src_b, src_a, dst_b);

    let df_r = factor_value(blend.dst, src_r, src_a, dst_r);
    let df_g = factor_value(blend.dst, src_g, src_a, dst_g);
    let df_b = factor_value(blend.dst, src_b, src_a, dst_b);

    (
        (src_r * sf_r + dst_r * df_r).clamp(0.0, 255.0),
        (src_g * sf_g + dst_g * df_g).clamp(0.0, 255.0),
        (src_b * sf_b + dst_b * df_b).clamp(0.0, 255.0),
    )
}

impl GfxDevice for IntelGfxBackend {
    fn caps(&self) -> DeviceCaps {
        DeviceCaps::minimal_software()
    }

    fn create_buffer(&mut self, desc: BufferDesc) -> Result<BufferId> {
        let len = usize::try_from(desc.size).map_err(|_| Error::OutOfMemory)?;
        let data = vec![0u8; len];
        let raw = Self::alloc_slot(&mut self.buffers, SwBuffer { data, _desc: desc });
        Ok(BufferId::from_raw(raw))
    }

    fn destroy_buffer(&mut self, id: BufferId) {
        let Some(slot) = Self::id_to_slot(id.raw()) else {
            return;
        };
        if let Some(entry) = self.buffers.get_mut(slot) {
            *entry = None;
        }
    }

    fn create_shader(&mut self, _desc: ShaderDesc<'_>) -> Result<ShaderId> {
        Err(Error::Unsupported)
    }

    fn destroy_shader(&mut self, _id: ShaderId) {}

    fn create_pipeline(&mut self, desc: PipelineDesc) -> Result<PipelineId> {
        let raw = Self::alloc_slot(&mut self.pipelines, desc);
        Ok(PipelineId::from_raw(raw))
    }

    fn destroy_pipeline(&mut self, id: PipelineId) {
        let Some(slot) = Self::id_to_slot(id.raw()) else {
            return;
        };
        if let Some(entry) = self.pipelines.get_mut(slot) {
            *entry = None;
        }
    }

    fn create_image(&mut self, desc: ImageDesc) -> Result<ImageId> {
        let bytes = (desc.width as usize)
            .saturating_mul(desc.height as usize)
            .saturating_mul(4);
        let data = vec![0u8; bytes];
        let raw = Self::alloc_slot(
            &mut self.images,
            SwImage {
                width: desc.width,
                height: desc.height,
                format: desc.format,
                data,
            },
        );
        Ok(ImageId::from_raw(raw))
    }

    fn destroy_image(&mut self, id: ImageId) {
        let Some(slot) = Self::id_to_slot(id.raw()) else {
            return;
        };
        if let Some(entry) = self.images.get_mut(slot) {
            *entry = None;
        }
    }

    fn write_image(&mut self, id: ImageId, data: &[u8]) -> Result<()> {
        let slot = Self::id_to_slot(id.raw()).ok_or(Error::NotFound)?;
        let Some(img) = self.images.get_mut(slot).and_then(|x| x.as_mut()) else {
            return Err(Error::NotFound);
        };

        let expected = (img.width as usize)
            .saturating_mul(img.height as usize)
            .saturating_mul(4);
        if data.len() < expected {
            return Err(Error::Invalid);
        }

        if img.format != ImageFormat::Rgba8888 && img.format != ImageFormat::Rgbx8888 {
            return Err(Error::Unsupported);
        }

        img.data[..expected].copy_from_slice(&data[..expected]);
        Ok(())
    }

    fn write_buffer(&mut self, id: BufferId, offset: u64, data: &[u8]) -> Result<()> {
        let offset = usize::try_from(offset).map_err(|_| Error::Invalid)?;
        let Some(buf) = self.buffer_mut(id) else {
            return Err(Error::NotFound);
        };
        if offset.saturating_add(data.len()) > buf.data.len() {
            return Err(Error::Invalid);
        }
        buf.data[offset..offset + data.len()].copy_from_slice(data);
        Ok(())
    }

    fn map_buffer(&mut self, id: BufferId, mode: MapMode) -> Result<MappedRange> {
        let _ = mode;
        let Some(buf) = self.buffer_mut(id) else {
            return Err(Error::NotFound);
        };
        Ok(MappedRange {
            ptr: buf.data.as_mut_ptr(),
            len: buf.data.len(),
        })
    }

    fn unmap_buffer(&mut self, _id: BufferId) -> Result<()> {
        Ok(())
    }

    fn submit(&mut self, cmds: CommandBuffer<'_>) -> Result<FenceId> {
        let Some(fb) = self.current_fb() else {
            return Err(Error::Unsupported);
        };

        for cmd in cmds.commands {
            match *cmd {
                Command::ClearColor { rgb } => self.fill_fb(&fb, rgb),
                Command::ClearRect {
                    rgb,
                    x,
                    y,
                    width,
                    height,
                } => self.clear_rect(&fb, rgb, x, y, width, height),
                Command::BindPipeline(id) => {
                    if self.pipeline_ref(id).is_none() {
                        return Err(Error::NotFound);
                    }
                    self.state.pipeline = Some(id);
                }
                Command::BindVertexBuffer { buffer, offset } => {
                    if self.buffer_ref(buffer).is_none() {
                        return Err(Error::NotFound);
                    }
                    let off = usize::try_from(offset).map_err(|_| Error::Invalid)?;
                    self.state.vertex_buffer = Some((buffer, off));
                }
                Command::BindImage(id) => {
                    if self.image_ref(id).is_none() {
                        return Err(Error::NotFound);
                    }
                    self.state.image = Some(id);
                }
                Command::SetSampler(s) => self.state.sampler = s,
                Command::SetBlend(b) => self.state.blend = b,
                Command::SetViewport(vp) => self.state.viewport = Some(vp),
                Command::Draw {
                    vertex_count,
                    first_vertex,
                } => {
                    self.draw(&fb, vertex_count, first_vertex)?;
                }
                Command::Present => {
                    // Immediate-mode software renderer writes directly to scanout.
                }
            }
        }

        let id = FenceId::from_raw(self.fence_seq);
        self.fence_seq = self.fence_seq.wrapping_add(1).max(1);
        Ok(id)
    }

    fn poll(&mut self, fence: FenceId) -> bool {
        fence.is_valid()
    }

    fn device_idle(&mut self) {}
}

impl GfxPresent for IntelGfxBackend {
    fn configure_swapchain(&mut self, desc: SwapchainDesc) -> Result<()> {
        self.swapchain = desc;
        Ok(())
    }

    fn swapchain_desc(&self) -> SwapchainDesc {
        self.swapchain
    }

    fn hw_cursor_supported(&mut self) -> bool {
        self.cursor.is_some()
    }

    fn hw_cursor_define_bgra(
        &mut self,
        width: u32,
        height: u32,
        hot_x: u32,
        hot_y: u32,
        pixels_bgra: &[u8],
    ) -> Result<()> {
        let Some(cur) = self.cursor.as_mut() else {
            return Err(Error::Unsupported);
        };
        if width == 0 || height == 0 {
            return Err(Error::Invalid);
        }
        let src_needed = (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(4);
        if pixels_bgra.len() < src_needed {
            return Err(Error::Invalid);
        }

        if cur.mmio_len <= CURPOS_A + 4 {
            return Err(Error::Unsupported);
        }

        unsafe { core::ptr::write_bytes(cur.virt, 0, CURSOR_BYTES) };

        let copy_w = (width as usize).min(CURSOR_W);
        let copy_h = (height as usize).min(CURSOR_H);
        for row in 0..copy_h {
            let src_off = row.saturating_mul(width as usize).saturating_mul(4);
            let dst_off = row.saturating_mul(CURSOR_W).saturating_mul(4);
            let bytes = copy_w.saturating_mul(4);
            unsafe {
                core::ptr::copy_nonoverlapping(
                    pixels_bgra.as_ptr().add(src_off),
                    cur.virt.add(dst_off),
                    bytes,
                );
            }
        }

        cur.hot_x = hot_x.min((CURSOR_W - 1) as u32);
        cur.hot_y = hot_y.min((CURSOR_H - 1) as u32);
        cur.defined = true;

        Self::program_cursor(cur);
        Ok(())
    }

    fn hw_cursor_move(&mut self, x: i32, y: i32) -> Result<()> {
        let Some(cur) = self.cursor.as_ref() else {
            return Err(Error::Unsupported);
        };
        if !cur.defined {
            return Err(Error::Invalid);
        }
        Self::move_cursor(cur, x, y);
        Ok(())
    }
}

fn first_fb_dimensions(
    framebuffers: Option<&'static ::limine::response::FramebufferResponse>,
) -> Option<(u32, u32)> {
    let fb = framebuffers
        .and_then(|r| r.framebuffers().next())
        .or_else(|| crate::limine::framebuffer_response().and_then(|r| r.framebuffers().next()))?;
    Some((fb.width() as u32, fb.height() as u32))
}
