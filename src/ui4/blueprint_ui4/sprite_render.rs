//! Sprite ABI rendering through the resident GLSL vertex/fragment pipeline.
//! Sources are immutable CPU-authored uploads. Imports pin producer pages;
//! only exact Render0 retirement permits releasing the temporary GPU aliases.
use super::*;
use crate::intel::render::{self, ResidentSceneFragmentContract as Fragment};

#[derive(Copy, Clone)]
pub(super) struct Sprite {
    pub quad: TrueosUi4SpriteQuad,
    pub source: GpgpuRgba8Surface,
    pub source_virt: *const u8,
    pub premultiplied: bool,
}

pub(super) enum Failure {
    Rejected,
    Busy,
    Incomplete,
}

fn fragment(sprite: Sprite) -> Fragment {
    if sprite.quad.sprite_id == 0 {
        if sprite.quad.flags & SPRITE_QUAD_FLAG_SRC_OVER != 0 {
            Fragment::ConstantRgba
        } else {
            Fragment::ConstantRgbaCopy
        }
    } else if sprite.quad.flags & (SPRITE_QUAD_FLAG_SRC_OVER | SPRITE_QUAD_FLAG_PREMUL_COMPOSITOR)
        == 0
    {
        Fragment::ClipPosition3UvTextureCopy
    } else if sprite.premultiplied {
        Fragment::ClipPosition3UvTexturePremultiplied
    } else {
        Fragment::ClipPosition3UvTexture
    }
}

/// Use the existing sampled GLSL shader. A tinted CPU-authored upload is
/// prepared before submission; ordinary Hull and gallery images import without
/// copying. Premultiplied tint also multiplies RGB by the tint's alpha.
fn texture(sprite: Sprite) -> Result<render::ResidentSampledTexture, &'static str> {
    let tint = sprite.quad.color_rgba.to_le_bytes();
    if tint == [255; 4] {
        return render::import_resident_sampled_rgba8_texture(sprite.source, false);
    }
    if sprite.source_virt.is_null() {
        return Err("sprite-source-unavailable");
    }
    let mut pixels = Vec::new();
    let len = sprite.source.pitch_bytes as usize * sprite.source.height as usize;
    pixels
        .try_reserve_exact(len)
        .map_err(|_| "sprite-tint-allocation")?;
    // The parent's SURFACES lock pins these CPU-authored source bytes.
    pixels.extend_from_slice(unsafe { core::slice::from_raw_parts(sprite.source_virt, len) });
    for row in pixels.chunks_exact_mut(sprite.source.pitch_bytes as usize) {
        for pixel in row[..sprite.source.width as usize * 4].chunks_exact_mut(4) {
            for channel in 0..4 {
                pixel[channel] =
                    ((u32::from(pixel[channel]) * u32::from(tint[channel]) + 127) / 255) as u8;
                if sprite.premultiplied && channel < 3 {
                    pixel[channel] =
                        ((u32::from(pixel[channel]) * u32::from(tint[3]) + 127) / 255) as u8;
                }
            }
        }
    }
    render::create_resident_sampled_rgba8_texture(
        sprite.source.width,
        sprite.source.height,
        sprite.source.pitch_bytes,
        0,
        &pixels,
    )
}

pub(super) fn draw(
    destination: GpgpuRgba8Surface,
    sprites: &[Sprite],
) -> Result<render::ResidentSceneReleaseFence, Failure> {
    let mut meshes = Vec::new();
    let mut textures = Vec::new();
    let mut starts = Vec::new();
    let prepared: Result<(), Failure> = (|| {
        let mut start = 0;
        while start < sprites.len() {
            let first = sprites[start];
            let mut end = start + 1;
            while end < sprites.len()
                && sprites[end].quad.sprite_id == first.quad.sprite_id
                && sprites[end].quad.color_rgba == first.quad.color_rgba
                && fragment(sprites[end]) == fragment(first)
            {
                end += 1;
            }
            let mut vertices = Vec::with_capacity((end - start) * 4);
            let mut indices = Vec::with_capacity((end - start) * 6);
            for sprite in &sprites[start..end] {
                let q = sprite.quad;
                let base = vertices.len() as u32;
                for (x, y, u, v) in [
                    (q.c0_x, q.c0_y, q.c0_u, q.c0_v),
                    (q.c1_x, q.c1_y, q.c1_u, q.c1_v),
                    (q.c2_x, q.c2_y, q.c2_u, q.c2_v),
                    (q.c3_x, q.c3_y, q.c3_u, q.c3_v),
                ] {
                    vertices.push([
                        2.0 * x / destination.width as f32 - 1.0,
                        1.0 - 2.0 * y / destination.height as f32,
                        0.0,
                        u,
                        v,
                    ]);
                }
                indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
            }
            let mesh = if first.quad.sprite_id == 0 {
                render::create_resident_indexed_mesh(
                    &vertices
                        .iter()
                        .map(|v| [v[0], v[1], v[2]])
                        .collect::<Vec<_>>(),
                    &indices,
                    render::ResidentScenePrimitiveTopology::TriangleList,
                )
            } else {
                render::create_resident_textured_triangle_mesh(&vertices, &indices)
            }
            .map_err(|_| Failure::Rejected)?;
            meshes.push(mesh);
            textures.push(if first.quad.sprite_id == 0 {
                None
            } else {
                Some(texture(first).map_err(|_| Failure::Rejected)?)
            });
            starts.push(start);
            start = end;
        }
        Ok(())
    })();
    if prepared.is_err() {
        for mesh in &meshes {
            let _ = render::release_resident_triangle_mesh(mesh);
        }
        for texture in textures.iter().flatten() {
            let _ = render::release_resident_sampled_texture(texture);
        }
        return Err(Failure::Rejected);
    }
    let draws = meshes
        .iter()
        .zip(textures.iter())
        .zip(starts.iter())
        .map(|((mesh, texture), start)| {
            let sprite = sprites[*start];
            render::ResidentSceneDraw {
                mesh,
                depth_flags: None,
                rgba: sprite.quad.color_rgba.to_le_bytes(),
                sampled_texture: texture.as_ref(),
                fragment_contract: fragment(sprite),
                viewport_translation_px: [0.0; 2],
                topology: render::ResidentScenePrimitiveTopology::TriangleList,
                point_width_px: 0,
                line_width_px: 0.0,
                clip_adjacency: false,
            }
        })
        .collect::<Vec<_>>();
    let result = render::render_resident_indexed_scene_frame_premultiplied_direct_to_surface(
        &draws,
        None,
        destination,
        false,
    );
    let release = result
        .as_ref()
        .ok()
        .filter(|result| result.frame_complete && result.completed_draws == draws.len())
        .and_then(|result| result.release_fence)
        .filter(|release| release.matches(destination.phys, destination.bytes));
    let busy = matches!(result, Err("render-busy" | "render-storage-busy"));
    if release.is_some() || busy {
        for mesh in &meshes {
            if !render::release_resident_triangle_mesh(mesh) {
                return Err(Failure::Incomplete);
            }
        }
        for texture in textures.iter().flatten() {
            if !render::release_resident_sampled_texture(texture) {
                return Err(Failure::Incomplete);
            }
        }
    }
    // If acceptance/retirement is ambiguous, these allocations and aliases stay
    // pinned and the caller quarantines its producer and source allocations.
    release.ok_or(if busy {
        Failure::Busy
    } else {
        Failure::Incomplete
    })
}
