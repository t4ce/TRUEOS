const UI2_ATHLAS_GRID_DEMO_TEX_ID: u32 = 4_708;
const UI2_ATHLAS_GRID_DEMO_WINDOW_X: f32 = 560.0;
const UI2_ATHLAS_GRID_DEMO_WINDOW_Y: f32 = 96.0;
const UI2_ATHLAS_GRID_DEMO_WINDOW_Z: i16 = 32;
const UI2_ATHLAS_GRID_DEMO_SIZE_CASE: usize = 0;
const UI2_ATHLAS_GRID_DEMO_BG_RGBA: [u8; 4] = [0xFF, 0xFF, 0xFF, 0xFF];
const UI2_ATHLAS_GRID_DEMO_FG_RGBA: [u8; 4] = [0x16, 0x18, 0x1E, 0xFF];
const UI2_ATHLAS_GRID_DEMO_DEFER_MS: u64 = 16;
const UI2_ATHLAS_GRID_DEMO_COLUMNS: usize = 4;
const UI2_ATHLAS_GRID_DEMO_GAP_PX: u32 = 12;

fn bucket_cell_origin(bucket: usize, cell_w: u32, cell_h: u32) -> (u32, u32) {
    let col = bucket % UI2_ATHLAS_GRID_DEMO_COLUMNS;
    let row = bucket / UI2_ATHLAS_GRID_DEMO_COLUMNS;
    (
        UI2_ATHLAS_GRID_DEMO_GAP_PX + (col as u32) * (cell_w + UI2_ATHLAS_GRID_DEMO_GAP_PX),
        UI2_ATHLAS_GRID_DEMO_GAP_PX + (row as u32) * (cell_h + UI2_ATHLAS_GRID_DEMO_GAP_PX),
    )
}

#[embassy_executor::task]
pub async fn ui2_athlas_grid_demo_task() {
    let mut bucket_dims =
        [(0u32, 0u32); crate::gfx::imba_athlas::athlasmetrics::ATHLAS_BUCKET_COUNT];
    let mut cell_w = 0u32;
    let mut cell_h = 0u32;

    for bucket in 0..crate::gfx::imba_athlas::athlasmetrics::ATHLAS_BUCKET_COUNT {
        let Some((bucket_w, bucket_h)) = crate::gfx::imba_athlas::imba_athlas_bucket_png_dimensions(
            UI2_ATHLAS_GRID_DEMO_SIZE_CASE,
            bucket,
        ) else {
            crate::log!(
                "ui2-athlas-grid-demo: missing bucket dimensions size_case={} bucket={}\n",
                UI2_ATHLAS_GRID_DEMO_SIZE_CASE,
                bucket,
            );
            return;
        };
        bucket_dims[bucket] = (bucket_w, bucket_h);
        cell_w = cell_w.max(bucket_w);
        cell_h = cell_h.max(bucket_h);
    }

    let rows = crate::gfx::imba_athlas::athlasmetrics::ATHLAS_BUCKET_COUNT
        .div_ceil(UI2_ATHLAS_GRID_DEMO_COLUMNS);
    let width = UI2_ATHLAS_GRID_DEMO_GAP_PX
        + (UI2_ATHLAS_GRID_DEMO_COLUMNS as u32) * (cell_w + UI2_ATHLAS_GRID_DEMO_GAP_PX);
    let height =
        UI2_ATHLAS_GRID_DEMO_GAP_PX + (rows as u32) * (cell_h + UI2_ATHLAS_GRID_DEMO_GAP_PX);

    let Some(surface) = crate::r::ui2::Ui2SurfaceWindow::new(
        "Athlas Buckets",
        crate::r::ui2::Ui2Rect {
            x: UI2_ATHLAS_GRID_DEMO_WINDOW_X,
            y: UI2_ATHLAS_GRID_DEMO_WINDOW_Y,
            w: width as f32,
            h: height as f32,
        },
        UI2_ATHLAS_GRID_DEMO_WINDOW_Z,
        220,
        UI2_ATHLAS_GRID_DEMO_TEX_ID,
        false,
        UI2_ATHLAS_GRID_DEMO_BG_RGBA,
    ) else {
        crate::log!(
            "ui2-athlas-grid-demo: window creation failed tex={}\n",
            UI2_ATHLAS_GRID_DEMO_TEX_ID
        );
        return;
    };

    crate::log!(
        "ui2-athlas-grid-demo: placeholder window={} tex={} size_case={} size={}x{} buckets={} cell={}x{}\n",
        surface.window_id(),
        surface.tex_id(),
        UI2_ATHLAS_GRID_DEMO_SIZE_CASE,
        width,
        height,
        crate::gfx::imba_athlas::athlasmetrics::ATHLAS_BUCKET_COUNT,
        cell_w,
        cell_h
    );

    embassy_time::Timer::after(embassy_time::Duration::from_millis(UI2_ATHLAS_GRID_DEMO_DEFER_MS))
        .await;

    for (bucket, (bucket_w, bucket_h)) in bucket_dims.iter().copied().enumerate() {
        let Some((upload_w, upload_h, rgba)) =
            crate::gfx::imba_athlas::imba_athlas_bucket_surface_rgba(
                UI2_ATHLAS_GRID_DEMO_SIZE_CASE,
                bucket,
                UI2_ATHLAS_GRID_DEMO_BG_RGBA,
                UI2_ATHLAS_GRID_DEMO_FG_RGBA,
            )
        else {
            crate::log!(
                "ui2-athlas-grid-demo: bucket surface build failed size_case={} bucket={}\n",
                UI2_ATHLAS_GRID_DEMO_SIZE_CASE,
                bucket,
            );
            return;
        };

        if upload_w != bucket_w || upload_h != bucket_h {
            crate::log!(
                "ui2-athlas-grid-demo: bucket size changed bucket={} expected={}x{} got={}x{}\n",
                bucket,
                bucket_w,
                bucket_h,
                upload_w,
                upload_h
            );
            return;
        }

        let (cell_x, cell_y) = bucket_cell_origin(bucket, cell_w, cell_h);
        let upload_x = cell_x + (cell_w - bucket_w) / 2;
        let upload_y = cell_y + (cell_h - bucket_h) / 2;
        if !surface.upload_rgba_region(
            upload_x,
            upload_y,
            bucket_w,
            bucket_h,
            rgba.as_slice(),
            "ui2-athlas-grid-demo-upload-region",
        ) {
            crate::log!(
                "ui2-athlas-grid-demo: region upload failed window={} tex={} bucket={} rect={}x{}@{},{}\n",
                surface.window_id(),
                surface.tex_id(),
                bucket,
                bucket_w,
                bucket_h,
                upload_x,
                upload_y
            );
            return;
        }
    }

    crate::log!(
        "ui2-athlas-grid-demo: window={} tex={} deferred_bucket_grid size_case={} size={}x{} buckets={}\n",
        surface.window_id(),
        surface.tex_id(),
        UI2_ATHLAS_GRID_DEMO_SIZE_CASE,
        width,
        height,
        crate::gfx::imba_athlas::athlasmetrics::ATHLAS_BUCKET_COUNT
    );

    loop {
        embassy_time::Timer::after(embassy_time::Duration::from_secs(3600)).await;
    }
}
