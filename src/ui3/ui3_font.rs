use alloc::vec::Vec;

use serde_json::Value;

const UI3_LAYOUT_TEXT_NODE_MAX: usize = 512;
const UI3_TEXT_PLACEMENT_MAX: usize = 4096;
const UI3_TEXT_SUBMIT_BATCH_PLACEMENTS: usize = 256;
const UI3_TEXT_COLOR_RGBA: u32 = 0x0000_0000;
const UI3_PAINTED_BOX_MAX: usize = 25;
const UI3_GRADIENT_DESCS_PER_PAINTED_BOX_MAX: usize = 5;
const UI3_GRADIENT_DESC_MAX: usize = UI3_PAINTED_BOX_MAX * UI3_GRADIENT_DESCS_PER_PAINTED_BOX_MAX;
const UI3_SUMMARY_ICON_CLOSED: char = '\u{25B6}';
const UI3_SUMMARY_ICON_OPEN: char = '\u{1F53D}';
const UI3_SUMMARY_ICON_X_PAD: f32 = 4.0;

#[derive(Debug, Default)]
pub(crate) struct Ui3FontScratch {
    placements: Vec<crate::intel::gpgpu::GpgpuSprite64Placement>,
    gradients: Vec<crate::intel::gpgpu::GpgpuGradientRect>,
    control_gradients: Vec<crate::intel::gpgpu::GpgpuGradientRect>,
    assets: Vec<Ui3AssetPlacement>,
    layout_adjustments: Vec<Ui3LayoutAdjustment>,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct Ui3FontScene {
    pub(crate) browser_instance_id: u32,
    pub(crate) scroll_y: f32,
    pub(crate) viewport_width: u32,
    pub(crate) viewport_height: u32,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct Ui3FontDrawResult {
    pub(crate) text_nodes: usize,
    pub(crate) placements: usize,
    pub(crate) assets: usize,
    pub(crate) layout_shift_px: u32,
    pub(crate) batches: usize,
    pub(crate) gradients: usize,
    pub(crate) clipped: usize,
    pub(crate) clear_ok: bool,
    pub(crate) clear_ms: u64,
    pub(crate) rect_ms: u64,
    pub(crate) asset_ms: u64,
    pub(crate) text_ms: u64,
    pub(crate) show_ms: u64,
    pub(crate) submit_ok: bool,
    pub(crate) presented: bool,
    pub(crate) submit_ms: u64,
    pub(crate) present_ms: u64,
}

#[derive(Copy, Clone, Debug, Default)]
struct Ui3TextCollectStats {
    text_nodes: usize,
    clipped: usize,
    text_node_cap_hit: bool,
    placement_cap_hit: bool,
}

#[derive(Copy, Clone, Debug, Default)]
struct Ui3GradientCollectStats {
    painted_boxes: usize,
    painted_box_cap_hit: bool,
    gradient_desc_cap_hit: bool,
}

#[derive(Clone, Debug)]
struct Ui3AssetPlacement {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    asset: crate::surfer::asset_shack::BrowserAssetReady,
}

#[derive(Copy, Clone, Debug)]
struct Ui3LayoutAdjustment {
    after_y: f32,
    delta_y: f32,
}

#[allow(dead_code)]
pub(crate) fn draw_paint_plan_primary(
    plan: &Value,
    scene: Ui3FontScene,
    scratch: &mut Ui3FontScratch,
    present_reason: &str,
) -> Ui3FontDrawResult {
    scratch.placements.clear();
    scratch.gradients.clear();
    scratch.control_gradients.clear();
    scratch.assets.clear();
    scratch.layout_adjustments.clear();
    let mut collect = Ui3TextCollectStats::default();
    let mut gradient_collect = Ui3GradientCollectStats::default();
    collect_paint_plan_ready_assets(
        plan,
        scene.scroll_y,
        scene,
        &mut scratch.assets,
        &mut scratch.layout_adjustments,
    );
    collect_paint_plan_rect_gradients(
        plan,
        scene.scroll_y,
        scene,
        &scratch.layout_adjustments,
        &mut scratch.gradients,
        &mut scratch.control_gradients,
        &mut gradient_collect,
    );
    collect_paint_plan_summary_icons(
        plan,
        scene.scroll_y,
        scene,
        &scratch.layout_adjustments,
        &mut scratch.placements,
        &mut collect,
    );
    collect_paint_plan_text_placements(
        plan,
        scene.scroll_y,
        scene,
        &scratch.layout_adjustments,
        &mut scratch.placements,
        &mut collect,
    );
    finish_draw_primary(scratch, collect, gradient_collect, scene, present_reason)
}

pub(crate) fn draw_paint_plan_backend(
    plan: &Value,
    scene: Ui3FontScene,
    scratch: &mut Ui3FontScratch,
    surface: &crate::ui3::ui3_surface::Ui3RgbaSurface,
    reason: &str,
) -> Ui3FontDrawResult {
    draw_paint_plan_backend_band(plan, scene, scratch, surface, 0, surface.height, reason)
}

pub(crate) fn draw_paint_plan_backend_band(
    plan: &Value,
    mut scene: Ui3FontScene,
    scratch: &mut Ui3FontScratch,
    surface: &crate::ui3::ui3_surface::Ui3RgbaSurface,
    band_y: u32,
    band_height: u32,
    reason: &str,
) -> Ui3FontDrawResult {
    scene.scroll_y = band_y as f32;
    scene.viewport_height = band_height;
    scratch.placements.clear();
    scratch.gradients.clear();
    scratch.control_gradients.clear();
    scratch.assets.clear();
    scratch.layout_adjustments.clear();
    let mut collect = Ui3TextCollectStats::default();
    let mut gradient_collect = Ui3GradientCollectStats::default();
    collect_paint_plan_ready_assets(
        plan,
        scene.scroll_y,
        scene,
        &mut scratch.assets,
        &mut scratch.layout_adjustments,
    );
    collect_paint_plan_rect_gradients(
        plan,
        scene.scroll_y,
        scene,
        &scratch.layout_adjustments,
        &mut scratch.gradients,
        &mut scratch.control_gradients,
        &mut gradient_collect,
    );
    collect_paint_plan_summary_icons(
        plan,
        scene.scroll_y,
        scene,
        &scratch.layout_adjustments,
        &mut scratch.placements,
        &mut collect,
    );
    collect_paint_plan_text_placements(
        plan,
        scene.scroll_y,
        scene,
        &scratch.layout_adjustments,
        &mut scratch.placements,
        &mut collect,
    );
    finish_draw_backend(
        scratch,
        collect,
        gradient_collect,
        scene,
        surface,
        band_y,
        band_height,
        reason,
    )
}

#[allow(dead_code)]
fn finish_draw_primary(
    scratch: &mut Ui3FontScratch,
    collect: Ui3TextCollectStats,
    gradient_collect: Ui3GradientCollectStats,
    scene: Ui3FontScene,
    present_reason: &str,
) -> Ui3FontDrawResult {
    let gradient_count = scratch
        .gradients
        .len()
        .saturating_add(scratch.control_gradients.len());
    log_collect_caps(scratch, collect, gradient_collect, gradient_count, scene);
    let has_assets = !scratch.assets.is_empty();
    let has_gradients = !scratch.gradients.is_empty() || !scratch.control_gradients.is_empty();
    let should_present_clear = if has_assets {
        !has_gradients
    } else {
        scratch.placements.is_empty() && !has_gradients
    };
    let clear_result = crate::intel::gpgpu::clear_primary_rgba8_white_for_redraw_stats(
        should_present_clear,
        present_reason,
    );
    let gradient_result = if scratch.gradients.is_empty() {
        None
    } else {
        crate::intel::gpgpu::gradient_rects_rgba8_over_primary(
            scratch.gradients.as_slice(),
            scratch.control_gradients.is_empty() && (has_assets || scratch.placements.is_empty()),
        )
    };
    let control_gradient_result = if scratch.control_gradients.is_empty() {
        None
    } else {
        crate::intel::gpgpu::gradient_rects_rgba8_over_primary(
            scratch.control_gradients.as_slice(),
            has_assets || scratch.placements.is_empty(),
        )
    };
    let asset_start = embassy_time_driver::now();
    let assets_drawn = draw_ready_assets_primary(scratch.assets.as_slice());
    let asset_ms = if assets_drawn == 0 {
        0
    } else {
        elapsed_ms_since(asset_start)
    };
    let mut text_batches = 0usize;
    let mut text_submitted = false;
    let mut text_presented = false;
    let mut text_submit_ms = 0u64;
    let mut text_present_ms = 0u64;
    if !scratch.placements.is_empty() {
        let total_batches = scratch
            .placements
            .len()
            .saturating_add(UI3_TEXT_SUBMIT_BATCH_PLACEMENTS - 1)
            / UI3_TEXT_SUBMIT_BATCH_PLACEMENTS;
        for (batch_index, chunk) in scratch
            .placements
            .chunks(UI3_TEXT_SUBMIT_BATCH_PLACEMENTS)
            .enumerate()
        {
            let present = batch_index + 1 == total_batches;
            let Some(result) =
                crate::intel::gpgpu::sprite64_worklist_primary(chunk, present, present_reason)
            else {
                continue;
            };
            text_batches = text_batches.saturating_add(1);
            text_submitted |= result.submitted;
            text_presented |= result.presented;
            text_submit_ms = text_submit_ms.saturating_add(result.submit_ms);
            text_present_ms = text_present_ms.saturating_add(result.present_ms);
        }
    }

    Ui3FontDrawResult {
        text_nodes: collect.text_nodes,
        placements: scratch.placements.len(),
        assets: assets_drawn,
        layout_shift_px: layout_adjustment_total_px(scratch.layout_adjustments.as_slice()),
        batches: text_batches,
        gradients: gradient_count,
        clipped: collect.clipped,
        clear_ok: clear_result.is_some(),
        clear_ms: clear_result
            .as_ref()
            .map(|result| result.total_ms)
            .unwrap_or(0),
        rect_ms: gradient_result
            .as_ref()
            .map(|result| result.fill_ms.saturating_add(result.blend_ms))
            .unwrap_or(0)
            .saturating_add(
                control_gradient_result
                    .as_ref()
                    .map(|result| result.fill_ms.saturating_add(result.blend_ms))
                    .unwrap_or(0),
            ),
        asset_ms,
        text_ms: text_submit_ms,
        show_ms: clear_result
            .as_ref()
            .map(|result| result.present_ms)
            .unwrap_or(0)
            .saturating_add(
                gradient_result
                    .as_ref()
                    .map(|result| result.present_ms)
                    .unwrap_or(0),
            )
            .saturating_add(
                control_gradient_result
                    .as_ref()
                    .map(|result| result.present_ms)
                    .unwrap_or(0),
            )
            .saturating_add(text_present_ms),
        submit_ok: text_submitted
            || assets_drawn != 0
            || gradient_result.as_ref().is_some_and(|result| result.ok)
            || control_gradient_result
                .as_ref()
                .is_some_and(|result| result.ok),
        presented: text_presented
            || assets_drawn != 0
            || gradient_result
                .as_ref()
                .is_some_and(|result| result.presented)
            || control_gradient_result
                .as_ref()
                .is_some_and(|result| result.presented)
            || clear_result
                .as_ref()
                .is_some_and(|result| result.present_ms != 0),
        submit_ms: clear_result
            .as_ref()
            .map(|result| result.submit_ms)
            .unwrap_or(0)
            .saturating_add(
                gradient_result
                    .as_ref()
                    .map(|result| result.fill_ms.saturating_add(result.blend_ms))
                    .unwrap_or(0),
            )
            .saturating_add(
                control_gradient_result
                    .as_ref()
                    .map(|result| result.fill_ms.saturating_add(result.blend_ms))
                    .unwrap_or(0),
            )
            .saturating_add(asset_ms)
            .saturating_add(text_submit_ms),
        present_ms: clear_result
            .as_ref()
            .map(|result| result.present_ms)
            .unwrap_or(0)
            .saturating_add(
                gradient_result
                    .as_ref()
                    .map(|result| result.present_ms)
                    .unwrap_or(0),
            )
            .saturating_add(
                control_gradient_result
                    .as_ref()
                    .map(|result| result.present_ms)
                    .unwrap_or(0),
            )
            .saturating_add(text_present_ms),
    }
}

fn finish_draw_backend(
    scratch: &mut Ui3FontScratch,
    collect: Ui3TextCollectStats,
    gradient_collect: Ui3GradientCollectStats,
    scene: Ui3FontScene,
    surface: &crate::ui3::ui3_surface::Ui3RgbaSurface,
    target_y: u32,
    clear_height: u32,
    reason: &str,
) -> Ui3FontDrawResult {
    let Ok(target_y_i32) = i32::try_from(target_y) else {
        return Ui3FontDrawResult::default();
    };
    let gradient_count = scratch
        .gradients
        .len()
        .saturating_add(scratch.control_gradients.len());
    log_collect_caps(scratch, collect, gradient_collect, gradient_count, scene);

    let clear_start = embassy_time_driver::now();
    surface.clear_white_range(target_y, clear_height);
    let clear_ms = elapsed_ms_since(clear_start);

    let gradient_start = embassy_time_driver::now();
    let gradient_stats = draw_backend_gradients(surface, scratch.gradients.as_slice(), target_y);
    let control_gradient_stats =
        draw_backend_gradients(surface, scratch.control_gradients.as_slice(), target_y);
    let rect_ms = if gradient_stats.submits != 0 || control_gradient_stats.submits != 0 {
        elapsed_ms_since(gradient_start)
    } else {
        0
    };

    let asset_start = embassy_time_driver::now();
    let assets_drawn = draw_ready_assets_backend(surface, scratch.assets.as_slice(), target_y);
    let asset_ms = if assets_drawn == 0 {
        0
    } else {
        elapsed_ms_since(asset_start)
    };

    let text_start = embassy_time_driver::now();
    let mut text_batches = 0usize;
    let mut text_submitted = false;
    let mut shifted = Vec::new();
    if !scratch.placements.is_empty() {
        for page in surface.pages() {
            shifted.clear();
            let page_y0 = page.y0 as i32;
            let page_y1 = page_y0.saturating_add(page.height as i32);
            let cell = crate::intel::gpgpu::SPRITE64_WORKLIST_CELL_PIXELS as i32;
            for placement in scratch.placements.iter().copied() {
                let doc_placement = placement.translated(0, target_y_i32);
                let glyph_y0 = doc_placement.dst_y();
                let glyph_y1 = glyph_y0.saturating_add(cell);
                if glyph_y1 <= page_y0 || glyph_y0 >= page_y1 {
                    continue;
                }
                shifted.push(doc_placement.translated(0, -page_y0));
            }
            if shifted.is_empty() {
                continue;
            }
            let dst = page.as_gpgpu(surface.width, surface.pitch_bytes);
            for chunk in shifted.chunks(UI3_TEXT_SUBMIT_BATCH_PLACEMENTS) {
                let Some(result) =
                    crate::intel::gpgpu::sprite64_worklist_surface(chunk, dst, reason)
                else {
                    continue;
                };
                text_batches = text_batches.saturating_add(1);
                text_submitted |= result.submitted;
            }
        }
    }
    let text_ms = if text_batches == 0 {
        0
    } else {
        elapsed_ms_since(text_start)
    };

    Ui3FontDrawResult {
        text_nodes: collect.text_nodes,
        placements: scratch.placements.len(),
        assets: assets_drawn,
        layout_shift_px: layout_adjustment_total_px(scratch.layout_adjustments.as_slice()),
        batches: text_batches,
        gradients: gradient_count,
        clipped: collect.clipped,
        clear_ok: true,
        clear_ms,
        rect_ms,
        asset_ms,
        text_ms,
        show_ms: 0,
        submit_ok: text_submitted
            || assets_drawn != 0
            || gradient_stats.submits != 0
            || control_gradient_stats.submits != 0,
        presented: false,
        submit_ms: clear_ms
            .saturating_add(rect_ms)
            .saturating_add(asset_ms)
            .saturating_add(text_ms),
        present_ms: 0,
    }
}

fn draw_backend_gradients(
    surface: &crate::ui3::ui3_surface::Ui3RgbaSurface,
    gradients: &[crate::intel::gpgpu::GpgpuGradientRect],
    target_y: u32,
) -> crate::intel::gpgpu::GpgpuWorklistSubmitStats {
    let mut total = crate::intel::gpgpu::GpgpuWorklistSubmitStats::default();
    if gradients.is_empty() {
        return total;
    }

    let mut descs = Vec::new();
    for page in surface.pages() {
        descs.clear();
        for gradient in gradients {
            let Some(doc_rect) = offset_rect_y(gradient.rect, target_y) else {
                continue;
            };
            let Some(rect) = clip_doc_rect_to_page(doc_rect, surface.width, page.y0, page.height)
            else {
                continue;
            };
            if rect.width > u16::MAX as u32 || rect.height > u16::MAX as u32 {
                continue;
            }
            let Ok(dst_x) = i16::try_from(rect.x) else {
                continue;
            };
            let Ok(dst_y) = i16::try_from(rect.y) else {
                continue;
            };
            descs.push(crate::intel::gpgpu::GradientRectWorklistRgba8Desc {
                dst_xy: pack_i16_pair_u32(dst_x, dst_y),
                size: pack_u16_pair_u32(rect.width as u16, rect.height as u16),
                color0_rgba: gradient.color0_rgba,
                color1_rgba: gradient.color1_rgba,
                flags: if gradient.vertical {
                    crate::intel::gpgpu::GRADIENT_RECT_WORKLIST_FLAG_VERTICAL
                } else {
                    0
                },
            });
        }
        if descs.is_empty() {
            continue;
        }
        let stats = crate::intel::gpgpu::gradient_rect_worklist_rgba8_stats(
            page.as_gpgpu(surface.width, surface.pitch_bytes),
            descs.as_slice(),
        );
        total.descs = total.descs.saturating_add(stats.descs);
        total.walkers = total.walkers.saturating_add(stats.walkers);
        total.submits = total.submits.saturating_add(stats.submits);
        total.submit_ms = total.submit_ms.saturating_add(stats.submit_ms);
    }
    total
}

fn draw_ready_assets_backend(
    surface: &crate::ui3::ui3_surface::Ui3RgbaSurface,
    assets: &[Ui3AssetPlacement],
    target_y: u32,
) -> usize {
    let mut drawn = 0usize;
    for placement in assets {
        if placement.asset.width == 0
            || placement.asset.height == 0
            || placement.asset.rgba.is_empty()
            || placement.width == 0
            || placement.height == 0
        {
            continue;
        }
        let mut placement_drawn = false;
        for page in surface.pages() {
            placement_drawn |= blend_asset_into_backend_page(surface, page, placement, target_y);
        }
        if placement_drawn {
            drawn = drawn.saturating_add(1);
        }
    }
    drawn
}

fn blend_asset_into_backend_page(
    surface: &crate::ui3::ui3_surface::Ui3RgbaSurface,
    page: &crate::ui3::ui3_surface::Ui3RgbaPage,
    placement: &Ui3AssetPlacement,
    target_y: u32,
) -> bool {
    if page.virt.is_null() {
        return false;
    }
    let dst_x0 = placement.x.max(0) as u32;
    let dst_x1 = (placement.x as i64)
        .saturating_add(placement.width as i64)
        .min(surface.width as i64)
        .max(0) as u32;
    let placement_y_doc = (placement.y as i64).saturating_add(target_y as i64);
    let dst_y0_doc = placement_y_doc.max(page.y0 as i64).max(0) as u32;
    let dst_y1_doc = placement_y_doc
        .saturating_add(placement.height as i64)
        .min(page.y0.saturating_add(page.height) as i64)
        .max(0) as u32;
    if dst_x0 >= dst_x1 || dst_y0_doc >= dst_y1_doc {
        return false;
    }

    let src_pitch = placement.asset.width as usize * 4;
    let dst_pitch = surface.pitch_bytes as usize;
    for doc_y in dst_y0_doc..dst_y1_doc {
        let rel_y = (doc_y as i64).saturating_sub(placement_y_doc).max(0) as u32;
        let src_y = (rel_y as u64)
            .saturating_mul(placement.asset.height as u64)
            .checked_div(placement.height.max(1) as u64)
            .unwrap_or(0)
            .min(placement.asset.height.saturating_sub(1) as u64) as u32;
        let dst_y = doc_y.saturating_sub(page.y0);
        let dst_row = unsafe { page.virt.add(dst_y as usize * dst_pitch) };
        let src_row_off = src_y as usize * src_pitch;
        let Some(src_row) = placement
            .asset
            .rgba
            .get(src_row_off..src_row_off.saturating_add(src_pitch))
        else {
            return false;
        };
        for dst_x in dst_x0..dst_x1 {
            let rel_x = (dst_x as i64).saturating_sub(placement.x as i64).max(0) as u32;
            let src_x = (rel_x as u64)
                .saturating_mul(placement.asset.width as u64)
                .checked_div(placement.width.max(1) as u64)
                .unwrap_or(0)
                .min(placement.asset.width.saturating_sub(1) as u64)
                as usize;
            let src_off = src_x.saturating_mul(4);
            let r = src_row[src_off] as u32;
            let g = src_row[src_off + 1] as u32;
            let b = src_row[src_off + 2] as u32;
            let a = src_row[src_off + 3] as u32;
            if a == 0 {
                continue;
            }
            let dst_px = unsafe { dst_row.add(dst_x as usize * 4) };
            if a == 0xFF {
                unsafe {
                    core::ptr::write_volatile(dst_px, r as u8);
                    core::ptr::write_volatile(dst_px.add(1), g as u8);
                    core::ptr::write_volatile(dst_px.add(2), b as u8);
                    core::ptr::write_volatile(dst_px.add(3), 0xFF);
                }
            } else {
                let dr = unsafe { core::ptr::read_volatile(dst_px) } as u32;
                let dg = unsafe { core::ptr::read_volatile(dst_px.add(1)) } as u32;
                let db = unsafe { core::ptr::read_volatile(dst_px.add(2)) } as u32;
                let da = unsafe { core::ptr::read_volatile(dst_px.add(3)) } as u32;
                let inv = 255 - a;
                unsafe {
                    core::ptr::write_volatile(dst_px, ((r * a + dr * inv + 127) / 255) as u8);
                    core::ptr::write_volatile(
                        dst_px.add(1),
                        ((g * a + dg * inv + 127) / 255) as u8,
                    );
                    core::ptr::write_volatile(
                        dst_px.add(2),
                        ((b * a + db * inv + 127) / 255) as u8,
                    );
                    core::ptr::write_volatile(
                        dst_px.add(3),
                        ((a * 255 + da * inv + 127) / 255) as u8,
                    );
                }
            }
        }
    }
    crate::intel::dma_cache_flush_range(page.virt as *const u8, page.bytes);
    true
}

fn clip_doc_rect_to_page(
    rect: crate::intel::gpgpu::GpgpuRect,
    surface_width: u32,
    page_y0: u32,
    page_height: u32,
) -> Option<crate::intel::gpgpu::GpgpuRect> {
    if rect.width == 0 || rect.height == 0 || surface_width == 0 || page_height == 0 {
        return None;
    }
    let x0 = (rect.x as i64).max(0);
    let y0 = (rect.y as i64).max(page_y0 as i64);
    let x1 = (rect.x as i64)
        .saturating_add(rect.width as i64)
        .min(surface_width as i64);
    let y1 = (rect.y as i64)
        .saturating_add(rect.height as i64)
        .min(page_y0.saturating_add(page_height) as i64);
    if x0 >= x1 || y0 >= y1 {
        return None;
    }
    Some(crate::intel::gpgpu::GpgpuRect::new(
        x0 as i32,
        y0.saturating_sub(page_y0 as i64) as i32,
        (x1 - x0) as u32,
        (y1 - y0) as u32,
    ))
}

fn offset_rect_y(
    rect: crate::intel::gpgpu::GpgpuRect,
    target_y: u32,
) -> Option<crate::intel::gpgpu::GpgpuRect> {
    let target_y = i32::try_from(target_y).ok()?;
    Some(crate::intel::gpgpu::GpgpuRect::new(
        rect.x,
        rect.y.checked_add(target_y)?,
        rect.width,
        rect.height,
    ))
}

fn pack_i16_pair_u32(x: i16, y: i16) -> u32 {
    (((y as u16 as u32) & 0xFFFF) << 16) | ((x as u16 as u32) & 0xFFFF)
}

fn pack_u16_pair_u32(x: u16, y: u16) -> u32 {
    (u32::from(y) << 16) | u32::from(x)
}

fn collect_paint_plan_ready_assets(
    plan: &Value,
    scroll_y: f32,
    scene: Ui3FontScene,
    assets: &mut Vec<Ui3AssetPlacement>,
    adjustments: &mut Vec<Ui3LayoutAdjustment>,
) {
    if scene.browser_instance_id == 0 {
        return;
    }
    let Some(boxes) = plan.get("paintedBoxes").and_then(Value::as_array) else {
        return;
    };
    for item in boxes {
        if assets.len() >= UI3_PAINTED_BOX_MAX {
            break;
        }
        if item.get("role").and_then(Value::as_str) != Some("image") {
            continue;
        }
        let Some(key) = item.get("key").and_then(Value::as_str) else {
            continue;
        };
        let x = json_f32_field(item, "x").unwrap_or(0.0);
        let y_doc = json_f32_field(item, "y").unwrap_or(0.0);
        let Some(width) = json_u32_field(item, "width") else {
            continue;
        };
        let Some(height) = json_u32_field(item, "height") else {
            continue;
        };
        if width == 0 || height == 0 || x + width as f32 <= 0.0 || x >= scene.viewport_width as f32
        {
            continue;
        }
        let Some(asset) =
            crate::surfer::asset_shack::ready_asset_for_tag(scene.browser_instance_id, key)
        else {
            continue;
        };
        let explicit_width = image_box_has_explicit_size(item, "width");
        let explicit_height = image_box_has_explicit_size(item, "height");
        let (resolved_width, resolved_height) =
            resolved_asset_size(&asset, x, width, height, explicit_width, explicit_height);
        if !explicit_height && resolved_height > height {
            adjustments.push(Ui3LayoutAdjustment {
                after_y: y_doc + height as f32,
                delta_y: resolved_height.saturating_sub(height) as f32,
            });
        }
        let adjusted_y = y_doc + layout_shift_for_y(y_doc, adjustments) - scroll_y;
        if adjusted_y + resolved_height as f32 <= 0.0
            || adjusted_y >= scene.viewport_height as f32
            || x + resolved_width as f32 <= 0.0
            || x >= scene.viewport_width as f32
        {
            continue;
        }
        assets.push(Ui3AssetPlacement {
            x: floor_i32(x),
            y: floor_i32(adjusted_y),
            width: resolved_width,
            height: resolved_height,
            asset,
        });
    }
}

#[allow(dead_code)]
fn draw_ready_assets_primary(assets: &[Ui3AssetPlacement]) -> usize {
    let mut drawn = 0usize;
    for placement in assets {
        if placement.asset.width == 0
            || placement.asset.height == 0
            || placement.asset.rgba.is_empty()
        {
            continue;
        }
        if crate::intel::blend_rgba_primary_rect_scaled(
            placement.asset.rgba.as_slice(),
            placement.asset.width,
            placement.asset.height,
            placement.asset.width as usize * 4,
            0,
            0,
            placement.asset.width,
            placement.asset.height,
            placement.x,
            placement.y,
            placement.width,
            placement.height,
            "ui3-asset-ready-primary",
        ) {
            drawn = drawn.saturating_add(1);
        }
    }
    drawn
}

fn resolved_asset_size(
    asset: &crate::surfer::asset_shack::BrowserAssetReady,
    x: f32,
    layout_width: u32,
    layout_height: u32,
    explicit_width: bool,
    explicit_height: bool,
) -> (u32, u32) {
    let intrinsic_width = asset.width.max(1);
    let intrinsic_height = asset.height.max(1);
    if explicit_width && explicit_height {
        return (layout_width.max(1), layout_height.max(1));
    }

    let aspect_height_for_width = |width: u32| -> u32 {
        (((width as u64)
            .saturating_mul(intrinsic_height as u64)
            .saturating_add((intrinsic_width / 2) as u64))
            / intrinsic_width as u64)
            .clamp(1, u32::MAX as u64) as u32
    };
    let aspect_width_for_height = |height: u32| -> u32 {
        (((height as u64)
            .saturating_mul(intrinsic_width as u64)
            .saturating_add((intrinsic_height / 2) as u64))
            / intrinsic_height as u64)
            .clamp(1, u32::MAX as u64) as u32
    };

    let (mut width, mut height) = if explicit_width {
        let width = layout_width.max(1);
        (width, aspect_height_for_width(width))
    } else if explicit_height {
        let height = layout_height.max(1);
        (aspect_width_for_height(height), height)
    } else {
        (intrinsic_width, intrinsic_height)
    };

    let max_width = crate::intel::active_scanout_dimensions()
        .map(|(scanout_width, _)| {
            if x < 0.0 {
                scanout_width
            } else {
                scanout_width.saturating_sub(floor_i32(x).max(0) as u32)
            }
        })
        .unwrap_or(layout_width)
        .max(1);
    if width > max_width {
        width = max_width;
        height = aspect_height_for_width(width);
    }

    (width.max(1), height.max(1))
}

fn image_box_has_explicit_size(node: &Value, axis: &str) -> bool {
    let Some(attrs) = node.get("attrs").and_then(Value::as_object) else {
        return false;
    };
    let fallback_axis = if axis == "width" { "w" } else { "h" };
    attrs
        .get(axis)
        .or_else(|| attrs.get(fallback_axis))
        .is_some_and(|value| match value {
            Value::String(s) => !s.trim().is_empty(),
            Value::Number(_) => true,
            _ => false,
        })
}

fn layout_shift_for_y(y: f32, adjustments: &[Ui3LayoutAdjustment]) -> f32 {
    adjustments
        .iter()
        .filter(|adjustment| y >= adjustment.after_y)
        .map(|adjustment| adjustment.delta_y)
        .sum()
}

fn layout_adjustment_total_px(adjustments: &[Ui3LayoutAdjustment]) -> u32 {
    let total: f32 = adjustments
        .iter()
        .map(|adjustment| adjustment.delta_y)
        .sum();
    if !total.is_finite() || total <= 0.0 {
        return 0;
    }
    libm::ceilf(total).min(u32::MAX as f32) as u32
}

fn log_collect_caps(
    scratch: &Ui3FontScratch,
    collect: Ui3TextCollectStats,
    gradient_collect: Ui3GradientCollectStats,
    gradient_count: usize,
    scene: Ui3FontScene,
) {
    if gradient_collect.painted_box_cap_hit {
        crate::log_warn!(
            target: "ui3";
            "ui3-font: painted-box cap reached boxes={} cap={} gradients={} scroll_y={} viewport={}x{}\n",
            gradient_collect.painted_boxes,
            UI3_PAINTED_BOX_MAX,
            gradient_count,
            scene.scroll_y as u32,
            scene.viewport_width,
            scene.viewport_height
        );
    }
    if gradient_collect.gradient_desc_cap_hit {
        crate::log_warn!(
            target: "ui3";
            "ui3-font: gradient-desc cap reached gradients={} cap={} boxes={} scroll_y={} viewport={}x{}\n",
            gradient_count,
            UI3_GRADIENT_DESC_MAX,
            gradient_collect.painted_boxes,
            scene.scroll_y as u32,
            scene.viewport_width,
            scene.viewport_height
        );
    }
    if collect.text_node_cap_hit {
        crate::log_warn!(
            target: "ui3";
            "ui3-font: text-node cap reached text_nodes={} cap={} placements={} scroll_y={} viewport={}x{}\n",
            collect.text_nodes,
            UI3_LAYOUT_TEXT_NODE_MAX,
            scratch.placements.len(),
            scene.scroll_y as u32,
            scene.viewport_width,
            scene.viewport_height
        );
    }
    if collect.placement_cap_hit {
        crate::log_warn!(
            target: "ui3";
            "ui3-font: placement cap reached placements={} cap={} text_nodes={} scroll_y={} viewport={}x{}\n",
            scratch.placements.len(),
            UI3_TEXT_PLACEMENT_MAX,
            collect.text_nodes,
            scene.scroll_y as u32,
            scene.viewport_width,
            scene.viewport_height
        );
    }
}

fn collect_paint_plan_rect_gradients(
    plan: &Value,
    scroll_y: f32,
    scene: Ui3FontScene,
    adjustments: &[Ui3LayoutAdjustment],
    gradients: &mut Vec<crate::intel::gpgpu::GpgpuGradientRect>,
    control_gradients: &mut Vec<crate::intel::gpgpu::GpgpuGradientRect>,
    stats: &mut Ui3GradientCollectStats,
) {
    let Some(boxes) = plan.get("paintedBoxes").and_then(Value::as_array) else {
        return;
    };
    for item in boxes {
        if stats.painted_boxes >= UI3_PAINTED_BOX_MAX {
            stats.painted_box_cap_hit = true;
            break;
        }
        if paint_plan_gradient_count(gradients, control_gradients) >= UI3_GRADIENT_DESC_MAX {
            stats.gradient_desc_cap_hit = true;
            break;
        }
        let role = item.get("role").and_then(Value::as_str).unwrap_or("");
        let remaining = UI3_GRADIENT_DESC_MAX
            .saturating_sub(paint_plan_gradient_count(gradients, control_gradients));
        let y_doc = json_f32_field(item, "y").unwrap_or(0.0);
        let adjusted_y = y_doc + layout_shift_for_y(y_doc, adjustments) - scroll_y;
        let pushed = match role {
            "dialog" => push_painted_box_gradients(
                json_f32_field(item, "x").unwrap_or(0.0),
                adjusted_y,
                item,
                scene,
                remaining,
                gradients,
            ),
            "button" | "iframe" | "image" | "link" | "rule" | "table" | "table-cell" => {
                push_painted_box_gradients(
                    json_f32_field(item, "x").unwrap_or(0.0),
                    adjusted_y,
                    item,
                    scene,
                    remaining,
                    control_gradients,
                )
            }
            _ => 0,
        };
        if pushed != 0 {
            stats.painted_boxes = stats.painted_boxes.saturating_add(1);
        }
    }
}

fn collect_paint_plan_summary_icons(
    plan: &Value,
    scroll_y: f32,
    scene: Ui3FontScene,
    adjustments: &[Ui3LayoutAdjustment],
    placements: &mut Vec<crate::intel::gpgpu::GpgpuSprite64Placement>,
    stats: &mut Ui3TextCollectStats,
) {
    let Some(icons) = plan.get("summaryIcons").and_then(Value::as_array) else {
        return;
    };
    for icon in icons {
        if placements.len() >= UI3_TEXT_PLACEMENT_MAX {
            stats.placement_cap_hit = true;
            break;
        }
        let y_doc = json_f32_field(icon, "y").unwrap_or(0.0);
        push_summary_icon_placement_resolved(
            json_f32_field(icon, "x").unwrap_or(0.0),
            y_doc + layout_shift_for_y(y_doc, adjustments) - scroll_y,
            json_f32_field(icon, "height").unwrap_or(0.0),
            json_bool_field(icon, "open").unwrap_or(false),
            scene,
            placements,
            stats,
        );
    }
}

fn collect_paint_plan_text_placements(
    plan: &Value,
    scroll_y: f32,
    scene: Ui3FontScene,
    adjustments: &[Ui3LayoutAdjustment],
    placements: &mut Vec<crate::intel::gpgpu::GpgpuSprite64Placement>,
    stats: &mut Ui3TextCollectStats,
) {
    let Some(text_runs) = plan.get("textRuns").and_then(Value::as_array) else {
        return;
    };
    for run in text_runs {
        if stats.text_nodes >= UI3_LAYOUT_TEXT_NODE_MAX
            || placements.len() >= UI3_TEXT_PLACEMENT_MAX
        {
            if stats.text_nodes >= UI3_LAYOUT_TEXT_NODE_MAX {
                stats.text_node_cap_hit = true;
            }
            if placements.len() >= UI3_TEXT_PLACEMENT_MAX {
                stats.placement_cap_hit = true;
            }
            break;
        }
        let Some(text) = run.get("text").and_then(Value::as_str) else {
            continue;
        };
        if text.is_empty() {
            continue;
        }
        stats.text_nodes = stats.text_nodes.saturating_add(1);
        let text_color = json_rgb24_field(run, "textColor")
            .map(rgb24_to_sprite_tint_word)
            .unwrap_or(UI3_TEXT_COLOR_RGBA);
        let preserve_whitespace = paint_plan_preserve_whitespace(run);
        let face = paint_plan_font_face(run);
        let x = json_f32_field(run, "x").unwrap_or(0.0);
        let y_doc = json_f32_field(run, "y").unwrap_or(0.0);
        let y = y_doc + layout_shift_for_y(y_doc, adjustments) - scroll_y;
        if let Some(lines) = run.get("lines").and_then(Value::as_array) {
            let line_count = lines.len().max(1);
            let layout_line_height = json_f32_field(run, "height")
                .map(|height| height / line_count as f32)
                .filter(|height| height.is_finite() && *height > 0.0);
            push_layout_text_lines(
                lines,
                x,
                y,
                layout_line_height,
                scene,
                text_color,
                face,
                preserve_whitespace,
                placements,
                stats,
            );
        } else {
            push_text_placements(
                text,
                x,
                y,
                None,
                scene,
                text_color,
                face,
                preserve_whitespace,
                placements,
                stats,
            );
        }
    }
}

fn paint_plan_gradient_count(
    gradients: &[crate::intel::gpgpu::GpgpuGradientRect],
    control_gradients: &[crate::intel::gpgpu::GpgpuGradientRect],
) -> usize {
    gradients.len().saturating_add(control_gradients.len())
}

fn push_painted_box_gradients(
    x: f32,
    y: f32,
    node: &Value,
    scene: Ui3FontScene,
    remaining: usize,
    gradients: &mut Vec<crate::intel::gpgpu::GpgpuGradientRect>,
) -> usize {
    if remaining == 0 {
        return 0;
    }
    let Some(width) = json_u32_field(node, "width") else {
        return 0;
    };
    let Some(height) = json_u32_field(node, "height") else {
        return 0;
    };
    if width == 0 || height == 0 || y + height as f32 <= 0.0 || y >= scene.viewport_height as f32 {
        return 0;
    }
    if x + width as f32 <= 0.0 || x >= scene.viewport_width as f32 {
        return 0;
    }
    let Some(paint) = node.get("paint") else {
        return 0;
    };
    let border_width = json_u32_field(paint, "borderWidth")
        .unwrap_or(0)
        .min(width / 2)
        .min(height / 2);

    let mut pushed = 0usize;
    if border_width > 0 {
        let Some(border_color) = json_rgb24_field(paint, "borderColor").map(rgb24_to_rgba8_word)
        else {
            return 0;
        };
        pushed = pushed.saturating_add(push_solid_gradient_rect(
            gradients,
            remaining.saturating_sub(pushed),
            floor_i32(x),
            floor_i32(y),
            width,
            border_width,
            border_color,
        ));
        pushed = pushed.saturating_add(push_solid_gradient_rect(
            gradients,
            remaining.saturating_sub(pushed),
            floor_i32(x),
            floor_i32(y + (height - border_width) as f32),
            width,
            border_width,
            border_color,
        ));
        pushed = pushed.saturating_add(push_solid_gradient_rect(
            gradients,
            remaining.saturating_sub(pushed),
            floor_i32(x),
            floor_i32(y + border_width as f32),
            border_width,
            height.saturating_sub(border_width.saturating_mul(2)),
            border_color,
        ));
        pushed = pushed.saturating_add(push_solid_gradient_rect(
            gradients,
            remaining.saturating_sub(pushed),
            floor_i32(x + (width - border_width) as f32),
            floor_i32(y + border_width as f32),
            border_width,
            height.saturating_sub(border_width.saturating_mul(2)),
            border_color,
        ));
    }

    let Some(color0) = json_rgba8_field(paint, "color0Rgba")
        .or_else(|| json_rgb24_field(paint, "color0").map(rgb24_to_rgba8_word))
    else {
        return pushed;
    };
    let color1 = json_rgba8_field(paint, "color1Rgba")
        .or_else(|| json_rgb24_field(paint, "color1").map(rgb24_to_rgba8_word))
        .unwrap_or(color0);
    let fill_x = x + border_width as f32;
    let fill_y = y + border_width as f32;
    let fill_width = width.saturating_sub(border_width.saturating_mul(2));
    let fill_height = height.saturating_sub(border_width.saturating_mul(2));
    if fill_width == 0 || fill_height == 0 || pushed >= remaining {
        return pushed;
    }
    gradients.push(crate::intel::gpgpu::GpgpuGradientRect {
        rect: crate::intel::gpgpu::GpgpuRect::new(
            floor_i32(fill_x),
            floor_i32(fill_y),
            fill_width,
            fill_height,
        ),
        color0_rgba: color0,
        color1_rgba: color1,
        vertical: false,
    });
    pushed.saturating_add(1)
}

fn push_summary_icon_placement_resolved(
    x: f32,
    y: f32,
    height: f32,
    open: bool,
    scene: Ui3FontScene,
    placements: &mut Vec<crate::intel::gpgpu::GpgpuSprite64Placement>,
    stats: &mut Ui3TextCollectStats,
) {
    if placements.len() >= UI3_TEXT_PLACEMENT_MAX {
        stats.placement_cap_hit = true;
        return;
    }
    let Some(slot) = summary_icon_slot(open) else {
        return;
    };
    let cell = crate::intel::gpgpu::SPRITE64_WORKLIST_CELL_PIXELS as f32;
    let y_offset = if height > cell {
        (height - cell) * 0.5
    } else {
        0.0
    };
    let dst_x = floor_i32(x + UI3_SUMMARY_ICON_X_PAD);
    let dst_y = floor_i32(y + y_offset);

    let fallback_max_x = scene
        .viewport_width
        .saturating_sub(crate::intel::gpgpu::SPRITE64_WORKLIST_CELL_PIXELS)
        as i32;
    let fallback_max_y = scene
        .viewport_height
        .saturating_sub(crate::intel::gpgpu::SPRITE64_WORKLIST_CELL_PIXELS)
        as i32;
    let (max_draw_x, max_draw_y) = (fallback_max_x, fallback_max_y);
    if dst_x < 0 || dst_x > max_draw_x || dst_y < 0 || dst_y > max_draw_y {
        stats.clipped = stats.clipped.saturating_add(1);
        return;
    }

    placements.push(crate::intel::gpgpu::GpgpuSprite64Placement::src_over(slot, dst_x, dst_y));
}

fn summary_icon_slot(open: bool) -> Option<u16> {
    let primary = if open {
        UI3_SUMMARY_ICON_OPEN
    } else {
        UI3_SUMMARY_ICON_CLOSED
    };
    crate::ui3::althlasfont::twemoji::twemoji_lookup_glyph_region(primary)
        .or_else(|| {
            crate::ui3::althlasfont::twemoji::twemoji_lookup_glyph_region(UI3_SUMMARY_ICON_CLOSED)
        })
        .map(|region| region.slot)
}

fn push_solid_gradient_rect(
    gradients: &mut Vec<crate::intel::gpgpu::GpgpuGradientRect>,
    remaining: usize,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    color_rgba: u32,
) -> usize {
    if remaining == 0 || width == 0 || height == 0 {
        return 0;
    }
    gradients.push(crate::intel::gpgpu::GpgpuGradientRect {
        rect: crate::intel::gpgpu::GpgpuRect::new(x, y, width, height),
        color0_rgba: color_rgba,
        color1_rgba: color_rgba,
        vertical: false,
    });
    1
}

fn push_text_placements(
    text: &str,
    x: f32,
    y: f32,
    line_advance: Option<f32>,
    scene: Ui3FontScene,
    color_rgba: u32,
    face: crate::ui3::althlasfont::bitmapfont::AthlasFontFace,
    preserve_whitespace: bool,
    placements: &mut Vec<crate::intel::gpgpu::GpgpuSprite64Placement>,
    stats: &mut Ui3TextCollectStats,
) {
    let font_line_height =
        crate::ui3::althlasfont::bitmapfont::athlas_font_line_height_px(face).unwrap_or(22) as f32;
    let line_advance = line_advance
        .filter(|height| height.is_finite() && *height > 0.0)
        .unwrap_or(font_line_height);
    for (line_index, line) in text.split('\n').enumerate() {
        push_text_line_placements(
            line,
            x,
            y + line_advance * line_index as f32,
            font_line_height,
            scene,
            color_rgba,
            face,
            preserve_whitespace,
            placements,
            stats,
        );
        if placements.len() >= UI3_TEXT_PLACEMENT_MAX {
            break;
        }
    }
}

fn push_layout_text_lines(
    lines: &[Value],
    x: f32,
    y: f32,
    line_advance: Option<f32>,
    scene: Ui3FontScene,
    color_rgba: u32,
    face: crate::ui3::althlasfont::bitmapfont::AthlasFontFace,
    preserve_whitespace: bool,
    placements: &mut Vec<crate::intel::gpgpu::GpgpuSprite64Placement>,
    stats: &mut Ui3TextCollectStats,
) {
    let font_line_height =
        crate::ui3::althlasfont::bitmapfont::athlas_font_line_height_px(face).unwrap_or(22) as f32;
    let line_advance = line_advance
        .filter(|height| height.is_finite() && *height > 0.0)
        .unwrap_or(font_line_height);
    for (line_index, line) in lines.iter().enumerate() {
        let Some(text) = line.as_str() else {
            continue;
        };
        push_text_line_placements(
            text,
            x,
            y + line_advance * line_index as f32,
            font_line_height,
            scene,
            color_rgba,
            face,
            preserve_whitespace,
            placements,
            stats,
        );
        if placements.len() >= UI3_TEXT_PLACEMENT_MAX {
            break;
        }
    }
}

fn push_text_line_placements(
    text: &str,
    x: f32,
    y: f32,
    line_height: f32,
    scene: Ui3FontScene,
    color_rgba: u32,
    face: crate::ui3::althlasfont::bitmapfont::AthlasFontFace,
    preserve_whitespace: bool,
    placements: &mut Vec<crate::intel::gpgpu::GpgpuSprite64Placement>,
    stats: &mut Ui3TextCollectStats,
) {
    let preserved_space_advance = if preserve_whitespace {
        preserved_text_space_advance(face, line_height)
    } else {
        line_height * 0.35
    };
    let fallback_max_x = scene
        .viewport_width
        .saturating_sub(crate::intel::gpgpu::SPRITE64_WORKLIST_CELL_PIXELS)
        as i32;
    let fallback_max_y = scene
        .viewport_height
        .saturating_sub(crate::intel::gpgpu::SPRITE64_WORKLIST_CELL_PIXELS)
        as i32;
    let (max_draw_x, max_draw_y) = (fallback_max_x, fallback_max_y);
    let dst_y = floor_i32(y);
    if dst_y < 0 || dst_y > max_draw_y {
        stats.clipped = stats.clipped.saturating_add(text.chars().count());
        return;
    }

    let mut pen_x = x;
    for ch in text.chars() {
        if placements.len() >= UI3_TEXT_PLACEMENT_MAX {
            stats.placement_cap_hit = true;
            break;
        }
        if ch.is_control() {
            continue;
        }
        if ch.is_whitespace() {
            pen_x += if preserve_whitespace && ch == '\t' {
                preserved_space_advance * 4.0
            } else {
                preserved_space_advance
            };
            continue;
        }
        let Some(region) =
            crate::ui3::althlasfont::bitmapfont::athlas_lookup_glyph_region(face, ch)
        else {
            pen_x += line_height * 0.35;
            continue;
        };
        let advance =
            f32::from(crate::ui3::althlasfont::bitmapfont::athlas_glyph_advance_px(region));
        let dst_x = floor_i32(pen_x);
        if dst_x < 0 || dst_x > max_draw_x {
            stats.clipped = stats.clipped.saturating_add(1);
            pen_x += advance;
            continue;
        }
        let Some(slot) = crate::intel::gpgpu::sprite64_font_slot_for_region(face, region) else {
            pen_x += advance;
            continue;
        };
        placements.push(crate::intel::gpgpu::GpgpuSprite64Placement::tinted_src_over(
            slot, dst_x, dst_y, color_rgba,
        ));
        pen_x += advance;
    }
}

fn preserved_text_space_advance(
    face: crate::ui3::althlasfont::bitmapfont::AthlasFontFace,
    line_height: f32,
) -> f32 {
    crate::ui3::althlasfont::bitmapfont::athlas_lookup_glyph_region(face, '█')
        .or_else(|| crate::ui3::althlasfont::bitmapfont::athlas_lookup_glyph_region(face, 'M'))
        .map(|region| f32::from(region.src_w.max(1)))
        .unwrap_or(line_height * 0.58)
}

fn paint_plan_preserve_whitespace(node: &Value) -> bool {
    node.get("preserveWhitespace")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn paint_plan_font_face(node: &Value) -> crate::ui3::althlasfont::bitmapfont::AthlasFontFace {
    use crate::ui3::althlasfont::bitmapfont::{
        ATHLAS_FONT_FACE_LUCIDA_1X, ATHLAS_FONT_FACE_LUCIDA_HALF, ATHLAS_FONT_FACE_LUCIDA_THIRD,
    };

    let tier = node
        .get("fontRenderTier")
        .and_then(Value::as_str)
        .or_else(|| node.get("fontTier").and_then(Value::as_str))
        .unwrap_or("");

    match tier {
        "third" => ATHLAS_FONT_FACE_LUCIDA_THIRD,
        "half" => ATHLAS_FONT_FACE_LUCIDA_HALF,
        "1x" => ATHLAS_FONT_FACE_LUCIDA_1X,
        // The render-tree contract can already ask for 2x. The sprite64 atlas
        // currently tops out at 1x, so clamp here until the 2x face is wired.
        "2x" => ATHLAS_FONT_FACE_LUCIDA_1X,
        _ => {
            let font_size = json_f32_field(node, "fontSizePx").unwrap_or(15.0);
            if font_size <= 10.0 {
                ATHLAS_FONT_FACE_LUCIDA_THIRD
            } else if font_size <= 15.0 {
                ATHLAS_FONT_FACE_LUCIDA_HALF
            } else {
                ATHLAS_FONT_FACE_LUCIDA_1X
            }
        }
    }
}

fn json_f32_field(node: &Value, key: &str) -> Option<f32> {
    let number = node.get(key)?.as_f64()?;
    if number.is_finite() {
        Some(number as f32)
    } else {
        None
    }
}

fn json_u32_field(node: &Value, key: &str) -> Option<u32> {
    let number = node.get(key)?.as_u64()?;
    u32::try_from(number).ok()
}

fn json_bool_field(node: &Value, key: &str) -> Option<bool> {
    node.get(key)?.as_bool()
}

fn json_rgb24_field(node: &Value, key: &str) -> Option<u32> {
    json_u32_field(node, key).map(|rgb| rgb & 0x00FF_FFFF)
}

fn json_rgba8_field(node: &Value, key: &str) -> Option<u32> {
    json_u32_field(node, key)
}

fn rgb24_to_rgba8_word(rgb: u32) -> u32 {
    0xFF00_0000 | (rgb & 0x00FF_FFFF)
}

fn rgb24_to_sprite_tint_word(rgb: u32) -> u32 {
    let r = (rgb >> 16) & 0xFF;
    let g = (rgb >> 8) & 0xFF;
    let b = rgb & 0xFF;
    0xFF00_0000 | (b << 16) | (g << 8) | r
}

fn floor_i32(value: f32) -> i32 {
    if !value.is_finite() {
        return 0;
    }
    libm::floorf(value).clamp(i32::MIN as f32, i32::MAX as f32) as i32
}

fn elapsed_ms_since(start: u64) -> u64 {
    let now = embassy_time_driver::now();
    let ticks = now.saturating_sub(start);
    let hz = embassy_time_driver::TICK_HZ;
    if hz == 0 {
        0
    } else {
        ticks.saturating_mul(1000) / hz
    }
}
