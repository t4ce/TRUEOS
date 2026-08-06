use alloc::vec::Vec;

use super::types::{ClError, ClResult, MemFlags, MemId};

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) struct BufferObject {
    pub(crate) id: MemId,
    pub(crate) size: usize,
    pub(crate) gpu: Option<u64>,
    pub(crate) phys: Option<u64>,
    pub(crate) virt: Option<*mut u8>,
    pub(crate) flags: MemFlags,
    pub(crate) host_shadow: Vec<u8>,
}

impl BufferObject {
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn new(id: MemId, flags: MemFlags, size: usize) -> Self {
        Self::try_new(id, flags, size).expect("opencl buffer host shadow allocation failed")
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn try_new(id: MemId, flags: MemFlags, size: usize) -> ClResult<Self> {
        let mut host_shadow = Vec::new();
        host_shadow
            .try_reserve_exact(size)
            .map_err(|_| ClError::OutOfHostMemory)?;
        host_shadow.resize(size, 0);

        Ok(Self {
            id,
            size,
            gpu: None,
            phys: None,
            virt: None,
            flags,
            host_shadow,
        })
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn with_gpu(mut self, gpu: u64) -> Self {
        self.gpu = Some(gpu);
        self
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn with_phys(mut self, phys: u64) -> Self {
        self.phys = Some(phys);
        self
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn with_virt(mut self, virt: *mut u8) -> Self {
        self.virt = Some(virt);
        self
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn bind_gpu(&mut self, gpu: u64) {
        self.gpu = Some(gpu);
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn bind_phys(&mut self, phys: u64) {
        self.phys = Some(phys);
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn bind_virt(&mut self, virt: *mut u8) {
        self.virt = Some(virt);
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn clear_bindings(&mut self) {
        self.gpu = None;
        self.phys = None;
        self.virt = None;
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn shadow(&self) -> &[u8] {
        &self.host_shadow
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn shadow_mut(&mut self) -> &mut [u8] {
        &mut self.host_shadow
    }
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) struct BufferRegistry {
    next_id: u64,
    buffers: Vec<BufferObject>,
}

impl BufferRegistry {
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn new() -> Self {
        Self {
            next_id: 1,
            buffers: Vec::new(),
        }
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn create(&mut self, flags: MemFlags, size: usize) -> ClResult<MemId> {
        self.create_buffer(flags, size)
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn create_buffer(&mut self, flags: MemFlags, size: usize) -> ClResult<MemId> {
        let id = self.next_mem_id();
        self.buffers
            .try_reserve(1)
            .map_err(|_| ClError::OutOfHostMemory)?;
        self.buffers.push(BufferObject::try_new(id, flags, size)?);
        Ok(id)
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn get(&self, id: MemId) -> Option<&BufferObject> {
        self.buffers.iter().find(|buffer| buffer.id == id)
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn get_mut(&mut self, id: MemId) -> Option<&mut BufferObject> {
        self.buffers.iter_mut().find(|buffer| buffer.id == id)
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn release(&mut self, id: MemId) -> Option<BufferObject> {
        let index = self.buffers.iter().position(|buffer| buffer.id == id)?;
        Some(self.buffers.remove(index))
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn list(&self) -> &[BufferObject] {
        &self.buffers
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn is_empty(&self) -> bool {
        self.buffers.is_empty()
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) fn len(&self) -> usize {
        self.buffers.len()
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    fn next_mem_id(&mut self) -> MemId {
        loop {
            let raw = self.next_id;
            self.next_id = self.next_id.wrapping_add(1);
            if self.next_id == 0 {
                self.next_id = 1;
            }

            let Some(id) = MemId::new(raw as _) else {
                continue;
            };

            if self.buffers.iter().all(|buffer| buffer.id != id) {
                return id;
            }
        }
    }
}

impl Default for BufferRegistry {
    fn default() -> Self {
        Self::new()
    }
}
