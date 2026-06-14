use alloc::collections::LinkedList;

#[derive(Debug)]
pub(crate) struct Ui3RgbaSurface {
    pub(crate) gpu: u64,
    pub(crate) phys: u64,
    pub(crate) virt: *mut u8,
    pub(crate) bytes: usize,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pitch_bytes: u32,
    pages: LinkedList<Ui3RgbaPage>,
}

#[allow(dead_code)]
#[derive(Debug)]
pub(crate) struct Ui3RgbaPage {
    pub(crate) y0: u32,
    pub(crate) height: u32,
    pub(crate) phys: u64,
    pub(crate) virt: *mut u8,
    pub(crate) bytes: usize,
    pub(crate) gpu: u64,
}

impl Ui3RgbaSurface {
    #[allow(dead_code)]
    pub(crate) fn alloc(width: u32, height: u32, gpu: u64) -> Option<Self> {
        if width == 0 || height == 0 {
            return None;
        }
        let min_pitch = width.checked_mul(core::mem::size_of::<u32>() as u32)?;
        let pitch_bytes = align_up(min_pitch as usize, crate::intel::WARM_ALIGN)? as u32;
        let mut surface = Self {
            gpu,
            phys: 0,
            virt: core::ptr::null_mut(),
            bytes: 0,
            width,
            height: 0,
            pitch_bytes,
            pages: LinkedList::new(),
        };
        if surface.append_tail_page(height) {
            Some(surface)
        } else {
            None
        }
    }

