use alloc::string::String;
use alloc::vec::Vec;

use embassy_time::{Duration as EmbassyDuration, Timer};

use super::super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_line,
    print_shell_line, set_matrix_target_active, switch_matrix_target_slot,
};
use crate::disc::block;
use crate::intel::types::{UiPlaneSlot, UiPresent, UiRect, UiSurfaceFormat};
use crate::r::ui_surface::{self, UiSurfaceHandle};
use crate::shell2::shell2_cmd::ParseOutcome;

const DIASHOW_DIR: &str = "diashow";
const DIASHOW_SLOT: &str = "dia";
const MAX_IMAGES: usize = 200;
const START_DELAY_MS: u64 = 1_000;
const FRAME_DELAY_MS: u64 = 15;
const PRESENT_SCALE: u32 = 2;
const AP1_UI_SERVICE_SLOT: u32 = 1;

struct Slide {
    path: String,
    decoded: crate::ui3::img::jpeg_codec::DecodedJpeg,
}

struct DiashowGpuSurface {
    handle: Option<UiSurfaceHandle>,
    width: u32,
    height: u32,
}

impl DiashowGpuSurface {
    const fn new() -> Self {
        Self {
            handle: None,
            width: 0,
            height: 0,
        }
    }

    fn ensure(&mut self, width: u32, height: u32) -> Result<UiSurfaceHandle, String> {
        if let Some(handle) = self.handle
            && self.width == width
            && self.height == height
        {
            return Ok(handle);
        }

        self.destroy();
        let handle = ui_surface::create_surface(width, height, UiSurfaceFormat::Rgba8888)
            .map_err(|err| alloc::format!("gpu surface create failed: {:?}", err))?;
        self.handle = Some(handle);
        self.width = width;
        self.height = height;
        Ok(handle)
    }

    fn destroy(&mut self) {
        if let Some(handle) = self.handle.take() {
            let _ = ui_surface::destroy_surface(handle);
        }
        self.width = 0;
        self.height = 0;
    }
}

impl Drop for DiashowGpuSurface {
    fn drop(&mut self) {
        self.destroy();
    }
}

struct PresentGeometry {
    src: UiRect,
    dst: UiRect,
}

pub(crate) fn try_parse(io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    if !rest.trim().is_empty() {
        print_shell_line(io, "diashow: usage `diashow`");
        return ParseOutcome::Handled;
    }

    let active_target = matrix_target_for_backend(io);
    let target = switch_matrix_target_slot(&active_target, DIASHOW_SLOT);
    print_matrix_target_line(&target, "diashow: scanning /diashow/*.jpeg on AP1 uicore");

    let Some(ap1) = crate::workers::spawner_for_slot(AP1_UI_SERVICE_SLOT) else {
        print_matrix_target_line(&target, "diashow: AP1 uicore spawner is not registered");
        return ParseOutcome::Handled;
    };

    set_matrix_target_active(&target, true);
    match diashow_task(target.clone()) {
        Ok(token) => {
            ap1.spawn_and_wake_remote(token);
            print_matrix_target_line(&target, "diashow: queued");
        }
        Err(err) => {
            set_matrix_target_active(&target, false);
            print_matrix_target_line(
                &target,
                alloc::format!("diashow: spawn failed {err:?}").as_str(),
            );
        }
    }

    ParseOutcome::Handled
}

#[embassy_executor::task(pool_size = 1)]
async fn diashow_task(target: MatrixTarget) {
    let result = run_diashow(&target).await;
    if let Err(err) = result {
        print_matrix_target_line(&target, alloc::format!("diashow: {err}").as_str());
    }
    set_matrix_target_active(&target, false);
}

