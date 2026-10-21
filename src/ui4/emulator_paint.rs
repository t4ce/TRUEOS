//! Deferred fixed-function paint for the emulator. Operations belong to an
//! exact UI4 backing buffer and are consumed under its ordinary read lease.
//! The virgl presenter executes them into a host texture before composition.
use super::{FrameHandle, FrameReadLease, FrameWriteLease};
use alloc::{sync::Arc, vec::Vec};
use spin::Mutex;

#[derive(Clone, Copy)]
pub(crate) struct Vertex {
    pub position: [f32; 2],
    pub uv: [f32; 2],
}

pub(crate) struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    pub premultiplied: bool,
}

#[derive(Clone)]
pub(crate) struct Paint {
    pub vertices: Arc<Vec<Vertex>>,
    pub image: Option<Arc<Image>>,
    pub color: u32,
    pub source_over: bool,
}

struct Buffer {
    frame: FrameHandle,
    index: u8,
    paints: Vec<Paint>,
    vertices: usize,
    image_bytes: usize,
}
static BUFFERS: Mutex<Vec<Buffer>> = Mutex::new(Vec::new());

pub(crate) fn begin(lease: FrameWriteLease) {
    let mut buffers = BUFFERS.lock();
    buffers.retain(|b| b.frame != lease.frame || b.index != lease.buffer_index);
}

pub(crate) fn forget(frame: FrameHandle) {
    BUFFERS.lock().retain(|b| b.frame != frame);
}

pub(crate) fn append(lease: FrameWriteLease, paints: Vec<Paint>) -> bool {
    let vertices = paints.iter().map(|p| p.vertices.len()).sum::<usize>();
    let image_bytes = paints
        .iter()
        .filter_map(|p| p.image.as_ref())
        .map(|i| i.pixels.len())
        .sum::<usize>();
    let mut buffers = BUFFERS.lock();
    let index = buffers
        .iter()
        .position(|b| b.frame == lease.frame && b.index == lease.buffer_index);
    let (old_vertices, old_bytes) = index
        .map(|i| (buffers[i].vertices, buffers[i].image_bytes))
        .unwrap_or_default();
    if old_vertices.saturating_add(vertices) > 1_000_000
        || old_bytes.saturating_add(image_bytes) > 32 * 1024 * 1024
    {
        return false;
    }
    let index = index.unwrap_or_else(|| {
        buffers.push(Buffer {
            frame: lease.frame,
            index: lease.buffer_index,
            paints: Vec::new(),
            vertices: 0,
            image_bytes: 0,
        });
        buffers.len() - 1
    });
    buffers[index].vertices += vertices;
    buffers[index].image_bytes += image_bytes;
    buffers[index].paints.extend(paints);
    true
}

pub(crate) fn snapshot(lease: FrameReadLease) -> Vec<Paint> {
    BUFFERS
        .lock()
        .iter()
        .find(|b| b.frame == lease.frame && b.index == lease.buffer_index)
        .map(|b| b.paints.clone())
        .unwrap_or_default()
}
