use core::str::SplitWhitespace;

use embassy_executor::Spawner;

use super::super::{ShellBackend2, print_shell_line};
use crate::intel::gpgpu::{
    MANDEL64_WORKLIST_DEFAULT_ITERATIONS, MANDEL64_WORKLIST_MAX_ITERATIONS,
    shell_mandel64_worklist_scanout, shell_twemoji_atlas_worklist_scanout,
};
use crate::shell2::shell2_cmd::{CommandSessionKind, ParseOutcome};

const CANVAS2D_SPRITES64_COUNT: u32 = 16;

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "gpgpu canvas2d sprites64");
    print_shell_line(io, "gpgpu canvas2d mandel64 [iterations]");
    print_shell_line(io, "gpgpu canvas3d cube");
    print_shell_line(io, "gpgpu canvas3d ico");
    print_shell_line(io, "gpgpu canvas3d para");
    print_shell_line(io, "gpgpu smoke");
}

fn expect_no_more(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) -> bool {
    if args.next().is_none() {
        true
    } else {
        usage(io);
        false
    }
}

fn run_canvas2d(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) {
    let Some(kind) = args.next() else {
        usage(io);
        return;
    };

    if kind.eq_ignore_ascii_case("sprites64") {
        if !expect_no_more(io, args) {
            return;
        }
        run_canvas2d_sprites64(io);
    } else if kind.eq_ignore_ascii_case("mandel64") {
        run_canvas2d_mandel64(io, args);
    } else {
        usage(io);
    }
}

fn run_canvas2d_sprites64(io: &'static dyn ShellBackend2) -> bool {
    let Some(result) = shell_twemoji_atlas_worklist_scanout(CANVAS2D_SPRITES64_COUNT) else {
        print_shell_line(
            io,
            "gpgpu canvas2d sprites64: no result (check primary surface, atlas cache, and worklist artifact)",
        );
        return false;
    };
    let msg = alloc::format!(
        "gpgpu canvas2d sprites64: mode=sprite64-worklist ok={} requested={} desc={} walkers={} pixels={} submit_ms={} present_ms={} total_ms={} last_id={} last_dst={},{} primary={}x{} slots={} atlas_gpu=0x{:X} desc_gpu=0x{:X} presented={}",
        result.ok as u8,
        result.requested,
        result.descriptors,
        result.walkers,
        result.copied_pixels,
        result.submit_ms,
        result.present_ms,
        result.total_ms,
        result.last_slot,
        result.last_dst_xy.x,
        result.last_dst_xy.y,
        result.primary_width,
        result.primary_height,
        result.slots,
        result.atlas_gpu,
        result.desc_gpu,
        result.presented as u8
    );
    print_shell_line(io, msg.as_str());
    result.ok
}

fn run_canvas2d_mandel64(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) -> bool {
    let iterations = match args.next() {
        Some(value) => match value.parse::<u32>() {
            Ok(iterations) => iterations.clamp(1, MANDEL64_WORKLIST_MAX_ITERATIONS),
            Err(_) => {
                usage(io);
                return false;
            }
        },
        None => MANDEL64_WORKLIST_DEFAULT_ITERATIONS,
    };
    if !expect_no_more(io, args) {
        return false;
    }

    let Some(result) = shell_mandel64_worklist_scanout(iterations) else {
        print_shell_line(
            io,
            "gpgpu canvas2d mandel64: no result (check primary surface, iGPU claim, and mandel artifact)",
        );
        return false;
    };
    let msg = alloc::format!(
        "gpgpu canvas2d mandel64: mode=mandel64-worklist ok={} iterations={} requested={} desc={} walkers={} pixels={} submit_ms={} present_ms={} total_ms={} last_src={},{} last_dst={},{} primary={}x{} desc_gpu=0x{:X} presented={}",
        result.ok as u8,
        iterations,
        result.requested,
        result.descriptors,
        result.walkers,
        result.pixels,
        result.submit_ms,
        result.present_ms,
        result.total_ms,
        result.last_src_xy.x,
        result.last_src_xy.y,
        result.last_dst_xy.x,
        result.last_dst_xy.y,
        result.primary_width,
        result.primary_height,
        result.desc_gpu,
        result.presented as u8
    );
    print_shell_line(io, msg.as_str());
    result.ok
}

fn run_canvas3d(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    let Some(kind) = args.next() else {
        usage(io);
        return ParseOutcome::Handled;
    };
    if !expect_no_more(io, args) {
        return ParseOutcome::Handled;
    }

    let session_id = if kind.eq_ignore_ascii_case("cube") {
        crate::ui3::ui3_canvas::submit_canvas3d_cube(spawner, io)
    } else if kind.eq_ignore_ascii_case("ico") {
        crate::ui3::ui3_canvas::submit_canvas3d_ico(spawner, io)
    } else if kind.eq_ignore_ascii_case("para") {
        crate::ui3::ui3_canvas::submit_canvas3d_para(spawner, io)
    } else {
        usage(io);
        return ParseOutcome::Handled;
    };

    match session_id {
        Some(session_id) => {
            ParseOutcome::StartSession(CommandSessionKind::GpuCanvasRunning(session_id))
        }
        None => ParseOutcome::Handled,
    }
}

fn run_smoke(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) {
    if !expect_no_more(io, args) {
        return;
    }
    let sprites_ok = run_canvas2d_sprites64(io);
    let mut mandel_args = "".split_whitespace();
    let mandel_ok = run_canvas2d_mandel64(io, &mut mandel_args);
    let msg = alloc::format!(
        "gpgpu smoke: canvas2d_sprites64={} canvas2d_mandel64={} ok={}",
        sprites_ok as u8,
        mandel_ok as u8,
        (sprites_ok && mandel_ok) as u8
    );
    print_shell_line(io, msg.as_str());
}

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    let Some(cmd) = args.next() else {
        usage(io);
        return ParseOutcome::Handled;
    };

    if cmd.eq_ignore_ascii_case("canvas2d") {
        run_canvas2d(io, args);
    } else if cmd.eq_ignore_ascii_case("canvas3d") {
        return run_canvas3d(spawner, io, args);
    } else if cmd.eq_ignore_ascii_case("smoke") {
        run_smoke(io, args);
    } else {
        usage(io);
    }

    ParseOutcome::Handled
}
