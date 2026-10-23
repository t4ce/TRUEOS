//! Unscaled, bit-preserving RGBA copies and rectangle fills on virgl.
//! Requests own snapshots, and only the live waiter publishes the readback.
//! A cancelled future therefore never leaves a borrowed frame/DMA allocation
//! in the renderer queue.
use super::*;
use crate::intel::{GucBcs0RgbaCopy, GucBcs0RgbaFill, GucBcs0RgbaSurface};
use core::sync::atomic::{AtomicBool, Ordering};

const VIRGL_CCMD_RESOURCE_COPY_REGION: u8 = 17;
const VIRGL_CCMD_CLEAR_TEXTURE: u8 = 47;
static REPORTED_SUCCESS: AtomicBool = AtomicBool::new(false);

struct Image {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

fn snapshot(surface: GucBcs0RgbaSurface) -> Result<Image, &'static str> {
    let row_bytes = (surface.width as usize)
        .checked_mul(4)
        .ok_or("virgl-blit-size")?;
    let bytes = row_bytes
        .checked_mul(surface.height as usize)
        .ok_or("virgl-blit-size")?;
    if surface.phys == 0
        || bytes == 0
        || row_bytes > surface.pitch_bytes as usize
        || (surface.height as usize - 1)
            .checked_mul(surface.pitch_bytes as usize)
            .and_then(|offset| offset.checked_add(row_bytes))
            .is_none_or(|end| end > surface.bytes)
        || surface.phys.checked_add(surface.bytes as u64).is_none()
    {
        return Err("virgl-blit-surface");
    }
    let source = crate::phys::phys_to_virt(surface.phys as usize) as *const u8;
    crate::intel::dma_cache_flush_range(source, surface.bytes);
    let mut pixels = Vec::with_capacity(bytes);
    for row in 0..surface.height as usize {
        let line = unsafe {
            core::slice::from_raw_parts(source.add(row * surface.pitch_bytes as usize), row_bytes)
        };
        pixels.extend_from_slice(line);
    }
    Ok(Image {
        width: surface.width,
        height: surface.height,
        pixels,
    })
}

fn publish(destination: GucBcs0RgbaSurface, pixels: &[u8]) -> Result<(), &'static str> {
    let row_bytes = destination.width as usize * 4;
    if pixels.len() != row_bytes * destination.height as usize {
        return Err("virgl-blit-readback-size");
    }
    let target = crate::phys::phys_to_virt(destination.phys as usize) as *mut u8;
    for row in 0..destination.height as usize {
        unsafe {
            core::ptr::copy_nonoverlapping(
                pixels.as_ptr().add(row * row_bytes),
                target.add(row * destination.pitch_bytes as usize),
                row_bytes,
            );
        }
    }
    crate::intel::dma_cache_flush_range(target, destination.bytes);
    Ok(())
}

/// Source and destination ownership follows the ordinary UI4 retired-buffer
/// contract. GPU work uses owned copies and cannot access either after await.
pub(crate) async fn copy_rgba(
    destination: GucBcs0RgbaSurface,
    copies: &[GucBcs0RgbaCopy],
) -> Result<(), &'static str> {
    if copies.is_empty() {
        return Ok(());
    }
    if !crate::ui4::text_blit::valid_copies(destination, copies) {
        return Err("virgl-blit-copy-bounds");
    }
    let target = snapshot(destination)?;
    let mut sources: Vec<(GucBcs0RgbaSurface, Image)> = Vec::new();
    let mut operations = Vec::with_capacity(copies.len());
    for &copy in copies {
        let index = if let Some(index) = sources
            .iter()
            .position(|(surface, _)| *surface == copy.source)
        {
            index
        } else {
            sources.push((copy.source, snapshot(copy.source)?));
            sources.len() - 1
        };
        operations.push((index, copy));
    }
    let output = super::super::requests::dispatch(move |compositor, gpu| {
        execute(
            compositor,
            gpu,
            target,
            sources.into_iter().map(|(_, image)| image).collect(),
            operations,
            Vec::new(),
        )
    })
    .await?;
    publish(destination, &output)
}

