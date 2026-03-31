const UI2_ATHLAS_GRID_DEMO_WINDOW_X: f32 = 560.0;
const UI2_ATHLAS_GRID_DEMO_WINDOW_Y: f32 = 96.0;
const UI2_ATHLAS_GRID_DEMO_WINDOW_Z: i16 = 32;
const UI2_ATHLAS_GRID_DEMO_WINDOW_X_GAP: f32 = 28.0;
const UI2_ATHLAS_GRID_DEMO_CONTENT_ID_BASE: u32 = 40;
const UI2_ATHLAS_GRID_DEMO_TILE_TEX_ID_BASE: u32 = 4_800;
const UI2_ATHLAS_GRID_DEMO_BG_RGBA: [u8; 4] = [0xFF, 0xFF, 0xFF, 0xFF];
const UI2_ATHLAS_GRID_DEMO_FG_RGBA: [u8; 4] = [0x16, 0x18, 0x1E, 0xFF];
const UI2_ATHLAS_GRID_DEMO_ALT_BG_RGBA: [u8; 4] = [0x08, 0x09, 0x0C, 0xFF];
const UI2_ATHLAS_GRID_DEMO_ALT_FG_RGBA: [u8; 4] = [0xF4, 0xF6, 0xFA, 0xFF];
const UI2_ATHLAS_GRID_DEMO_DEFER_MS: u64 = 16;
const UI2_ATHLAS_GRID_DEMO_GAP_PX: u32 = 12;
const UI2_ATHLAS_GRID_DEMO_VIEWPORT_H: u32 = 240;

fn demo_title(size_case: usize) -> &'static str {
    match size_case {
        0 => "Athlas Buckets half",
        1 => "Athlas Buckets 1x",
        2 => "Athlas Buckets 3x",
        _ => "Athlas Buckets",
    }
}

fn demo_colors(size_case: usize) -> ([u8; 4], [u8; 4]) {
    if size_case == 0 {
        (UI2_ATHLAS_GRID_DEMO_BG_RGBA, UI2_ATHLAS_GRID_DEMO_FG_RGBA)
    } else {
        (UI2_ATHLAS_GRID_DEMO_ALT_BG_RGBA, UI2_ATHLAS_GRID_DEMO_ALT_FG_RGBA)
    }
}

fn demo_content_id(size_case: usize) -> u32 {
    UI2_ATHLAS_GRID_DEMO_CONTENT_ID_BASE.saturating_add(size_case as u32)
}

fn demo_tile_tex_id(size_case: usize, bucket: usize) -> u32 {
    UI2_ATHLAS_GRID_DEMO_TILE_TEX_ID_BASE
        .saturating_add(
            (size_case as u32)
                .saturating_mul(crate::gfx::imba_athlas::athlasmetrics::ATHLAS_BUCKET_COUNT as u32),
        )
        .saturating_add(bucket as u32)
}

fn bucket_origin_left_aligned(bucket_dims: &[(u32, u32)], bucket: usize) -> (u32, u32) {
    let mut y = UI2_ATHLAS_GRID_DEMO_GAP_PX;
    for (idx, (_, bucket_h)) in bucket_dims.iter().copied().enumerate() {
        if idx == bucket {
            return (UI2_ATHLAS_GRID_DEMO_GAP_PX, y);
        }
        y = y
            .saturating_add(bucket_h)
            .saturating_add(UI2_ATHLAS_GRID_DEMO_GAP_PX);
    }
    (UI2_ATHLAS_GRID_DEMO_GAP_PX, UI2_ATHLAS_GRID_DEMO_GAP_PX)
}

