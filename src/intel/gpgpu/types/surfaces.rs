#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct GpgpuPoint {
    pub(crate) x: i32,
    pub(crate) y: i32,
}

impl GpgpuPoint {
    pub(crate) const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct GpgpuRect {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl GpgpuRect {
    pub(crate) const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub(crate) const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) enum GpgpuRgba8StorageOrder {
    /// Bytes in increasing memory order are R, G, B, A.
    #[default]
    Rgba,
    /// Bytes in increasing memory order are B, G, R, A while shader-facing
    /// channels remain logical RGBA. Intel ARGB cursor scanout needs this.
    Bgra,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuRgba8Surface {
    pub(crate) phys: u64,
    pub(crate) gpu: u64,
    pub(crate) bytes: usize,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pitch_bytes: u32,
    pub(crate) storage_order: GpgpuRgba8StorageOrder,
}

impl GpgpuRgba8Surface {
    pub(crate) fn new(
        phys: u64,
        gpu: u64,
        bytes: usize,
        width: u32,
        height: u32,
        pitch_bytes: u32,
    ) -> Option<Self> {
        let surface = Self {
            phys,
            gpu,
            bytes,
            width,
            height,
            pitch_bytes,
            storage_order: GpgpuRgba8StorageOrder::Rgba,
        };
        if surface.is_valid() {
            Some(surface)
        } else {
            None
        }
    }

    pub(crate) fn new_bgra(
        phys: u64,
        gpu: u64,
        bytes: usize,
        width: u32,
        height: u32,
        pitch_bytes: u32,
    ) -> Option<Self> {
        let mut surface = Self::new(phys, gpu, bytes, width, height, pitch_bytes)?;
        surface.storage_order = GpgpuRgba8StorageOrder::Bgra;
        Some(surface)
    }

    pub(crate) fn is_valid(self) -> bool {
        if self.width == 0 || self.height == 0 {
            return false;
        }
        if (self.phys & 0xFFF) != 0 {
            return false;
        }
        let min_pitch = self
            .width
            .saturating_mul(core::mem::size_of::<u32>() as u32);
        if self.pitch_bytes < min_pitch {
            return false;
        }
        let Some(last_row) = (self.height as usize)
            .checked_sub(1)
            .and_then(|row| row.checked_mul(self.pitch_bytes as usize))
        else {
            return false;
        };
        let Some(min_bytes) = last_row.checked_add(min_pitch as usize) else {
            return false;
        };
        min_bytes <= self.bytes
    }

    pub(crate) const fn bounds(self) -> GpgpuRect {
        GpgpuRect::new(0, 0, self.width, self.height)
    }
}

/// Persistent linear RGBA8 storage owned by one GPU-side producer.
///
/// Gridpaper uses this for its immutable page base: geometry and static glyph
/// instances are rendered once through ordinary PAT0/WB mappings, then the C++
/// copy kernel reads it through the same cache policy to seed each PAT3/UC
/// scanout buffer before animated font instances are composited.
pub(crate) struct GpgpuOwnedRgba8Surface {
    surface: GpgpuRgba8Surface,
    virt: *mut u8,
}

unsafe impl Send for GpgpuOwnedRgba8Surface {}
unsafe impl Sync for GpgpuOwnedRgba8Surface {}

impl GpgpuOwnedRgba8Surface {
    pub(crate) const fn surface(&self) -> GpgpuRgba8Surface {
        self.surface
    }

    /// Copy a completed linear RGBA surface into a tightly packed CPU buffer.
    ///
    /// GridPaper printing is the cold consumer for this path. Live UI4 frames
    /// remain GPU-owned and never read back; a print raster is read once only
    /// after the same compute renderer has produced its final release proof.
    pub(crate) fn readback_tight_rgba(&self) -> Option<Vec<u8>> {
        if !self.surface.is_valid() || self.virt.is_null() {
            return None;
        }
        let row_bytes = (self.surface.width as usize).checked_mul(core::mem::size_of::<u32>())?;
        let pitch_bytes = self.surface.pitch_bytes as usize;
        if pitch_bytes < row_bytes {
            return None;
        }
        let output_bytes = row_bytes.checked_mul(self.surface.height as usize)?;
        super::dma_flush(self.virt, self.surface.bytes);
        let mut rgba = Vec::with_capacity(output_bytes);
        for row in 0..self.surface.height as usize {
            let offset = row.checked_mul(pitch_bytes)?;
            let end = offset.checked_add(row_bytes)?;
            if end > self.surface.bytes {
                return None;
            }
            let source = unsafe { core::slice::from_raw_parts(self.virt.add(offset), row_bytes) };
            rgba.extend_from_slice(source);
        }
        Some(rgba)
    }
}

impl Drop for GpgpuOwnedRgba8Surface {
    fn drop(&mut self) {
        if !retire_font_rcs_ppgtt_range(self.surface.gpu, self.surface.phys, self.surface.bytes) {
            crate::log_error!(
                target: "gpgpu";
                "intel/gpgpu: persistent RGBA8 backing retirement refused phys=0x{:X} gpu=0x{:X} bytes={} action=no-unmap-no-free\n",
                self.surface.phys,
                self.surface.gpu,
                self.surface.bytes,
            );
            return;
        }
        crate::dma::dealloc(self.virt, self.surface.bytes);
        recycle_font_coverage_gpu_va(self.surface.gpu, self.surface.bytes);
    }
}

/// Decoder-owned Xe media Tile64 NV12 storage mapped read-only by convention into the
/// compositor's private PPGTT.  The media engine's VA is only an opaque alias;
/// direct RCS installs its own PTEs for the same physical picture.
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuNv12Tile64Surface {
    pub(crate) phys: u64,
    pub(crate) gpu: u64,
    pub(crate) bytes: usize,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pitch_bytes: u32,
    pub(crate) uv_offset: u32,
}

impl GpgpuNv12Tile64Surface {
    pub(crate) fn new(
        phys: u64,
        gpu: u64,
        bytes: usize,
        width: u32,
        height: u32,
        pitch_bytes: u32,
        uv_offset: u32,
    ) -> Option<Self> {
        let surface = Self {
            phys,
            gpu,
            bytes,
            width,
            height,
            pitch_bytes,
            uv_offset,
        };
        surface.is_valid().then_some(surface)
    }

    pub(crate) fn is_valid(self) -> bool {
        if self.phys == 0
            || self.gpu == 0
            || !self.phys.is_multiple_of(4096)
            || !self.gpu.is_multiple_of(4096)
            || self.width == 0
            || self.height == 0
            || self.pitch_bytes < self.width
            || !self.pitch_bytes.is_multiple_of(256)
            || self.uv_offset == 0
            || !self.uv_offset.is_multiple_of(self.pitch_bytes)
        {
            return false;
        }
        let chroma_row = self.uv_offset / self.pitch_bytes;
        if !chroma_row.is_multiple_of(256) {
            return false;
        }
        let Some(total_rows) = chroma_row
            .checked_add(self.height.div_ceil(2))
            .map(|rows| rows.next_multiple_of(256))
        else {
            return false;
        };
        let Some(required) = u64::from(total_rows).checked_mul(u64::from(self.pitch_bytes)) else {
            return false;
        };
        required <= self.bytes as u64
    }
}

/// Linear NV12 storage shared by the UI4 RCS converter and VDEnc input path.
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuNv12LinearSurface {
    pub(crate) phys: u64,
    pub(crate) gpu: u64,
    pub(crate) bytes: usize,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pitch_bytes: u32,
    pub(crate) uv_offset: u32,
}

impl GpgpuNv12LinearSurface {
    pub(crate) fn new(
        phys: u64,
        gpu: u64,
        bytes: usize,
        width: u32,
        height: u32,
        pitch_bytes: u32,
    ) -> Option<Self> {
        let uv_offset = pitch_bytes.checked_mul(height)?;
        let surface = Self {
            phys,
            gpu,
            bytes,
            width,
            height,
            pitch_bytes,
            uv_offset,
        };
        surface.is_valid().then_some(surface)
    }

    pub(crate) fn is_valid(self) -> bool {
        if self.phys == 0
            || self.gpu == 0
            || !self.phys.is_multiple_of(4096)
            || !self.gpu.is_multiple_of(4096)
            || self.width == 0
            || self.height == 0
            || !self.width.is_multiple_of(2)
            || !self.height.is_multiple_of(2)
            || self.pitch_bytes < self.width
        {
            return false;
        }
        let Some(required) = u64::from(self.pitch_bytes)
            .checked_mul(u64::from(self.height))
            .and_then(|luma| luma.checked_add(luma / 2))
        else {
            return false;
        };
        self.uv_offset == self.pitch_bytes.saturating_mul(self.height)
            && required <= self.bytes as u64
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuRgb565Surface {
    pub(crate) phys: u64,
    pub(crate) gpu: u64,
    pub(crate) bytes: usize,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pitch_bytes: u32,
}

impl GpgpuRgb565Surface {
    pub(crate) fn new(
        phys: u64,
        gpu: u64,
        bytes: usize,
        width: u32,
        height: u32,
        pitch_bytes: u32,
    ) -> Option<Self> {
        let surface = Self {
            phys,
            gpu,
            bytes,
            width,
            height,
            pitch_bytes,
        };
        if surface.is_valid() {
            Some(surface)
        } else {
            None
        }
    }

    pub(crate) fn is_valid(self) -> bool {
        if self.width == 0 || self.height == 0 {
            return false;
        }
        if (self.phys & 0xFFF) != 0 {
            return false;
        }
        let min_pitch = self
            .width
            .saturating_mul(core::mem::size_of::<u16>() as u32);
        if self.pitch_bytes < min_pitch {
            return false;
        }
        let Some(last_row) = (self.height as usize)
            .checked_sub(1)
            .and_then(|row| row.checked_mul(self.pitch_bytes as usize))
        else {
            return false;
        };
        let Some(min_bytes) = last_row.checked_add(min_pitch as usize) else {
            return false;
        };
        min_bytes <= self.bytes
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuMask8Surface {
    pub(crate) phys: u64,
    pub(crate) gpu: u64,
    pub(crate) bytes: usize,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pitch_bytes: u32,
}

pub(crate) struct GpgpuOwnedMask8Surface {
    surface: GpgpuMask8Surface,
    virt: *mut u8,
    /// Once an accepted GPU batch loses its retirement proof, neither the
    /// backing pages nor their virtual range may be recycled.  Keeping this
    /// bit with the owner makes that rule hold even when a caller forgets to
    /// deliberately leak the wrapper on an error path.
    quarantined: AtomicBool,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct GpgpuMask8Audit {
    pub(crate) nonzero_pixels: usize,
    pub(crate) bounds: GpgpuRect,
}

unsafe impl Send for GpgpuOwnedMask8Surface {}
unsafe impl Sync for GpgpuOwnedMask8Surface {}

impl GpgpuOwnedMask8Surface {
    pub(crate) const fn surface(&self) -> GpgpuMask8Surface {
        self.surface
    }

    fn quarantine_backing(&self) {
        self.quarantined.store(true, Ordering::Release);
    }

    fn backing_is_quarantined(&self) -> bool {
        self.quarantined.load(Ordering::Acquire)
    }

    /// Zero one bounded region before it is handed to a new GPU producer.
    /// Callers must first prove that no submitted batch can still reference
    /// the region; atlas tile leases provide that proof for cache reuse.
    fn clear_rect_cpu(&self, rect: GpgpuRect) -> bool {
        if self.backing_is_quarantined()
            || self.virt.is_null()
            || rect.x < 0
            || rect.y < 0
            || rect.width == 0
            || rect.height == 0
            || (rect.x as u32)
                .checked_add(rect.width)
                .is_none_or(|right| right > self.surface.width)
            || (rect.y as u32)
                .checked_add(rect.height)
                .is_none_or(|bottom| bottom > self.surface.height)
        {
            return false;
        }
        let Some(row_offset) = (rect.y as usize)
            .checked_mul(self.surface.pitch_bytes as usize)
            .and_then(|offset| offset.checked_add(rect.x as usize))
        else {
            return false;
        };
        let Some(last_row_offset) = (rect.height as usize)
            .checked_sub(1)
            .and_then(|row| row.checked_mul(self.surface.pitch_bytes as usize))
            .and_then(|offset| row_offset.checked_add(offset))
        else {
            return false;
        };
        if last_row_offset
            .checked_add(rect.width as usize)
            .is_none_or(|end| end > self.surface.bytes)
        {
            return false;
        }
        for row in 0..rect.height as usize {
            let offset = row_offset + row * self.surface.pitch_bytes as usize;
            unsafe {
                core::ptr::write_bytes(self.virt.add(offset), 0, rect.width as usize);
            }
        }
        super::dma_flush_strided_rows(
            unsafe { self.virt.add(row_offset) },
            rect.width as usize,
            self.surface.pitch_bytes as usize,
            rect.height as usize,
        )
    }

    /// Read back the persistent mask once after generation.  This is a cold
    /// path integrity check, not part of frame composition.
    pub(crate) fn nonzero_audit(&self) -> Option<GpgpuMask8Audit> {
        self.nonzero_audit_rect(self.surface.bounds())
    }

    /// Audit exactly one retired atlas tile. Returned bounds are tile-local,
    /// so the result stays valid when the immutable tile is stamped elsewhere.
    fn nonzero_audit_rect(&self, rect: GpgpuRect) -> Option<GpgpuMask8Audit> {
        if !self.surface.is_valid()
            || self.virt.is_null()
            || rect.x < 0
            || rect.y < 0
            || rect.width == 0
            || rect.height == 0
            || (rect.x as u32)
                .checked_add(rect.width)
                .is_none_or(|right| right > self.surface.width)
            || (rect.y as u32)
                .checked_add(rect.height)
                .is_none_or(|bottom| bottom > self.surface.height)
        {
            return None;
        }
        let row_offset = (rect.y as usize)
            .checked_mul(self.surface.pitch_bytes as usize)?
            .checked_add(rect.x as usize)?;
        if !super::dma_flush_strided_rows(
            unsafe { self.virt.add(row_offset) },
            rect.width as usize,
            self.surface.pitch_bytes as usize,
            rect.height as usize,
        ) {
            return None;
        }
        let mut min_x = rect.width;
        let mut min_y = rect.height;
        let mut max_x = 0u32;
        let mut max_y = 0u32;
        let mut nonzero_pixels = 0usize;
        for y in 0..rect.height {
            let row_offset = ((rect.y as u32 + y) as usize)
                .checked_mul(self.surface.pitch_bytes as usize)?
                .checked_add(rect.x as usize)?;
            for x in 0..rect.width {
                let offset = row_offset.checked_add(x as usize)?;
                let coverage = unsafe { core::ptr::read_volatile(self.virt.add(offset)) };
                if coverage == 0 {
                    continue;
                }
                nonzero_pixels = nonzero_pixels.saturating_add(1);
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
        (nonzero_pixels != 0).then_some(GpgpuMask8Audit {
            nonzero_pixels,
            bounds: GpgpuRect::new(
                min_x as i32,
                min_y as i32,
                max_x.saturating_sub(min_x).saturating_add(1),
                max_y.saturating_sub(min_y).saturating_add(1),
            ),
        })
    }
}

impl Drop for GpgpuOwnedMask8Surface {
    fn drop(&mut self) {
        if self.backing_is_quarantined() {
            crate::log_error!(
                target: "gpgpu";
                "intel/gpgpu: persistent R8 backing quarantined phys=0x{:X} gpu=0x{:X} bytes={} action=no-unmap-no-free\n",
                self.surface.phys,
                self.surface.gpu,
                self.surface.bytes,
            );
            return;
        }
        if !retire_font_rcs_ppgtt_range(self.surface.gpu, self.surface.phys, self.surface.bytes) {
            self.quarantine_backing();
            crate::log_error!(
                target: "gpgpu";
                "intel/gpgpu: persistent R8 backing retirement refused phys=0x{:X} gpu=0x{:X} bytes={} action=no-unmap-no-free\n",
                self.surface.phys,
                self.surface.gpu,
                self.surface.bytes,
            );
            return;
        }
        crate::dma::dealloc(self.virt, self.surface.bytes);
        recycle_font_coverage_gpu_va(self.surface.gpu, self.surface.bytes);
    }
}

/// Buffer-surface base addresses are consumed as RAW stateless bindings. Keep
/// every atlas tile and resident outline recipe naturally aligned without
/// paying one 4 KiB DMA allocation per cached glyph.
const GPGPU_FONT_CACHE_SUBALLOCATION_ALIGN: usize = 64;
pub(crate) const GPGPU_FONT_CACHE_MAX_ENTRIES: usize = 4096;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct GpgpuFontCacheEntryKey {
    slot: u32,
    generation: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum GpgpuFontCacheEntryState {
    Free,
    Reserved,
    Ready,
    Quarantined,
}

#[derive(Copy, Clone, Debug)]
struct GpgpuMask8AtlasEntry {
    generation: u32,
    allocation_rect: GpgpuRect,
    content_rect: GpgpuRect,
    state: GpgpuFontCacheEntryState,
    handles: usize,
}

impl Default for GpgpuMask8AtlasEntry {
    fn default() -> Self {
        Self {
            generation: 0,
            allocation_rect: GpgpuRect::default(),
            content_rect: GpgpuRect::default(),
            state: GpgpuFontCacheEntryState::Free,
            handles: 0,
        }
    }
}

#[derive(Clone, Debug)]
struct GpgpuMask8AtlasAllocator {
    width: u32,
    height: u32,
    max_entries: usize,
    entries: Vec<GpgpuMask8AtlasEntry>,
    free_rects: Vec<GpgpuRect>,
}

impl GpgpuMask8AtlasAllocator {
    fn new(width: u32, height: u32, max_entries: usize) -> Option<Self> {
        if width == 0
            || height == 0
            || max_entries == 0
            || max_entries > GPGPU_FONT_CACHE_MAX_ENTRIES
        {
            return None;
        }
        Some(Self {
            width,
            height,
            max_entries,
            entries: Vec::new(),
            free_rects: {
                let mut free_rects = Vec::with_capacity(1);
                free_rects.push(GpgpuRect::new(0, 0, width, height));
                free_rects
            },
        })
    }

    fn aligned_tile_width(width: u32) -> Option<u32> {
        let alignment = GPGPU_FONT_CACHE_SUBALLOCATION_ALIGN as u32;
        width
            .checked_add(alignment - 1)
            .map(|value| value & !(alignment - 1))
    }

    fn reserve(
        &mut self,
        width: u32,
        height: u32,
    ) -> Option<(GpgpuFontCacheEntryKey, GpgpuRect, GpgpuRect)> {
        let allocation_width = Self::aligned_tile_width(width)?;
        if width == 0 || height == 0 || allocation_width > self.width || height > self.height {
            return None;
        }
        let reusable_slot = self
            .entries
            .iter()
            .position(|entry| entry.state == GpgpuFontCacheEntryState::Free);
        if reusable_slot.is_none() && self.entries.len() >= self.max_entries {
            return None;
        }

        let best = self
            .free_rects
            .iter()
            .enumerate()
            .filter(|(_, rect)| rect.width >= allocation_width && rect.height >= height)
            .min_by_key(|(_, rect)| {
                let waste = u64::from(rect.width) * u64::from(rect.height)
                    - u64::from(allocation_width) * u64::from(height);
                let short_side = (rect.width - allocation_width).min(rect.height - height);
                (waste, short_side)
            })
            .map(|(index, _)| index)?;
        let free = self.free_rects.swap_remove(best);
        let allocation_rect = GpgpuRect::new(free.x, free.y, allocation_width, height);
        let content_rect = GpgpuRect::new(free.x, free.y, width, height);

        let remaining_width = free.width - allocation_width;
        let remaining_height = free.height - height;
        if remaining_width > remaining_height {
            if remaining_width != 0 {
                self.free_rects.push(GpgpuRect::new(
                    free.x + allocation_width as i32,
                    free.y,
                    remaining_width,
                    free.height,
                ));
            }
            if remaining_height != 0 {
                self.free_rects.push(GpgpuRect::new(
                    free.x,
                    free.y + height as i32,
                    allocation_width,
                    remaining_height,
                ));
            }
        } else {
            if remaining_width != 0 {
                self.free_rects.push(GpgpuRect::new(
                    free.x + allocation_width as i32,
                    free.y,
                    remaining_width,
                    height,
                ));
            }
            if remaining_height != 0 {
                self.free_rects.push(GpgpuRect::new(
                    free.x,
                    free.y + height as i32,
                    free.width,
                    remaining_height,
                ));
            }
        }

        let slot = reusable_slot.unwrap_or(self.entries.len());
        if slot == self.entries.len() {
            self.entries.push(GpgpuMask8AtlasEntry::default());
        }
        let entry = &mut self.entries[slot];
        entry.generation = entry.generation.wrapping_add(1).max(1);
        entry.allocation_rect = allocation_rect;
        entry.content_rect = content_rect;
        entry.state = GpgpuFontCacheEntryState::Reserved;
        entry.handles = 0;
        Some((
            GpgpuFontCacheEntryKey {
                slot: slot as u32,
                generation: entry.generation,
            },
            allocation_rect,
            content_rect,
        ))
    }

    fn entry_mut(&mut self, key: GpgpuFontCacheEntryKey) -> Option<&mut GpgpuMask8AtlasEntry> {
        let entry = self.entries.get_mut(key.slot as usize)?;
        (entry.generation == key.generation).then_some(entry)
    }

    fn publish(&mut self, key: GpgpuFontCacheEntryKey) -> bool {
        let Some(entry) = self.entry_mut(key) else {
            return false;
        };
        if entry.state != GpgpuFontCacheEntryState::Reserved {
            return false;
        }
        entry.state = GpgpuFontCacheEntryState::Ready;
        entry.handles = 1;
        true
    }

    fn add_handle(&mut self, key: GpgpuFontCacheEntryKey) -> bool {
        let Some(entry) = self.entry_mut(key) else {
            return false;
        };
        if !matches!(
            entry.state,
            GpgpuFontCacheEntryState::Ready | GpgpuFontCacheEntryState::Quarantined
        ) {
            return false;
        }
        let Some(handles) = entry.handles.checked_add(1) else {
            return false;
        };
        entry.handles = handles;
        true
    }

    fn release_reservation(&mut self, key: GpgpuFontCacheEntryKey) {
        let allocation_rect = {
            let Some(entry) = self.entry_mut(key) else {
                return;
            };
            if entry.state != GpgpuFontCacheEntryState::Reserved {
                return;
            }
            entry.state = GpgpuFontCacheEntryState::Free;
            entry.allocation_rect
        };
        self.release_rect(allocation_rect);
    }

    fn release_handle(&mut self, key: GpgpuFontCacheEntryKey) {
        let release_rect = {
            let Some(entry) = self.entry_mut(key) else {
                return;
            };
            if !matches!(
                entry.state,
                GpgpuFontCacheEntryState::Ready | GpgpuFontCacheEntryState::Quarantined
            ) || entry.handles == 0
            {
                return;
            }
            entry.handles -= 1;
            if entry.handles == 0 && entry.state == GpgpuFontCacheEntryState::Ready {
                entry.state = GpgpuFontCacheEntryState::Free;
                Some(entry.allocation_rect)
            } else {
                None
            }
        };
        if let Some(rect) = release_rect {
            self.release_rect(rect);
        }
    }

    fn quarantine(&mut self, key: GpgpuFontCacheEntryKey) -> bool {
        let Some(entry) = self.entry_mut(key) else {
            return false;
        };
        if !matches!(
            entry.state,
            GpgpuFontCacheEntryState::Reserved | GpgpuFontCacheEntryState::Ready
        ) {
            return entry.state == GpgpuFontCacheEntryState::Quarantined;
        }
        entry.state = GpgpuFontCacheEntryState::Quarantined;
        true
    }

    fn release_rect(&mut self, rect: GpgpuRect) {
        self.free_rects.push(rect);
        'coalesce: loop {
            for left in 0..self.free_rects.len() {
                for right in left + 1..self.free_rects.len() {
                    let a = self.free_rects[left];
                    let b = self.free_rects[right];
                    let merged =
                        if a.y == b.y && a.height == b.height && a.x + a.width as i32 == b.x {
                            Some(GpgpuRect::new(a.x, a.y, a.width + b.width, a.height))
                        } else if a.y == b.y && a.height == b.height && b.x + b.width as i32 == a.x
                        {
                            Some(GpgpuRect::new(b.x, b.y, a.width + b.width, a.height))
                        } else if a.x == b.x && a.width == b.width && a.y + a.height as i32 == b.y {
                            Some(GpgpuRect::new(a.x, a.y, a.width, a.height + b.height))
                        } else if a.x == b.x && a.width == b.width && b.y + b.height as i32 == a.y {
                            Some(GpgpuRect::new(b.x, b.y, a.width, a.height + b.height))
                        } else {
                            None
                        };
                    if let Some(merged) = merged {
                        self.free_rects[left] = merged;
                        self.free_rects.swap_remove(right);
                        continue 'coalesce;
                    }
                }
            }
            break;
        }
    }

    fn stats(&self) -> GpgpuMask8AtlasStats {
        let mut stats = GpgpuMask8AtlasStats {
            capacity_width: self.width,
            capacity_height: self.height,
            max_entries: self.max_entries,
            ..GpgpuMask8AtlasStats::default()
        };
        for entry in &self.entries {
            match entry.state {
                GpgpuFontCacheEntryState::Free => {}
                GpgpuFontCacheEntryState::Reserved => stats.reserved_entries += 1,
                GpgpuFontCacheEntryState::Ready => stats.ready_entries += 1,
                GpgpuFontCacheEntryState::Quarantined => stats.quarantined_entries += 1,
            }
        }
        stats.free_rects = self.free_rects.len();
        stats
    }
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct GpgpuMask8AtlasStats {
    pub(crate) capacity_width: u32,
    pub(crate) capacity_height: u32,
    pub(crate) max_entries: usize,
    pub(crate) reserved_entries: usize,
    pub(crate) ready_entries: usize,
    pub(crate) quarantined_entries: usize,
    pub(crate) free_rects: usize,
}

struct GpgpuMask8AtlasInner {
    storage: GpgpuOwnedMask8Surface,
    allocator: Mutex<GpgpuMask8AtlasAllocator>,
}

/// One bounded R8 backing allocation shared by immutable glyph tiles.
///
/// Tile leases are generation checked and reference counted independently of
/// the backing `Arc`. Dropping a cache entry therefore reclaims its rectangle
/// only after every in-flight plan has dropped its lease. A consumer whose GPU
/// submission becomes ambiguous must call `quarantine`; that permanently pins
/// the complete backing because a late GPU write can target any byte in its
/// bound RAW surface.
#[derive(Clone)]
pub(crate) struct GpgpuOwnedMask8Atlas {
    inner: alloc::sync::Arc<GpgpuMask8AtlasInner>,
}

impl GpgpuOwnedMask8Atlas {
    fn from_storage(storage: GpgpuOwnedMask8Surface, max_entries: usize) -> Option<Self> {
        let surface = storage.surface();
        let allocator = GpgpuMask8AtlasAllocator::new(surface.width, surface.height, max_entries)?;
        Some(Self {
            inner: alloc::sync::Arc::new(GpgpuMask8AtlasInner {
                storage,
                allocator: Mutex::new(allocator),
            }),
        })
    }

    pub(crate) fn surface(&self) -> GpgpuMask8Surface {
        self.inner.storage.surface()
    }

    pub(crate) fn reserve(&self, width: u32, height: u32) -> Option<GpgpuMask8AtlasReservation> {
        if self.inner.storage.backing_is_quarantined() {
            return None;
        }
        let (key, allocation_rect, content_rect) =
            self.inner.allocator.lock().reserve(width, height)?;
        if !self.inner.storage.clear_rect_cpu(allocation_rect) {
            self.inner.allocator.lock().release_reservation(key);
            return None;
        }
        Some(GpgpuMask8AtlasReservation {
            inner: self.inner.clone(),
            key,
            allocation_rect,
            content_rect,
            active: true,
        })
    }

    pub(crate) fn stats(&self) -> GpgpuMask8AtlasStats {
        self.inner.allocator.lock().stats()
    }
}

pub(crate) struct GpgpuMask8AtlasReservation {
    inner: alloc::sync::Arc<GpgpuMask8AtlasInner>,
    key: GpgpuFontCacheEntryKey,
    allocation_rect: GpgpuRect,
    content_rect: GpgpuRect,
    active: bool,
}

#[derive(Copy, Clone, Debug)]
struct GpgpuMask8AtlasCoverageTarget {
    mapping: GpgpuMask8Surface,
    binding_gpu: u64,
    binding_bytes: usize,
    width: u32,
    height: u32,
    pitch_bytes: u32,
}

impl GpgpuMask8AtlasReservation {
    pub(crate) const fn rect(&self) -> GpgpuRect {
        self.content_rect
    }

    pub(crate) const fn allocated_rect(&self) -> GpgpuRect {
        self.allocation_rect
    }

    fn coverage_target(&self) -> Option<GpgpuMask8AtlasCoverageTarget> {
        if !self.active || self.inner.storage.backing_is_quarantined() {
            return None;
        }
        let mapping = self.inner.storage.surface();
        let byte_offset = (self.content_rect.y as usize)
            .checked_mul(mapping.pitch_bytes as usize)?
            .checked_add(self.content_rect.x as usize)?;
        let binding_gpu = mapping.gpu.checked_add(byte_offset as u64)?;
        if !binding_gpu.is_multiple_of(GPGPU_FONT_CACHE_SUBALLOCATION_ALIGN as u64) {
            return None;
        }
        let binding_bytes = (self.content_rect.height as usize)
            .checked_sub(1)?
            .checked_mul(mapping.pitch_bytes as usize)?
            .checked_add(self.content_rect.width as usize)?;
        byte_offset
            .checked_add(binding_bytes)
            .filter(|end| *end <= mapping.bytes)?;
        Some(GpgpuMask8AtlasCoverageTarget {
            mapping,
            binding_gpu,
            binding_bytes,
            width: self.content_rect.width,
            height: self.content_rect.height,
            pitch_bytes: mapping.pitch_bytes,
        })
    }

    fn nonzero_audit(&self) -> Option<GpgpuMask8Audit> {
        if !self.active {
            return None;
        }
        self.inner.storage.nonzero_audit_rect(self.content_rect)
    }

    fn publish(
        mut self,
        audit: GpgpuMask8Audit,
        coverage_audit_ms: u64,
    ) -> Option<GpgpuMask8AtlasTile> {
        if !self.active || !self.inner.allocator.lock().publish(self.key) {
            return None;
        }
        self.active = false;
        Some(GpgpuMask8AtlasTile {
            inner: self.inner.clone(),
            key: self.key,
            rect: self.content_rect,
            audit,
            coverage_audit_ms,
        })
    }

    fn quarantine(mut self) {
        if self.active {
            self.inner.storage.quarantine_backing();
            self.inner.allocator.lock().quarantine(self.key);
            self.active = false;
        }
    }
}

impl Drop for GpgpuMask8AtlasReservation {
    fn drop(&mut self) {
        if self.active {
            self.inner.allocator.lock().release_reservation(self.key);
        }
    }
}

/// Immutable lease for one glyph-local subrectangle of an R8 atlas.
pub(crate) struct GpgpuMask8AtlasTile {
    inner: alloc::sync::Arc<GpgpuMask8AtlasInner>,
    key: GpgpuFontCacheEntryKey,
    rect: GpgpuRect,
    audit: GpgpuMask8Audit,
    coverage_audit_ms: u64,
}

impl GpgpuMask8AtlasTile {
    /// Existing mask compositors map the page-aligned backing and consume this
    /// subrectangle, so they require no atlas-specific shader ABI.
    pub(crate) fn surface(&self) -> GpgpuMask8Surface {
        self.inner.storage.surface()
    }

    pub(crate) const fn rect(&self) -> GpgpuRect {
        self.rect
    }

    /// One-time post-generation audit. Bounds are relative to `rect()` and no
    /// subsequent frame needs to read this immutable coverage back to the CPU.
    pub(crate) const fn audit(&self) -> GpgpuMask8Audit {
        self.audit
    }

    pub(crate) const fn coverage_audit_ms(&self) -> u64 {
        self.coverage_audit_ms
    }

    /// Pin this tile and its complete backing after an accepted source-read or
    /// destination-write batch loses its retirement proof.
    pub(crate) fn quarantine(&self) {
        self.inner.storage.quarantine_backing();
        self.inner.allocator.lock().quarantine(self.key);
    }
}

impl Clone for GpgpuMask8AtlasTile {
    fn clone(&self) -> Self {
        assert!(
            self.inner.allocator.lock().add_handle(self.key),
            "live R8 atlas lease lost its generation"
        );
        Self {
            inner: self.inner.clone(),
            key: self.key,
            rect: self.rect,
            audit: self.audit,
            coverage_audit_ms: self.coverage_audit_ms,
        }
    }
}

impl Drop for GpgpuMask8AtlasTile {
    fn drop(&mut self) {
        self.inner.allocator.lock().release_handle(self.key);
    }
}

#[derive(Copy, Clone, Debug)]
struct GpgpuFontOutlineOpsEntry {
    generation: u32,
    offset: usize,
    allocation_bytes: usize,
    input_bytes: usize,
    op_count: u32,
    state: GpgpuFontCacheEntryState,
    handles: usize,
}

impl Default for GpgpuFontOutlineOpsEntry {
    fn default() -> Self {
        Self {
            generation: 0,
            offset: 0,
            allocation_bytes: 0,
            input_bytes: 0,
            op_count: 0,
            state: GpgpuFontCacheEntryState::Free,
            handles: 0,
        }
    }
}

#[derive(Clone, Debug)]
struct GpgpuFontOutlineOpsAllocator {
    bytes: usize,
    max_entries: usize,
    entries: Vec<GpgpuFontOutlineOpsEntry>,
    free_ranges: Vec<(usize, usize)>,
}

impl GpgpuFontOutlineOpsAllocator {
    fn new(bytes: usize, max_entries: usize) -> Option<Self> {
        if bytes == 0
            || !bytes.is_multiple_of(GPGPU_FONT_CACHE_SUBALLOCATION_ALIGN)
            || max_entries == 0
            || max_entries > GPGPU_FONT_CACHE_MAX_ENTRIES
        {
            return None;
        }
        Some(Self {
            bytes,
            max_entries,
            entries: Vec::new(),
            free_ranges: {
                let mut free_ranges = Vec::with_capacity(1);
                free_ranges.push((0, bytes));
                free_ranges
            },
        })
    }

    fn reserve(
        &mut self,
        input_bytes: usize,
        op_count: u32,
    ) -> Option<(GpgpuFontCacheEntryKey, usize, usize)> {
        let allocation_bytes = align_up(input_bytes, GPGPU_FONT_CACHE_SUBALLOCATION_ALIGN)?;
        if input_bytes == 0 || op_count == 0 || allocation_bytes > self.bytes {
            return None;
        }
        let reusable_slot = self
            .entries
            .iter()
            .position(|entry| entry.state == GpgpuFontCacheEntryState::Free);
        if reusable_slot.is_none() && self.entries.len() >= self.max_entries {
            return None;
        }
        let range_index = self
            .free_ranges
            .iter()
            .enumerate()
            .filter(|(_, (start, end))| end.saturating_sub(*start) >= allocation_bytes)
            .min_by_key(|(_, (start, end))| end - start - allocation_bytes)
            .map(|(index, _)| index)?;
        let (offset, end) = self.free_ranges[range_index];
        let next = offset.checked_add(allocation_bytes)?;
        if next == end {
            self.free_ranges.swap_remove(range_index);
        } else {
            self.free_ranges[range_index].0 = next;
        }

        let slot = reusable_slot.unwrap_or(self.entries.len());
        if slot == self.entries.len() {
            self.entries.push(GpgpuFontOutlineOpsEntry::default());
        }
        let entry = &mut self.entries[slot];
        entry.generation = entry.generation.wrapping_add(1).max(1);
        entry.offset = offset;
        entry.allocation_bytes = allocation_bytes;
        entry.input_bytes = input_bytes;
        entry.op_count = op_count;
        entry.state = GpgpuFontCacheEntryState::Reserved;
        entry.handles = 0;
        Some((
            GpgpuFontCacheEntryKey {
                slot: slot as u32,
                generation: entry.generation,
            },
            offset,
            allocation_bytes,
        ))
    }

    fn entry_mut(&mut self, key: GpgpuFontCacheEntryKey) -> Option<&mut GpgpuFontOutlineOpsEntry> {
        let entry = self.entries.get_mut(key.slot as usize)?;
        (entry.generation == key.generation).then_some(entry)
    }

    fn publish(&mut self, key: GpgpuFontCacheEntryKey) -> bool {
        let Some(entry) = self.entry_mut(key) else {
            return false;
        };
        if entry.state != GpgpuFontCacheEntryState::Reserved {
            return false;
        }
        entry.state = GpgpuFontCacheEntryState::Ready;
        entry.handles = 1;
        true
    }

    fn add_handle(&mut self, key: GpgpuFontCacheEntryKey) -> bool {
        let Some(entry) = self.entry_mut(key) else {
            return false;
        };
        if !matches!(
            entry.state,
            GpgpuFontCacheEntryState::Ready | GpgpuFontCacheEntryState::Quarantined
        ) {
            return false;
        }
        let Some(handles) = entry.handles.checked_add(1) else {
            return false;
        };
        entry.handles = handles;
        true
    }

    fn release_reservation(&mut self, key: GpgpuFontCacheEntryKey) {
        let release = {
            let Some(entry) = self.entry_mut(key) else {
                return;
            };
            if entry.state != GpgpuFontCacheEntryState::Reserved {
                return;
            }
            entry.state = GpgpuFontCacheEntryState::Free;
            Some((entry.offset, entry.offset + entry.allocation_bytes))
        };
        if let Some(range) = release {
            self.release_range(range);
        }
    }

    fn release_handle(&mut self, key: GpgpuFontCacheEntryKey) {
        let release = {
            let Some(entry) = self.entry_mut(key) else {
                return;
            };
            if !matches!(
                entry.state,
                GpgpuFontCacheEntryState::Ready | GpgpuFontCacheEntryState::Quarantined
            ) || entry.handles == 0
            {
                return;
            }
            entry.handles -= 1;
            if entry.handles == 0 && entry.state == GpgpuFontCacheEntryState::Ready {
                entry.state = GpgpuFontCacheEntryState::Free;
                Some((entry.offset, entry.offset + entry.allocation_bytes))
            } else {
                None
            }
        };
        if let Some(range) = release {
            self.release_range(range);
        }
    }

    fn quarantine(&mut self, key: GpgpuFontCacheEntryKey) -> bool {
        let Some(entry) = self.entry_mut(key) else {
            return false;
        };
        if !matches!(
            entry.state,
            GpgpuFontCacheEntryState::Reserved | GpgpuFontCacheEntryState::Ready
        ) {
            return entry.state == GpgpuFontCacheEntryState::Quarantined;
        }
        entry.state = GpgpuFontCacheEntryState::Quarantined;
        true
    }

    fn release_range(&mut self, range: (usize, usize)) {
        self.free_ranges.push(range);
        self.free_ranges.sort_unstable_by_key(|range| range.0);
        let mut write = 0usize;
        for read in 0..self.free_ranges.len() {
            let range = self.free_ranges[read];
            if write != 0 && range.0 <= self.free_ranges[write - 1].1 {
                self.free_ranges[write - 1].1 = self.free_ranges[write - 1].1.max(range.1);
            } else {
                self.free_ranges[write] = range;
                write += 1;
            }
        }
        self.free_ranges.truncate(write);
    }
}

struct GpgpuFontOutlineOpsArenaInner {
    phys: u64,
    gpu: u64,
    bytes: usize,
    virt: *mut u8,
    allocator: Mutex<GpgpuFontOutlineOpsAllocator>,
    quarantined: AtomicBool,
}

unsafe impl Send for GpgpuFontOutlineOpsArenaInner {}
unsafe impl Sync for GpgpuFontOutlineOpsArenaInner {}

impl Drop for GpgpuFontOutlineOpsArenaInner {
    fn drop(&mut self) {
        if self.quarantined.load(Ordering::Acquire) {
            crate::log_error!(
                target: "gpgpu";
                "intel/gpgpu: resident font-outline arena quarantined phys=0x{:X} gpu=0x{:X} bytes={} action=no-unmap-no-free\n",
                self.phys,
                self.gpu,
                self.bytes,
            );
            return;
        }
        if !retire_font_rcs_ppgtt_range(self.gpu, self.phys, self.bytes) {
            self.quarantined.store(true, Ordering::Release);
            crate::log_error!(
                target: "gpgpu";
                "intel/gpgpu: resident font-outline arena retirement refused phys=0x{:X} gpu=0x{:X} bytes={} action=no-unmap-no-free\n",
                self.phys,
                self.gpu,
                self.bytes,
            );
            return;
        }
        crate::dma::dealloc(self.virt, self.bytes);
        recycle_font_coverage_gpu_va(self.gpu, self.bytes);
    }
}

/// Persistent, packed, immutable outline recipes. One arena services many
/// cache entries, avoiding 4 KiB physical/virtual waste per small glyph.
#[derive(Clone)]
pub(crate) struct GpgpuOwnedFontOutlineOpsArena {
    inner: alloc::sync::Arc<GpgpuFontOutlineOpsArenaInner>,
}

impl GpgpuOwnedFontOutlineOpsArena {
    fn from_allocation(
        phys: u64,
        gpu: u64,
        bytes: usize,
        virt: *mut u8,
        max_entries: usize,
    ) -> Option<Self> {
        let allocator = GpgpuFontOutlineOpsAllocator::new(bytes, max_entries)?;
        Some(Self {
            inner: alloc::sync::Arc::new(GpgpuFontOutlineOpsArenaInner {
                phys,
                gpu,
                bytes,
                virt,
                allocator: Mutex::new(allocator),
                quarantined: AtomicBool::new(false),
            }),
        })
    }

    pub(crate) fn insert(&self, ops: &[[u32; 8]]) -> Option<GpgpuFontOutlineOps> {
        if ops.is_empty()
            || ops.len() > u32::MAX as usize
            || self.inner.quarantined.load(Ordering::Acquire)
        {
            return None;
        }
        let input_bytes = ops.len().checked_mul(core::mem::size_of::<[u32; 8]>())?;
        let (key, offset, allocation_bytes) = self
            .inner
            .allocator
            .lock()
            .reserve(input_bytes, ops.len() as u32)?;
        let copied = offset
            .checked_add(allocation_bytes)
            .is_some_and(|end| end <= self.inner.bytes);
        if !copied {
            self.inner.allocator.lock().release_reservation(key);
            return None;
        }
        unsafe {
            core::ptr::write_bytes(self.inner.virt.add(offset), 0, allocation_bytes);
            core::ptr::copy_nonoverlapping(
                ops.as_ptr().cast::<u8>(),
                self.inner.virt.add(offset),
                input_bytes,
            );
        }
        super::dma_flush(unsafe { self.inner.virt.add(offset) }, allocation_bytes);
        if !self.inner.allocator.lock().publish(key) {
            self.inner.allocator.lock().release_reservation(key);
            return None;
        }
        Some(GpgpuFontOutlineOps {
            inner: self.inner.clone(),
            key,
            offset,
            input_bytes,
            op_count: ops.len() as u32,
        })
    }

    pub(crate) fn capacity_bytes(&self) -> usize {
        self.inner.bytes
    }
}

#[derive(Copy, Clone, Debug)]
struct GpgpuFontOutlineOpsBinding {
    mapping_gpu: u64,
    mapping_phys: u64,
    mapping_bytes: usize,
    binding_gpu: u64,
    binding_bytes: usize,
    op_count: u32,
}

/// One immutable resident Skrifa outline stream. Clones are cheap leases; its
/// packed arena range is reclaimed only after the cache and every prepared
/// plan have released their handles.
pub(crate) struct GpgpuFontOutlineOps {
    inner: alloc::sync::Arc<GpgpuFontOutlineOpsArenaInner>,
    key: GpgpuFontCacheEntryKey,
    offset: usize,
    input_bytes: usize,
    op_count: u32,
}

impl GpgpuFontOutlineOps {
    pub(crate) const fn op_count(&self) -> u32 {
        self.op_count
    }

    pub(crate) const fn bytes(&self) -> usize {
        self.input_bytes
    }

    pub(crate) fn gpu(&self) -> u64 {
        self.inner.gpu + self.offset as u64
    }

    fn binding(&self) -> Option<GpgpuFontOutlineOpsBinding> {
        if self.inner.quarantined.load(Ordering::Acquire) {
            return None;
        }
        let binding_gpu = self.inner.gpu.checked_add(self.offset as u64)?;
        self.offset
            .checked_add(self.input_bytes)
            .filter(|end| *end <= self.inner.bytes)?;
        Some(GpgpuFontOutlineOpsBinding {
            mapping_gpu: self.inner.gpu,
            mapping_phys: self.inner.phys,
            mapping_bytes: self.inner.bytes,
            binding_gpu,
            binding_bytes: self.input_bytes,
            op_count: self.op_count,
        })
    }

    fn quarantine_backing(&self) {
        self.inner.quarantined.store(true, Ordering::Release);
        self.inner.allocator.lock().quarantine(self.key);
    }
}

impl Clone for GpgpuFontOutlineOps {
    fn clone(&self) -> Self {
        assert!(
            self.inner.allocator.lock().add_handle(self.key),
            "live font-outline recipe lost its generation"
        );
        Self {
            inner: self.inner.clone(),
            key: self.key,
            offset: self.offset,
            input_bytes: self.input_bytes,
            op_count: self.op_count,
        }
    }
}

impl Drop for GpgpuFontOutlineOps {
    fn drop(&mut self) {
        self.inner.allocator.lock().release_handle(self.key);
    }
}

pub(crate) const GPGPU_FONT_INSTANCE_DESCRIPTOR_DWORDS: usize = 64;
pub(crate) const GPGPU_FONT_INSTANCE_DESCRIPTOR_BYTES: usize =
    GPGPU_FONT_INSTANCE_DESCRIPTOR_DWORDS * core::mem::size_of::<u32>();
pub(crate) const GPGPU_FONT_INSTANCE_MAX_LAYERS: usize = 64;

const GPGPU_FONT_INSTANCE_MAGIC: u32 = 0x3149_5446;
const GPGPU_FONT_INSTANCE_FLAG_ENABLED: u32 = 1 << 0;
const GPGPU_FONT_INSTANCE_FLAG_BACKGROUND: u32 = 1 << 1;
const GPGPU_FONT_INSTANCE_FLAG_COLOR_ANIMATION: u32 = 1 << 2;
const GPGPU_FONT_INSTANCE_FLAG_TIMING_SINE: u32 = 1 << 3;
const GPGPU_FONT_INSTANCE_ITERATION_SHIFT: u32 = 4;
const GPGPU_FONT_INSTANCE_CHANNELS_SHIFT: u32 = 8;
const GPGPU_FONT_INSTANCE_FRAME_COUNT_SHIFT: u32 = 16;
const GPGPU_FONT_INSTANCE_FLAG_AFFINE_TRANSFORM: u32 = 1 << 20;
const GPGPU_FONT_INSTANCE_FLAG_MOTION: u32 = 1 << 21;

/// One immutable-layout font presentation record consumed directly by the
/// C++/IGC font-instance kernel. The complete record is copied into persistent
/// GPU-visible storage only when its style program changes.
#[derive(Copy, Clone, Debug)]
pub(crate) struct GpgpuFontInstanceDescriptor {
    dwords: [u32; GPGPU_FONT_INSTANCE_DESCRIPTOR_DWORDS],
}

impl GpgpuFontInstanceDescriptor {
    pub(crate) fn new(
        mask: GpgpuMask8Surface,
        mask_rect: GpgpuRect,
        foreground_rgba: u32,
    ) -> Option<Self> {
        if !mask.is_valid()
            || mask_rect.x < 0
            || mask_rect.y < 0
            || mask_rect.width == 0
            || mask_rect.height == 0
            || (mask_rect.x as u32).checked_add(mask_rect.width)? > mask.width
            || (mask_rect.y as u32).checked_add(mask_rect.height)? > mask.height
        {
            return None;
        }
        let mut dwords = [0u32; GPGPU_FONT_INSTANCE_DESCRIPTOR_DWORDS];
        dwords[0] = GPGPU_FONT_INSTANCE_MAGIC;
        dwords[1] = GPGPU_FONT_INSTANCE_FLAG_ENABLED;
        dwords[2] = mask.pitch_bytes;
        dwords[3] = mask_rect.x as u32;
        dwords[4] = mask_rect.y as u32;
        dwords[5] = mask_rect.width;
        dwords[6] = mask_rect.height;
        dwords[7] = 1.0f32.to_bits();
        dwords[8] = 0.0f32.to_bits();
        dwords[9] = 1.0f32.to_bits();
        dwords[10] = foreground_rgba;
        dwords[11] = 0;
        dwords[12] = 1.0f32.to_bits();
        dwords[13] = 1.0f32.to_bits();
        Some(Self { dwords })
    }

    pub(crate) fn set_transform(&mut self, scale: f32, rotation_radians: f32, opacity: f32) {
        let scale = scale.clamp(0.125, 8.0);
        let rotation = rotation_radians.clamp(-core::f32::consts::PI, core::f32::consts::PI);
        self.dwords[7] = scale.to_bits();
        self.dwords[8] = rotation.to_bits();
        self.dwords[9] = opacity.clamp(0.0, 1.0).to_bits();
        if (scale - 1.0).abs() > f32::EPSILON || rotation.abs() > f32::EPSILON {
            self.dwords[1] |= GPGPU_FONT_INSTANCE_FLAG_AFFINE_TRANSFORM;
        } else {
            self.dwords[1] &= !GPGPU_FONT_INSTANCE_FLAG_AFFINE_TRANSFORM;
        }
    }

    pub(crate) fn set_background(&mut self, rgba: u32) {
        self.dwords[11] = rgba;
        if rgba >> 24 == 0 {
            self.dwords[1] &= !GPGPU_FONT_INSTANCE_FLAG_BACKGROUND;
        } else {
            self.dwords[1] |= GPGPU_FONT_INSTANCE_FLAG_BACKGROUND;
        }
    }

    pub(crate) fn set_motion(
        &mut self,
        period_seconds: f32,
        phase_cycles: f32,
        rotation_amplitude_radians: f32,
        scale_amplitude: f32,
        opacity_amplitude: f32,
        translation_amplitude_px: [f32; 2],
    ) {
        self.dwords[13] = period_seconds.clamp(0.016, 600.0).to_bits();
        self.dwords[14] = phase_cycles.to_bits();
        self.dwords[15] = rotation_amplitude_radians
            .clamp(-core::f32::consts::PI, core::f32::consts::PI)
            .to_bits();
        self.dwords[16] = scale_amplitude.clamp(-0.875, 4.0).to_bits();
        self.dwords[17] = opacity_amplitude.clamp(-1.0, 1.0).to_bits();
        self.dwords[18] = translation_amplitude_px[0].clamp(-4096.0, 4096.0).to_bits();
        self.dwords[19] = translation_amplitude_px[1].clamp(-4096.0, 4096.0).to_bits();
        if rotation_amplitude_radians.abs() > f32::EPSILON
            || scale_amplitude.abs() > f32::EPSILON
            || opacity_amplitude.abs() > f32::EPSILON
            || translation_amplitude_px[0].abs() > f32::EPSILON
            || translation_amplitude_px[1].abs() > f32::EPSILON
        {
            self.dwords[1] |= GPGPU_FONT_INSTANCE_FLAG_MOTION;
        } else {
            self.dwords[1] &= !GPGPU_FONT_INSTANCE_FLAG_MOTION;
        }
    }

    pub(crate) fn set_color_animation(
        &mut self,
        channels: u8,
        timing_sine: bool,
        iteration: u8,
        duration_seconds: f32,
        frames: &[(u16, u32)],
    ) -> bool {
        if channels == 0
            || channels & !0x0F != 0
            || iteration > 2
            || !(2..=8).contains(&frames.len())
            || frames[0].0 != 0
            || frames[frames.len() - 1].0 != 1_000
            || frames.windows(2).any(|pair| pair[1].0 <= pair[0].0)
        {
            return false;
        }
        let mut flags = self.dwords[1]
            | GPGPU_FONT_INSTANCE_FLAG_COLOR_ANIMATION
            | (u32::from(iteration) << GPGPU_FONT_INSTANCE_ITERATION_SHIFT)
            | (u32::from(channels) << GPGPU_FONT_INSTANCE_CHANNELS_SHIFT)
            | ((frames.len() as u32) << GPGPU_FONT_INSTANCE_FRAME_COUNT_SHIFT);
        if timing_sine {
            flags |= GPGPU_FONT_INSTANCE_FLAG_TIMING_SINE;
        }
        self.dwords[1] = flags;
        self.dwords[12] = duration_seconds.clamp(0.016, 600.0).to_bits();
        for (index, &(offset, rgba)) in frames.iter().enumerate() {
            self.dwords[32 + index * 2] = u32::from(offset);
            self.dwords[33 + index * 2] = rgba;
        }
        true
    }
}

/// Persistent VM-backed descriptor storage shared by all font walkers in one
/// retained page. Its PPGTT range is unique for the allocation lifetime.
pub(crate) struct GpgpuOwnedFontInstanceState {
    phys: u64,
    gpu: u64,
    bytes: usize,
    virt: *mut u8,
    capacity: usize,
}

unsafe impl Send for GpgpuOwnedFontInstanceState {}
unsafe impl Sync for GpgpuOwnedFontInstanceState {}

impl GpgpuOwnedFontInstanceState {
    pub(crate) const fn phys(&self) -> u64 {
        self.phys
    }

    pub(crate) const fn gpu(&self) -> u64 {
        self.gpu
    }

    pub(crate) const fn bytes(&self) -> usize {
        self.bytes
    }

    pub(crate) const fn capacity(&self) -> usize {
        self.capacity
    }

    pub(crate) fn descriptor_gpu(&self, index: usize) -> Option<u64> {
        (index < self.capacity)
            .then_some(self.gpu + (index * GPGPU_FONT_INSTANCE_DESCRIPTOR_BYTES) as u64)
    }

    pub(crate) fn write(&self, index: usize, descriptor: &GpgpuFontInstanceDescriptor) -> bool {
        if index >= self.capacity || self.virt.is_null() {
            return false;
        }
        let offset = index * GPGPU_FONT_INSTANCE_DESCRIPTOR_BYTES;
        unsafe {
            core::ptr::copy_nonoverlapping(
                descriptor.dwords.as_ptr() as *const u8,
                self.virt.add(offset),
                GPGPU_FONT_INSTANCE_DESCRIPTOR_BYTES,
            );
        }
        super::dma_flush(unsafe { self.virt.add(offset) }, GPGPU_FONT_INSTANCE_DESCRIPTOR_BYTES);
        true
    }
}

impl Drop for GpgpuOwnedFontInstanceState {
    fn drop(&mut self) {
        if !retire_font_rcs_ppgtt_range(self.gpu, self.phys, self.bytes) {
            crate::log_error!(
                target: "gpgpu";
                "intel/gpgpu: font-instance state retirement refused phys=0x{:X} gpu=0x{:X} bytes={} action=no-unmap-no-free\n",
                self.phys,
                self.gpu,
                self.bytes,
            );
            return;
        }
        crate::dma::dealloc(self.virt, self.bytes);
        recycle_font_coverage_gpu_va(self.gpu, self.bytes);
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct GpgpuFontInstanceLayer {
    pub(crate) mask: GpgpuMask8Surface,
    pub(crate) mask_rect: GpgpuRect,
    pub(crate) dst_center: [f32; 2],
    pub(crate) dispatch_rect: GpgpuRect,
    pub(crate) descriptor_index: usize,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuFontInstanceBatchResult {
    pub(crate) ok: bool,
    pub(crate) submitted: bool,
    pub(crate) requested_layers: usize,
    pub(crate) active_walkers: usize,
    pub(crate) submits: usize,
    /// Exact destination ownership minted from the final cache-draining
    /// completion packet when this batch targets direct scanout.
    pub(crate) release: Option<GpgpuRgba8ReleaseFence>,
}

impl GpgpuMask8Surface {
    pub(crate) fn new(
        phys: u64,
        gpu: u64,
        bytes: usize,
        width: u32,
        height: u32,
        pitch_bytes: u32,
    ) -> Option<Self> {
        let surface = Self {
            phys,
            gpu,
            bytes,
            width,
            height,
            pitch_bytes,
        };
        surface.is_valid().then_some(surface)
    }

    pub(crate) fn is_valid(self) -> bool {
        if self.width == 0 || self.height == 0 || self.pitch_bytes < self.width {
            return false;
        }
        if (self.phys & 0xFFF) != 0 {
            return false;
        }
        let Some(last_row) = (self.height as usize)
            .checked_sub(1)
            .and_then(|row| row.checked_mul(self.pitch_bytes as usize))
        else {
            return false;
        };
        let Some(min_bytes) = last_row.checked_add(self.width as usize) else {
            return false;
        };
        min_bytes <= self.bytes
    }

    pub(crate) const fn bounds(self) -> GpgpuRect {
        GpgpuRect::new(0, 0, self.width, self.height)
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuGlyphMaskBlit {
    pub(crate) mask: GpgpuMask8Surface,
    pub(crate) mask_rect: GpgpuRect,
    pub(crate) dst: GpgpuRgba8Surface,
    pub(crate) dst_xy: GpgpuPoint,
    pub(crate) color_rgba: u32,
}

/// One independently positioned/colorized persistent R8 coverage layer.
/// The destination is supplied once for the complete scene-level batch.
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuGlyphMaskLayer {
    pub(crate) mask: GpgpuMask8Surface,
    pub(crate) mask_rect: GpgpuRect,
    pub(crate) dst_xy: GpgpuPoint,
    pub(crate) color_rgba: u32,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuGlyphMaskBatchResult {
    pub(crate) ok: bool,
    /// True once the command buffer reached the hardware submission boundary.
    /// An incomplete submitted batch must not be replayed over the same target.
    pub(crate) submitted: bool,
    pub(crate) requested_layers: usize,
    pub(crate) active_walkers: usize,
    pub(crate) submits: usize,
    /// Exact destination ownership minted from the final cache-draining
    /// completion packet when this batch targets direct scanout.
    pub(crate) release: Option<GpgpuRgba8ReleaseFence>,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuSubmitStats {
    pub(crate) spans: usize,
    pub(crate) submits: usize,
    pub(crate) submit_ms: u64,
    pub(crate) total_ms: u64,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct GpgpuSubmissionProbe {
    /// Entry into the queue function through a fully encoded, DMA-visible batch.
    pub(crate) queue_prepare_us: u64,
    /// Entry into the queue function through accepted GuC admission.
    pub(crate) queue_total_us: u64,
    pub(crate) forcewake_us: u64,
    pub(crate) state_map_us: u64,
    pub(crate) ppgtt_init_us: u64,
    pub(crate) kernel_map_us: u64,
    pub(crate) source_map_us: u64,
    pub(crate) destination_map_us: u64,
    pub(crate) batch_encode_us: u64,
    pub(crate) admission_us: u64,
    /// Accepted GuC admission through host observation of the post marker.
    pub(crate) submit_to_marker_us: u64,
    pub(crate) completion_polls: u64,
    /// Command-stream timestamp frequency resolved from the Gen11+ GT clock
    /// configuration for this boot.
    pub(crate) gpu_timestamp_frequency_hz: u64,
    /// Host and PIPE_CONTROL samples from the same 36-bit RCS timestamp
    /// domain, ordered across the complete video submission.
    pub(crate) guc_h2g_publish_sequence: u64,
    pub(crate) gpu_host_pre_submit_timestamp: u64,
    /// First host observation that GuC advanced the H2G head through this
    /// submission. This can lag actual consumption by one completion poll.
    pub(crate) gpu_h2g_consumed_observe_timestamp: u64,
    pub(crate) gpu_batch_enter_timestamp: u64,
    pub(crate) gpu_pre_walker_timestamp: u64,
    pub(crate) gpu_post_walker_timestamp: u64,
    pub(crate) gpu_post_release_timestamp: u64,
    pub(crate) gpu_host_observe_timestamp: u64,
    pub(crate) gpu_pre_submit_to_batch_ticks: u64,
    pub(crate) gpu_pre_submit_to_batch_us: u64,
    pub(crate) gpu_pre_submit_to_h2g_consumed_ticks: u64,
    pub(crate) gpu_pre_submit_to_h2g_consumed_us: u64,
    pub(crate) gpu_h2g_consumed_to_batch_ticks: u64,
    pub(crate) gpu_h2g_consumed_to_batch_us: u64,
    pub(crate) gpu_batch_to_walker_ticks: u64,
    pub(crate) gpu_batch_to_walker_us: u64,
    pub(crate) gpu_walker_ticks: u64,
    pub(crate) gpu_walker_us: u64,
    pub(crate) gpu_walker_to_release_ticks: u64,
    pub(crate) gpu_walker_to_release_us: u64,
    pub(crate) gpu_release_to_observe_ticks: u64,
    pub(crate) gpu_release_to_observe_us: u64,
    pub(crate) gpu_pre_submit_to_observe_ticks: u64,
    pub(crate) gpu_pre_submit_to_observe_us: u64,
    pub(crate) gpu_walker_timestamp_valid: bool,
    pub(crate) gpu_phase_timestamps_valid: bool,
    pub(crate) gpu_h2g_split_valid: bool,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct GpgpuWorklistSubmitStats {
    pub(crate) descs: usize,
    pub(crate) walkers: usize,
    pub(crate) submits: usize,
    pub(crate) submit_ms: u64,
    pub(crate) probe: GpgpuSubmissionProbe,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum GpgpuSubmissionOutcome {
    /// The request failed before crossing the hardware submission boundary.
    Unavailable,
    /// The post marker retired, so all destination writes are complete.
    Complete,
    /// Hardware accepted the request but its post marker did not retire.
    SubmittedIncomplete,
}

impl Default for GpgpuSubmissionOutcome {
    fn default() -> Self {
        Self::Unavailable
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuWorklistSubmitResult {
    pub(crate) stats: GpgpuWorklistSubmitStats,
    pub(crate) outcome: GpgpuSubmissionOutcome,
}

/// Opaque serial and executor submission for one slot in the persistent UI4
/// compositor ring.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct Ui4CompositorSubmission {
    serial: u64,
    gpu: crate::gpu::executor::KernelSubmission,
}

impl Ui4CompositorSubmission {
    /// Create a future for the exact vGPU timeline point backing this UI4 job.
    pub(crate) fn fence(self) -> crate::gpu::executor::GpuFence {
        self.gpu.fence()
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Ui4CompositorSubmitError {
    Busy,
    Unavailable,
    InvalidWorklist,
    SubmissionRejected,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Ui4CompositorCompletion {
    Pending,
    Complete(GpgpuWorklistSubmitStats),
    Failed,
    InvalidSubmission,
}

#[derive(Copy, Clone, Debug)]
pub(crate) enum Ui4VideoFrameCompletion {
    Pending,
    Complete {
        stats: GpgpuWorklistSubmitStats,
        release: GpgpuRgba8ReleaseFence,
    },
    Failed,
}

#[derive(Copy, Clone, Debug)]
pub(crate) enum Ui4SpriteSceneCompletion {
    Pending,
    Complete {
        stats: GpgpuWorklistSubmitStats,
        release: GpgpuRgba8ReleaseFence,
    },
    Failed,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuShellMandel64WorklistResult {
    pub(crate) ok: bool,
    pub(crate) submitted: bool,
    pub(crate) marker: u32,
    pub(crate) requested: usize,
    pub(crate) descriptors: usize,
    pub(crate) walkers: usize,
    pub(crate) pixels: usize,
    pub(crate) submit_ms: u64,
    pub(crate) desc_gpu: u64,
    pub(crate) last_src_xy: GpgpuPoint,
    pub(crate) last_dst_xy: GpgpuPoint,
    /// Present only for a complete direct-scanout render whose final
    /// PIPE_CONTROL and post-sync marker retired for this exact allocation.
    pub(crate) release: Option<GpgpuRgba8ReleaseFence>,
}

/// Common result for a full-surface compute node that does not own
/// presentation. UI4 consumers decide whether and when to publish the frame.
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuRgba8KernelResult {
    pub(crate) ok: bool,
    pub(crate) submitted: bool,
    pub(crate) marker: u32,
    pub(crate) submit_ms: u64,
    /// Exact-surface producer release, minted only after the kernel's final
    /// cache-draining PIPE_CONTROL and post-sync marker have retired.
    pub(crate) release: Option<GpgpuRgba8ReleaseFence>,
}

/// Proof that one full-surface compute dispatch retired its producer-release
/// packet for one exact allocation. The fields stay private so consumers
/// cannot manufacture display eligibility from an address alone.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct GpgpuRgba8ReleaseFence {
    phys: u64,
    byte_len: usize,
    sequence: u64,
}

impl GpgpuRgba8ReleaseFence {
    pub(crate) const fn matches(self, phys: u64, byte_len: usize) -> bool {
        self.phys == phys && self.byte_len == byte_len
    }

    pub(crate) const fn sequence(self) -> u64 {
        self.sequence
    }
}

static GPGPU_RGBA8_RELEASE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Copy, Clone, Debug, Default)]
struct DirectRcsDispatchOutcome {
    submitted: bool,
    observed: u32,
}

/// Establish the final producer-to-display boundary for an RGBA8 allocation.
///
/// Earlier resolve/coverage/decorations may use different completion packets;
/// this dedicated batch remaps the exact destination PAT3/UC, drains HDC/L3
/// and render-target writes, and proves retirement with an ordered
/// PIPE_CONTROL post-sync cookie. No pixel shader or surface copy runs here.
pub(crate) fn release_rgba8_surface_for_scanout(dst: GpgpuRgba8Surface) -> GpgpuRgba8KernelResult {
    let started = direct_rcs_now_tick();
    if !dst.is_valid() {
        return GpgpuRgba8KernelResult::default();
    }
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        return GpgpuRgba8KernelResult::default();
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        return GpgpuRgba8KernelResult::default();
    };
    let prepared = direct_rcs_forcewake(dev)
        && direct_rcs_map_state(dev, state)
        && direct_rcs_init_ppgtt(state)
        && direct_rcs_map_ppgtt_scanout(state, dst.gpu, dst.phys, dst.bytes)
        && direct_rcs_encode_rgba8_scanout_release_batch(state);
    let submission = if prepared {
        direct_rcs_submit_batch_state(dev, state)
    } else {
        DirectRcsSubmissionState::Rejected
    };
    let submitted = submission.may_have_submitted();
    let marker = if submission.can_poll() {
        direct_rcs_poll_result_slot_timeout_ms(
            state,
            RGBA8_SCANOUT_RELEASE_MARKER_SLOT,
            RGBA8_SCANOUT_RELEASE_MARKER,
            UI4_COMPUTE_PRODUCER_RETIRE_TIMEOUT_MS,
        )
    } else {
        0
    };
    let ok = marker == RGBA8_SCANOUT_RELEASE_MARKER;
    GpgpuRgba8KernelResult {
        ok,
        submitted,
        marker,
        submit_ms: direct_rcs_elapsed_ms_since(started),
        release: ok.then(|| gpgpu_rgba8_release(dst)),
    }
}

/// Font Engine equivalent of the scanout release packet. The packet encoding
/// is shared, but every mutable execution object and retirement proof belongs
/// to the Font GuC context.
pub(crate) fn font_release_rgba8_surface_for_scanout(
    dst: GpgpuRgba8Surface,
) -> GpgpuRgba8KernelResult {
    let started = direct_rcs_now_tick();
    if !dst.is_valid() {
        return GpgpuRgba8KernelResult::default();
    }
    let _guard = FONT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        return GpgpuRgba8KernelResult::default();
    };
    let Some(state) = font_rcs_state_once(dev) else {
        return GpgpuRgba8KernelResult::default();
    };
    let prepared = direct_rcs_forcewake(dev)
        && direct_rcs_map_state(dev, state)
        && font_rcs_init_ppgtt_once(state)
        && direct_rcs_map_ppgtt_scanout(state, dst.gpu, dst.phys, dst.bytes)
        && direct_rcs_encode_rgba8_scanout_release_batch(state);
    let submission = if prepared {
        font_rcs_submit_batch_state(dev, state)
    } else {
        DirectRcsSubmissionState::Rejected
    };
    let submitted = submission.may_have_submitted();
    let marker = if submission.can_poll() {
        font_rcs_poll_result_slot_timeout_ms(
            state,
            RGBA8_SCANOUT_RELEASE_MARKER_SLOT,
            RGBA8_SCANOUT_RELEASE_MARKER,
            UI4_COMPUTE_PRODUCER_RETIRE_TIMEOUT_MS,
        )
    } else {
        0
    };
    let ok = marker == RGBA8_SCANOUT_RELEASE_MARKER;
    GpgpuRgba8KernelResult {
        ok,
        submitted,
        marker,
        submit_ms: direct_rcs_elapsed_ms_since(started),
        release: ok.then(|| gpgpu_rgba8_release(dst)),
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuMandel64Placement {
    pub(crate) src_x: i32,
    pub(crate) src_y: i32,
    pub(crate) dst_x: i32,
    pub(crate) dst_y: i32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) view_height: u32,
    pub(crate) mirror_at_center: bool,
    pub(crate) iterations: u32,
}

#[derive(Copy, Clone, Debug)]
struct GpgpuRectWorklistDescBuffer {
    phys: u64,
    gpu: u64,
    virt: *mut u8,
    bytes: usize,
}

unsafe impl Send for GpgpuRectWorklistDescBuffer {}
unsafe impl Sync for GpgpuRectWorklistDescBuffer {}

#[cfg(test)]
mod gpgpu_font_cache_storage_tests {
    use super::*;

    #[test]
    fn r8_atlas_reclaims_only_after_last_immutable_lease() {
        let mut atlas = GpgpuMask8AtlasAllocator::new(256, 128, 4).unwrap();
        let (first_key, first_allocation, first_content) = atlas.reserve(33, 20).unwrap();
        assert_eq!(first_allocation, GpgpuRect::new(0, 0, 64, 20));
        assert_eq!(first_content, GpgpuRect::new(0, 0, 33, 20));
        assert!(atlas.publish(first_key));
        assert!(atlas.add_handle(first_key));

        atlas.release_handle(first_key);
        assert_eq!(atlas.stats().ready_entries, 1);
        atlas.release_handle(first_key);
        assert_eq!(atlas.stats().ready_entries, 0);

        let (second_key, second_allocation, second_content) = atlas.reserve(33, 20).unwrap();
        assert_eq!(second_allocation, first_allocation);
        assert_eq!(second_content, first_content);
        assert_ne!(second_key.generation, first_key.generation);
    }

    #[test]
    fn r8_atlas_quarantine_never_reuses_uncertain_destination() {
        let mut atlas = GpgpuMask8AtlasAllocator::new(128, 64, 2).unwrap();
        let (key, _, _) = atlas.reserve(128, 64).unwrap();
        assert!(atlas.publish(key));
        assert!(atlas.quarantine(key));
        atlas.release_handle(key);

        let stats = atlas.stats();
        assert_eq!(stats.quarantined_entries, 1);
        assert!(atlas.reserve(128, 64).is_none());
    }

    #[test]
    fn resident_outline_ranges_pack_and_reuse_with_new_generation() {
        let mut arena = GpgpuFontOutlineOpsAllocator::new(1024, 4).unwrap();
        let (first_key, first_offset, first_bytes) = arena.reserve(97, 4).unwrap();
        assert_eq!(first_offset, 0);
        assert_eq!(first_bytes, 128);
        assert!(arena.publish(first_key));
        arena.release_handle(first_key);

        let (second_key, second_offset, second_bytes) = arena.reserve(97, 4).unwrap();
        assert_eq!(second_offset, first_offset);
        assert_eq!(second_bytes, first_bytes);
        assert_ne!(second_key.generation, first_key.generation);
    }

    #[test]
    fn entry_caps_apply_even_when_storage_has_room() {
        let mut atlas = GpgpuMask8AtlasAllocator::new(256, 64, 1).unwrap();
        let (key, _, _) = atlas.reserve(32, 32).unwrap();
        assert!(atlas.publish(key));
        assert!(atlas.reserve(32, 32).is_none());

        let mut recipes = GpgpuFontOutlineOpsAllocator::new(1024, 1).unwrap();
        let (key, _, _) = recipes.reserve(32, 1).unwrap();
        assert!(recipes.publish(key));
        assert!(recipes.reserve(32, 1).is_none());
    }
}