pub(crate) async fn fill_rgba(
    destination: GucBcs0RgbaSurface,
    fills: &[GucBcs0RgbaFill],
) -> Result<(), &'static str> {
    if fills.is_empty() {
        return Ok(());
    }
    if !crate::ui4::text_blit::valid_fills(destination, fills) {
        return Err("virgl-blit-fill-bounds");
    }
    let target = snapshot(destination)?;
    let fills = fills.to_vec();
    let output = super::super::requests::dispatch(move |compositor, gpu| {
        execute(compositor, gpu, target, Vec::new(), Vec::new(), fills)
    })
    .await?;
    publish(destination, &output)
}

fn resource_id(compositor: &mut Compositor) -> Result<u32, &'static str> {
    let id = compositor.next_id;
    compositor.next_id = id.checked_add(1).ok_or("virgl-blit-resource-id")?;
    Ok(id)
}

fn encode_copy(commands: &mut VirglCmdBuf, target: u32, source: u32, copy: GucBcs0RgbaCopy) {
    // virgl_protocol.h: RESOURCE_COPY_REGION has 13 payload words. Same
    // format, level and dimensions preserve all four bytes, including alpha.
    commands.push(virgl_cmd0(VIRGL_CCMD_RESOURCE_COPY_REGION, 0, 13));
    for value in [
        target,
        0,
        copy.destination_x,
        copy.destination_y,
        0,
        source,
        0,
        copy.source_x,
        copy.source_y,
        0,
        copy.width,
        copy.height,
        1,
    ] {
        commands.push(value);
    }
}

fn encode_fill(commands: &mut VirglCmdBuf, target: u32, fill: GucBcs0RgbaFill) {
    // CLEAR_TEXTURE accepts format-sized raw data. For RGBA8 its first data
    // dword is the packed RGBA pixel; the remaining union words are padding.
    commands.push(virgl_cmd0(VIRGL_CCMD_CLEAR_TEXTURE, 0, 12));
    for value in [
        target,
        0,
        fill.x,
        fill.y,
        0,
        fill.width,
        fill.height,
        1,
        fill.color,
        0,
        0,
        0,
    ] {
        commands.push(value);
    }
}

fn execute(
    compositor: &mut Compositor,
    gpu: &mut VirtioGpuLogo,
    target: Image,
    sources: Vec<Image>,
    copies: Vec<(usize, GucBcs0RgbaCopy)>,
    fills: Vec<GucBcs0RgbaFill>,
) -> Result<Vec<u8>, &'static str> {
    let mut resources = Vec::new();
    let result = (|| {
        let target_id = resource_id(compositor)?;
        resources.push(target_id);
        if !gpu.create_3d(
            target_id,
            target.width,
            target.height,
            TEXTURE_FORMAT_RGBA,
            (1 << 1) | (1 << 3),
            false,
        ) || !compositor.upload(
            gpu,
            target_id,
            target.width,
            target.height,
            target.width * 4,
            &target.pixels,
        ) {
            return Err("virgl-blit-target-upload");
        }
        let mut source_ids = Vec::with_capacity(sources.len());
        for source in &sources {
            let id = resource_id(compositor)?;
            resources.push(id);
            if !gpu.create_3d(id, source.width, source.height, TEXTURE_FORMAT_RGBA, 1 << 3, false)
                || !compositor.upload(
                    gpu,
                    id,
                    source.width,
                    source.height,
                    source.width * 4,
                    &source.pixels,
                )
            {
                return Err("virgl-blit-source-upload");
            }
            source_ids.push(id);
        }
        let mut commands = VirglCmdBuf::new();
        for (index, copy) in copies {
            encode_copy(&mut commands, target_id, source_ids[index], copy);
        }
        for fill in fills {
            encode_fill(&mut commands, target_id, fill);
        }
        if !compositor.submit(gpu, &commands) {
            return Err("virgl-blit-submit");
        }
        compositor.fence += 1;
        let output = gpu.readback_rgba(target_id, target.width, target.height, compositor.fence)?;
        if !REPORTED_SUCCESS.swap(true, Ordering::Relaxed) {
            crate::log_important!(target: "gfx"; "virgl-blit: ready copy=resource-copy-region fill=clear-texture rgba=bit-preserving completion=fenced readback=owned\n");
        }
        Ok(output)
    })();
    // The final draw fence/readback has retired before normal destruction.
    // A failed transport owns device quarantine; no original DMA was borrowed.
    for id in resources.into_iter().rev() {
        if !gpu.failed && !gpu.unref_resource(id) {
            return Err("virgl-blit-resource-release");
        }
    }
    result
}