async fn spawn_athlas_bucket_window(size_case: usize, window_x: f32) -> Option<f32> {
    let mut bucket_dims =
        [(0u32, 0u32); crate::gfx::imba_athlas::athlasmetrics::ATHLAS_BUCKET_COUNT];
    let mut content_w = UI2_ATHLAS_GRID_DEMO_GAP_PX * 2;
    let mut content_h = UI2_ATHLAS_GRID_DEMO_GAP_PX;

    for bucket in 0..crate::gfx::imba_athlas::athlasmetrics::ATHLAS_BUCKET_COUNT {
        let Some((bucket_w, bucket_h)) =
            crate::gfx::imba_athlas::imba_athlas_bucket_png_dimensions(size_case, bucket)
        else {
            crate::log!(
                "ui2-athlas-grid-demo: missing bucket dimensions size_case={} bucket={}\n",
                size_case,
                bucket,
            );
            return None;
        };
        bucket_dims[bucket] = (bucket_w, bucket_h);
        content_w = content_w.max(bucket_w.saturating_add(UI2_ATHLAS_GRID_DEMO_GAP_PX * 2));
        content_h = content_h
            .saturating_add(bucket_h)
            .saturating_add(UI2_ATHLAS_GRID_DEMO_GAP_PX);
    }

    let (bg_rgba, fg_rgba) = demo_colors(size_case);
    let viewport_w = content_w;
    let viewport_h = content_h.min(UI2_ATHLAS_GRID_DEMO_VIEWPORT_H.max(1));
    let content_id = demo_content_id(size_case);
    let Some(surface) = crate::r::ui2::Ui2SurfaceWindow::from_tiled_content(
        demo_title(size_case),
        crate::r::ui2::Ui2Rect {
            x: window_x,
            y: UI2_ATHLAS_GRID_DEMO_WINDOW_Y,
            w: viewport_w as f32,
            h: viewport_h as f32,
        },
        UI2_ATHLAS_GRID_DEMO_WINDOW_Z,
        220,
        bg_rgba,
    ) else {
        crate::log!("ui2-athlas-grid-demo: window creation failed size_case={}\n", size_case);
        return None;
    };

    if !surface.bind_hosted_scroll_state(content_id, content_w, content_h) {
        crate::log!(
            "ui2-athlas-grid-demo: hosted scroll bind failed window={} content_id={} size_case={}\n",
            surface.window_id(),
            content_id,
            size_case
        );
        return None;
    }

    crate::log!(
        "ui2-athlas-grid-demo: placeholder window={} content_id={} size_case={} viewport={}x{} content={}x{} buckets={}\n",
        surface.window_id(),
        content_id,
        size_case,
        viewport_w,
        viewport_h,
        content_w,
        content_h,
        crate::gfx::imba_athlas::athlasmetrics::ATHLAS_BUCKET_COUNT,
    );

    let _ = crate::r::ui2::minimize_window(surface.window_id());

    let tiles = bucket_dims
        .iter()
        .copied()
        .enumerate()
        .map(|(bucket, (bucket_w, bucket_h))| {
            let (upload_x, upload_y) = bucket_origin_left_aligned(&bucket_dims, bucket);
            crate::r::ui2::Ui2HostedSurfaceTile {
                tex_id: demo_tile_tex_id(size_case, bucket),
                x: upload_x,
                y: upload_y,
                width: bucket_w,
                height: bucket_h,
                blend_enabled: false,
            }
        })
        .collect::<alloc::vec::Vec<_>>();
    if !surface.set_tiles(bg_rgba, tiles.as_slice()) {
        crate::log!(
            "ui2-athlas-grid-demo: tile registration failed window={} size_case={}\n",
            surface.window_id(),
            size_case
        );
        return None;
    }

    embassy_time::Timer::after(embassy_time::Duration::from_millis(UI2_ATHLAS_GRID_DEMO_DEFER_MS))
        .await;

    for (bucket, (bucket_w, bucket_h)) in bucket_dims.iter().copied().enumerate() {
        let Some((upload_w, upload_h, rgba)) =
            crate::gfx::imba_athlas::imba_athlas_bucket_surface_rgba(
                size_case, bucket, bg_rgba, fg_rgba,
            )
        else {
            crate::log!(
                "ui2-athlas-grid-demo: bucket surface build failed size_case={} bucket={}\n",
                size_case,
                bucket,
            );
            return None;
        };

        if upload_w != bucket_w || upload_h != bucket_h {
            crate::log!(
                "ui2-athlas-grid-demo: bucket size changed size_case={} bucket={} expected={}x{} got={}x{}\n",
                size_case,
                bucket,
                bucket_w,
                bucket_h,
                upload_w,
                upload_h
            );
            return None;
        }

        let tile_tex_id = demo_tile_tex_id(size_case, bucket);
        let repaint_window_id = if crate::r::ui2::is_window_minimized(surface.window_id()) {
            0
        } else {
            surface.window_id()
        };
        if !crate::r::io::cabi::queue_texture_rgba_image_upload_copy(
            tile_tex_id,
            bucket_w,
            bucket_h,
            rgba.as_slice(),
            repaint_window_id,
            "ui2-athlas-grid-demo-upload-region",
        ) {
            crate::log!(
                "ui2-athlas-grid-demo: bucket upload failed window={} tex={} size_case={} bucket={} size={}x{}\n",
                surface.window_id(),
                tile_tex_id,
                size_case,
                bucket,
                bucket_w,
                bucket_h
            );
            return None;
        }
    }

    crate::log!(
        "ui2-athlas-grid-demo: window={} deferred_bucket_column size_case={} viewport={}x{} content={}x{} buckets={}\n",
        surface.window_id(),
        size_case,
        viewport_w,
        viewport_h,
        content_w,
        content_h,
        crate::gfx::imba_athlas::athlasmetrics::ATHLAS_BUCKET_COUNT
    );

    Some(window_x + viewport_w as f32 + UI2_ATHLAS_GRID_DEMO_WINDOW_X_GAP)
}

#[embassy_executor::task]
pub async fn ui2_athlas_grid_demo_task() {
    let mut next_window_x = UI2_ATHLAS_GRID_DEMO_WINDOW_X;
    for size_case in 0..crate::gfx::imba_athlas::athlasmetrics::ATHLAS_VARIANT_JSONS.len() {
        let Some(next_x) = spawn_athlas_bucket_window(size_case, next_window_x).await else {
            return;
        };
        next_window_x = next_x;
    }

    loop {
        embassy_time::Timer::after(embassy_time::Duration::from_secs(3600)).await;
    }
}
