use alloc::string::String as AllocString;
use core::str::SplitWhitespace;

use super::super::{ShellBackend2, print_shell_line};
use crate::intel::gpgpu::{
    GPGPU_SHELL_SURFACE_HEIGHT, GPGPU_SHELL_SURFACE_PITCH_BYTES, GPGPU_SHELL_SURFACE_WIDTH,
    GpgpuPoint, GpgpuRect, clear_rect_rgba8_white_upload_status, copy_rect_rgba8_upload_status,
    empty_eot_upload_status, shell_clear_white_rgba8, shell_copy_rgba8,
};
use crate::shell2::shell2_cmd::ParseOutcome;

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "gpgpu status");
    print_shell_line(io, "gpgpu clear [x y w h]");
    print_shell_line(io, "gpgpu copy [sx sy dx dy w h]");
    print_shell_line(io, "gpgpu smoke");
}

fn parse_i32(raw: Option<&str>) -> Option<i32> {
    raw?.parse::<i32>().ok()
}

fn parse_u32(raw: Option<&str>) -> Option<u32> {
    raw?.parse::<u32>().ok()
}

fn parse_clear_rect(args: &mut SplitWhitespace<'_>) -> Option<GpgpuRect> {
    let Some(x_raw) = args.next() else {
        return Some(GpgpuRect::new(0, 0, 4, 1));
    };
    let x = x_raw.parse::<i32>().ok()?;
    let y = parse_i32(args.next())?;
    let width = parse_u32(args.next())?;
    let height = parse_u32(args.next())?;
    if args.next().is_some() {
        return None;
    }
    Some(GpgpuRect::new(x, y, width, height))
}

fn parse_copy_rect(args: &mut SplitWhitespace<'_>) -> Option<(GpgpuRect, GpgpuPoint)> {
    let Some(sx_raw) = args.next() else {
        return Some((GpgpuRect::new(0, 0, 32, 1), GpgpuPoint::new(32, 0)));
    };
    let sx = sx_raw.parse::<i32>().ok()?;
    let sy = parse_i32(args.next())?;
    let dx = parse_i32(args.next())?;
    let dy = parse_i32(args.next())?;
    let width = parse_u32(args.next())?;
    let height = parse_u32(args.next())?;
    if args.next().is_some() {
        return None;
    }
    Some((GpgpuRect::new(sx, sy, width, height), GpgpuPoint::new(dx, dy)))
}

fn hex4(values: [u32; 4]) -> AllocString {
    alloc::format!(
        "[0x{:08X},0x{:08X},0x{:08X},0x{:08X}]",
        values[0],
        values[1],
        values[2],
        values[3]
    )
}

fn artifact_status(uploaded: bool) -> u8 {
    uploaded as u8
}

fn print_status(io: &'static dyn ShellBackend2) {
    let copy = copy_rect_rgba8_upload_status();
    let clear = clear_rect_rgba8_white_upload_status();
    let empty = empty_eot_upload_status();
    let msg = alloc::format!(
        "gpgpu: copy_upload={} clear_upload={} empty_upload={} shell_surface={}x{} pitch={} gpu=0x008A0000",
        artifact_status(copy.is_some()),
        artifact_status(clear.is_some()),
        artifact_status(empty.is_some()),
        GPGPU_SHELL_SURFACE_WIDTH,
        GPGPU_SHELL_SURFACE_HEIGHT,
        GPGPU_SHELL_SURFACE_PITCH_BYTES,
    );
    print_shell_line(io, msg.as_str());
}

fn run_clear(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) {
    let Some(rect) = parse_clear_rect(args) else {
        usage(io);
        return;
    };
    let Some(result) = shell_clear_white_rgba8(rect) else {
        print_shell_line(io, "gpgpu clear: no result (check iGPU claim, DMA, and rect bounds)");
        return;
    };
    let before = hex4(result.before_head);
    let after = hex4(result.after_head);
    let msg = alloc::format!(
        "gpgpu clear: ok={} rect={}x{}@{},{} spans={}/{} white={}/{} gpu=0x{:X} phys=0x{:X} before={} after={}",
        result.ok as u8,
        result.rect.width,
        result.rect.height,
        result.rect.x,
        result.rect.y,
        result.spans,
        result.expected_spans,
        result.white,
        result.pixels,
        result.surface.gpu,
        result.surface.phys,
        before,
        after
    );
    print_shell_line(io, msg.as_str());
}

fn run_copy(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) {
    let Some((src_rect, dst_xy)) = parse_copy_rect(args) else {
        usage(io);
        return;
    };
    let Some(result) = shell_copy_rgba8(src_rect, dst_xy) else {
        print_shell_line(
            io,
            "gpgpu copy: no result (check iGPU claim, DMA, bounds, and non-overlap)",
        );
        return;
    };
    let src = hex4(result.src_head);
    let dst = hex4(result.dst_head);
    let msg = alloc::format!(
        "gpgpu copy: ok={} rect={}x{} src={},{} dst={},{} spans={}/{} submits={}/{} copied={}/{} src_preserved={}/{} gpu=0x{:X} phys=0x{:X} src_head={} dst_head={}",
        result.ok as u8,
        result.src_rect.width,
        result.src_rect.height,
        result.src_rect.x,
        result.src_rect.y,
        result.dst_xy.x,
        result.dst_xy.y,
        result.spans,
        result.expected_spans,
        result.submits,
        result.expected_submits,
        result.copied,
        result.pixels,
        result.src_preserved,
        result.pixels,
        result.surface.gpu,
        result.surface.phys,
        src,
        dst
    );
    print_shell_line(io, msg.as_str());
}

pub(crate) fn try_parse(
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    let Some(cmd) = args.next() else {
        usage(io);
        return ParseOutcome::Handled;
    };

    if cmd.eq_ignore_ascii_case("status") {
        if args.next().is_some() {
            usage(io);
        } else {
            print_status(io);
        }
    } else if cmd.eq_ignore_ascii_case("clear") {
        run_clear(io, args);
    } else if cmd.eq_ignore_ascii_case("copy") {
        run_copy(io, args);
    } else if cmd.eq_ignore_ascii_case("smoke") {
        if args.next().is_some() {
            usage(io);
        } else {
            let mut clear_args = "".split_whitespace();
            let mut copy_args = "".split_whitespace();
            run_clear(io, &mut clear_args);
            run_copy(io, &mut copy_args);
        }
    } else {
        usage(io);
    }

    ParseOutcome::Handled
}
