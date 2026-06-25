use alloc::string::String;
use alloc::vec::Vec;

use embassy_time::{Duration as EmbassyDuration, Timer};

use super::super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_line,
    print_shell_line, set_matrix_target_active, switch_matrix_target_slot,
};
use crate::disc::block;
use crate::shell2::shell2_cmd::ParseOutcome;

const DIASHOW_DIR: &str = "diashow";
const DIASHOW_SLOT: &str = "dia";
const MAX_IMAGES: usize = 200;
const START_DELAY_MS: u64 = 1_000;
const FRAME_DELAY_MS: u64 = 33;
const AP1_UI_SERVICE_SLOT: u32 = 1;

struct Slide {
    path: String,
    decoded: crate::ui3::img::jpeg_codec::DecodedJpeg,
}

pub(crate) fn try_parse(io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    if !rest.trim().is_empty() {
        print_shell_line(io, "diashow: usage `diashow`");
        return ParseOutcome::Handled;
    }

    let active_target = matrix_target_for_backend(io);
    let target = switch_matrix_target_slot(&active_target, DIASHOW_SLOT);
    print_matrix_target_line(&target, "diashow: scanning /diashow/**/*.jpg|jpeg on AP1 uicore");

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
        return Err(String::from("no .jpg/.jpeg files found in /diashow"));
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

    let scanout = crate::intel::active_scanout_dimensions()
        .map(|(w, h)| alloc::format!(" scanout={}x{}", w, h))
        .unwrap_or_default();
    print_matrix_target_line(
        target,
        alloc::format!(
            "diashow: presenting {} frame(s) after {}ms{}",
            slides.len(),
            START_DELAY_MS,
            scanout
        )
        .as_str(),
    );

    Timer::after(EmbassyDuration::from_millis(START_DELAY_MS)).await;

    let mut presented = 0usize;
    for slide in slides.iter() {
        let ok = crate::intel::present_rgba_primary_center_unscaled(
            slide.decoded.rgba.as_slice(),
            slide.decoded.width,
            slide.decoded.height,
            (slide.decoded.width as usize).saturating_mul(4),
            "diashow",
        );
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
            "diashow: done presented={}/{} delay={}ms",
            presented,
            slides.len(),
            FRAME_DELAY_MS
        )
        .as_str(),
    );
    Ok(())
}

async fn collect_jpeg_paths(disk: block::DeviceHandle) -> Result<Vec<String>, String> {
    let mut paths = Vec::new();
    let mut dirs = Vec::new();
    dirs.push(String::from(DIASHOW_DIR));

    while let Some(dir) = dirs.pop() {
        let Some(listing) = crate::r::fs::trueosfs::list_dir_async(disk, dir.as_str())
            .await
            .map_err(|err| alloc::format!("list /{} failed: {:?}", dir, err))?
        else {
            continue;
        };

        for name in listing.lines().filter(|name| !name.is_empty()) {
            let path = alloc::format!("{}/{}", dir, name);
            if is_jpeg_name(name) {
                if crate::r::fs::trueosfs::file_info_async(disk, path.as_str())
                    .await
                    .map_err(|err| alloc::format!("stat {} failed: {:?}", path, err))?
                    .is_some()
                {
                    paths.push(path);
                    if paths.len() >= MAX_IMAGES {
                        paths.sort();
                        return Ok(paths);
                    }
                    continue;
                }
            }

            if crate::r::fs::trueosfs::dir_has_children_async(disk, path.as_str())
                .await
                .map_err(|err| alloc::format!("stat dir {} failed: {:?}", path, err))?
            {
                dirs.push(path);
            }
        }
    }

    paths.sort();
    Ok(paths)
}

fn is_jpeg_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    (name.len() > ".jpg".len() && lower.ends_with(".jpg"))
        || (name.len() > ".jpeg".len() && lower.ends_with(".jpeg"))
}