    #[allow(dead_code)]
    pub(crate) fn as_gpgpu(&self) -> crate::intel::gpgpu::GpgpuRgba8Surface {
        crate::intel::gpgpu::GpgpuRgba8Surface {
            phys: self.phys,
            gpu: self.gpu,
            bytes: self.bytes,
            width: self.width,
            height: self.height,
            pitch_bytes: self.pitch_bytes,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn clear_white(&self) {
        for page in &self.pages {
            page.clear_white();
        }
    }

    pub(crate) fn clear_white_range(&self, y0: u32, height: u32) {
        if height == 0 {
            return;
        }
        let y1 = y0.saturating_add(height).min(self.height);
        if y0 >= y1 {
            return;
        }
        for page in &self.pages {
            page.clear_white_doc_range(y0, y1);
        }
    }

    pub(crate) fn flush_for_display(&self) {
        for page in &self.pages {
            page.flush_for_display();
        }
    }

    #[allow(dead_code)]
    pub(crate) fn ensure_height(&mut self, new_height: u32) -> bool {
        if new_height <= self.height {
            return true;
        }
        self.append_tail_page(new_height.saturating_sub(self.height))
    }

    #[allow(dead_code)]
    pub(crate) fn dealloc(self) {
        drop(self);
    }

    pub(crate) fn pages(&self) -> impl Iterator<Item = &Ui3RgbaPage> {
        self.pages.iter()
    }

    pub(crate) fn bind_primary_scanout(
        &self,
        scroll_y: u32,
        viewport_width: u32,
        viewport_height: u32,
        reason: &str,
    ) -> bool {
        self.bind_primary_scanout_inner(scroll_y, viewport_width, viewport_height, true, reason)
    }

    pub(crate) fn bind_primary_scanout_without_flush(
        &self,
        scroll_y: u32,
        viewport_width: u32,
        viewport_height: u32,
        reason: &str,
    ) -> bool {
        self.bind_primary_scanout_inner(scroll_y, viewport_width, viewport_height, false, reason)
    }

    fn bind_primary_scanout_inner(
        &self,
        scroll_y: u32,
        viewport_width: u32,
        viewport_height: u32,
        flush: bool,
        reason: &str,
    ) -> bool {
        if self.width == 0 || self.height == 0 || viewport_width == 0 || viewport_height == 0 {
            return false;
        }
        let src_y = scroll_y.min(self.height.saturating_sub(1));
        let dst_w = viewport_width.min(self.width);
        let dst_h = viewport_height.min(self.height.saturating_sub(src_y));
        if dst_w == 0 || dst_h == 0 {
            return false;
        }
        if flush {
            self.flush_for_display();
        }
        crate::intel::set_primary_plane_source_mapped(
            crate::intel::PrimaryPlaneSource {
                phys: self.phys,
                gpu: self.gpu,
                byte_len: self.bytes,
                width: self.width,
                height: self.height,
                pitch_bytes: self.pitch_bytes,
                format: crate::intel::PrimaryPlaneSourceFormat::Xrgb8888,
                src_x: 0,
                src_y,
                dst_x: 0,
                dst_y: 0,
                dst_w,
                dst_h,
            },
            reason,
        )
    }

    fn append_tail_page(&mut self, height: u32) -> bool {
        if height == 0 {
            return true;
        }
        let y0 = self.height;
        let Some(page) = Ui3RgbaPage::alloc(self.gpu, y0, height, self.pitch_bytes) else {
            return false;
        };
        if self.pages.is_empty() {
            self.phys = page.phys;
            self.virt = page.virt;
        }
        self.bytes = self.bytes.saturating_add(page.bytes);
        self.height = self.height.saturating_add(page.height);
        self.pages.push_back(page);
        true
    }
}

impl Ui3RgbaPage {
    pub(crate) fn as_gpgpu(
        &self,
        width: u32,
        pitch_bytes: u32,
    ) -> crate::intel::gpgpu::GpgpuRgba8Surface {
        crate::intel::gpgpu::GpgpuRgba8Surface {
            phys: self.phys,
            gpu: self.gpu,
            bytes: self.bytes,
            width,
            height: self.height,
            pitch_bytes,
        }
    }

    fn alloc(gpu_base: u64, y0: u32, height: u32, pitch_bytes: u32) -> Option<Self> {
        if height == 0 || pitch_bytes == 0 {
            return None;
        }
        let byte_offset = u64::from(y0).checked_mul(u64::from(pitch_bytes))?;
        let gpu = gpu_base.checked_add(byte_offset)?;
        if (gpu as usize) & (crate::intel::WARM_ALIGN - 1) != 0 {
            return None;
        }
        let bytes = (pitch_bytes as usize).checked_mul(height as usize)?;
        let (phys, virt) = crate::dma::alloc(bytes, crate::intel::WARM_ALIGN)?;
        let page = Self {
            y0,
            height,
            phys,
            virt,
            bytes,
            gpu,
        };
        if page.map_ggtt() {
            page.clear_white();
            Some(page)
        } else {
            None
        }
    }

    fn clear_white(&self) {
        if self.virt.is_null() || self.bytes == 0 {
            return;
        }
        unsafe {
            core::ptr::write_bytes(self.virt, 0xFF, self.bytes);
        }
        self.flush_for_display();
    }

    fn clear_white_doc_range(&self, y0: u32, y1: u32) {
        if self.virt.is_null() || self.bytes == 0 || y0 >= y1 {
            return;
        }
        let page_y0 = self.y0;
        let page_y1 = self.y0.saturating_add(self.height);
        let clear_y0 = y0.max(page_y0);
        let clear_y1 = y1.min(page_y1);
        if clear_y0 >= clear_y1 {
            return;
        }
        let row0 = clear_y0.saturating_sub(page_y0) as usize;
        let rows = clear_y1.saturating_sub(clear_y0) as usize;
        let pitch = self
            .bytes
            .checked_div(self.height.max(1) as usize)
            .unwrap_or(0);
        if pitch == 0 {
            return;
        }
        let offset = row0.saturating_mul(pitch);
        let bytes = rows
            .saturating_mul(pitch)
            .min(self.bytes.saturating_sub(offset));
        if bytes == 0 {
            return;
        }
        unsafe {
            core::ptr::write_bytes(self.virt.add(offset), 0xFF, bytes);
        }
        crate::intel::dma_cache_flush_range(unsafe { self.virt.add(offset) } as *const u8, bytes);
    }

    fn flush_for_display(&self) {
        if self.virt.is_null() || self.bytes == 0 {
            return;
        }
        crate::intel::dma_cache_flush_range(self.virt as *const u8, self.bytes);
    }

    fn map_ggtt(&self) -> bool {
        let Some(dev) = crate::intel::claimed_device() else {
            return false;
        };
        if !crate::intel::map_ggtt(dev, self.phys, self.bytes, self.gpu) {
            return false;
        }
        crate::intel::ggtt_invalidate(dev);
        true
    }
}

impl Drop for Ui3RgbaPage {
    fn drop(&mut self) {
        if !self.virt.is_null() && self.bytes != 0 {
            crate::dma::dealloc(self.virt, self.bytes);
            self.virt = core::ptr::null_mut();
            self.bytes = 0;
        }
    }
}

fn align_up(value: usize, align: usize) -> Option<usize> {
    if align == 0 || !align.is_power_of_two() {
        return None;
    }
    value.checked_add(align - 1).map(|v| v & !(align - 1))
}