async fn run_diashow(target: &MatrixTarget) -> Result<(), String> {
    let disk = crate::r::fs::trueosfs::primary_root_handle()
        .ok_or_else(|| String::from("no TRUEOSFS root mounted"))?;

    let paths = collect_jpeg_paths(disk).await?;
    if paths.is_empty() {
        return Err(String::from("no .jpeg files found in /diashow"));
    }

    print_matrix_target_line(
        target,
        alloc::format!("diashow: decoding {} jpeg(s)", paths.len()).as_str(),
    );

    let mut slides = Vec::new();
    for path in paths {
        let Some(bytes) = crate::r::fs::trueosfs::file_out_async(disk, path.as_str())
            .await
            .map_err(|err| alloc::format!("read {} failed: {:?}", path, err))?
        else {
            continue;
        };

        match crate::ui3::img::jpeg_codec::decode_jpeg_rgba(bytes.as_slice()) {
            Ok(decoded) => {
                slides.push(Slide { path, decoded });
            }
            Err(err) => {
                print_matrix_target_line(
                    target,
                    alloc::format!("diashow: decode skipped code={} path={}", err.code(), path)
                        .as_str(),
                );
            }
        }
    }

    if slides.is_empty() {
        return Err(String::from("all jpeg decodes failed"));
    }

    let (scanout_width, scanout_height) = crate::intel::active_scanout_dimensions()
        .ok_or_else(|| String::from("no active scanout"))?;
    let scanout = alloc::format!(" scanout={}x{}", scanout_width, scanout_height);
    print_matrix_target_line(
        target,
        alloc::format!(
            "diashow: presenting {} frame(s) scale={}x after {}ms{}",
            slides.len(),
            PRESENT_SCALE,
            START_DELAY_MS,
            scanout
        )
        .as_str(),
    );

    Timer::after(EmbassyDuration::from_millis(START_DELAY_MS)).await;

    let mut presented = 0usize;
    let mut gpu_presented = 0usize;
    let mut fallback_presented = 0usize;
    let mut gpu_surface = DiashowGpuSurface::new();
    for slide in slides.iter() {
        let geometry = centered_scaled_geometry(
            slide.decoded.width,
            slide.decoded.height,
            PRESENT_SCALE,
            scanout_width,
            scanout_height,
        );
        let gpu_ok = match geometry {
            Some(geometry) => present_slide_gpu(&mut gpu_surface, slide, geometry),
            None => false,
        };
        let ok = if gpu_ok {
            gpu_presented = gpu_presented.saturating_add(1);
            true
        } else if present_slide_cpu_unscaled(slide) {
            fallback_presented = fallback_presented.saturating_add(1);
            true
        } else {
            false
        };
        if ok {
            presented = presented.saturating_add(1);
        } else {
            print_matrix_target_line(
                target,
                alloc::format!("diashow: present failed path={}", slide.path).as_str(),
            );
        }
        Timer::after(EmbassyDuration::from_millis(FRAME_DELAY_MS)).await;
    }

    print_matrix_target_line(
        target,
        alloc::format!(
            "diashow: done presented={}/{} gpu={} fallback={} delay={}ms",
            presented,
            slides.len(),
            gpu_presented,
            fallback_presented,
            FRAME_DELAY_MS
        )
        .as_str(),
    );
    Ok(())
}

async fn collect_jpeg_paths(disk: block::DeviceHandle) -> Result<Vec<String>, String> {
    let Some(listing) = crate::r::fs::trueosfs::list_dir_async(disk, DIASHOW_DIR)
        .await
        .map_err(|err| alloc::format!("list /{} failed: {:?}", DIASHOW_DIR, err))?
    else {
        return Err(String::from("TRUEOSFS root is unavailable"));
    };

    let mut paths: Vec<String> = listing
        .lines()
        .filter(|name| is_jpeg_name(name))
        .take(MAX_IMAGES)
        .map(|name| alloc::format!("{}/{}", DIASHOW_DIR, name))
        .collect();
    paths.sort();
    Ok(paths)
}

fn is_jpeg_name(name: &str) -> bool {
    name.len() > ".jpeg".len() && name.to_ascii_lowercase().ends_with(".jpeg")
}

fn centered_scaled_geometry(
    src_width: u32,
    src_height: u32,
    scale: u32,
    scanout_width: u32,
    scanout_height: u32,
) -> Option<PresentGeometry> {
    if src_width == 0 || src_height == 0 || scale == 0 || scanout_width == 0 || scanout_height == 0
    {
        return None;
    }

    let visible_src_w = src_width.min(scanout_width.checked_div(scale).unwrap_or(0));
    let visible_src_h = src_height.min(scanout_height.checked_div(scale).unwrap_or(0));
    if visible_src_w == 0 || visible_src_h == 0 {
        return None;
    }

    let dst_w = visible_src_w.checked_mul(scale)?;
    let dst_h = visible_src_h.checked_mul(scale)?;
    let src_x = src_width.saturating_sub(visible_src_w) / 2;
    let src_y = src_height.saturating_sub(visible_src_h) / 2;
    let dst_x = scanout_width.saturating_sub(dst_w) / 2;
    let dst_y = scanout_height.saturating_sub(dst_h) / 2;

    Some(PresentGeometry {
        src: UiRect::new(src_x, src_y, visible_src_w, visible_src_h),
        dst: UiRect::new(dst_x, dst_y, dst_w, dst_h),
    })
}

fn present_slide_gpu(
    gpu_surface: &mut DiashowGpuSurface,
    slide: &Slide,
    geometry: PresentGeometry,
) -> bool {
    let Ok(handle) = gpu_surface.ensure(slide.decoded.width, slide.decoded.height) else {
        return false;
    };
    let full = UiRect::new(0, 0, slide.decoded.width, slide.decoded.height);
    let src_pitch = (slide.decoded.width as usize).saturating_mul(4);
    if ui_surface::write_surface_rgba(handle, full, slide.decoded.rgba.as_slice(), src_pitch)
        .is_err()
    {
        return false;
    }

    ui_surface::present_surface(
        handle,
        UiPresent {
            src: geometry.src,
            dst: geometry.dst,
            plane: UiPlaneSlot::Primary,
        },
        "diashow",
    )
    .is_ok()
}

fn present_slide_cpu_unscaled(slide: &Slide) -> bool {
    crate::intel::present_rgba_primary_center_unscaled(
        slide.decoded.rgba.as_slice(),
        slide.decoded.width,
        slide.decoded.height,
        (slide.decoded.width as usize).saturating_mul(4),
        "diashow-fallback",
    )
}
