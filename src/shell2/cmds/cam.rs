//! HDMI camera bring-up: inspect receiver status or open its first frozen tile.
use trueos_executor::{Spawner, task};
use super::super::{ShellBackend2, print_shell_line};
use super::super::shell2_cmd::ParseOutcome;

pub(crate) fn try_parse(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    match rest.trim() {
        "status" => report(io),
        "" | "shot" => match capture(*spawner, io) {
            Ok(token) => spawner.spawn(token),
            Err(_) => print_shell_line(io, "cam: capture already running"),
        },
        _ => print_shell_line(io, "cam: usage cam [status|shot]"),
    }
    ParseOutcome::Handled
}
fn report(io: &'static dyn ShellBackend2) {
    match crate::tga::camera::status() {
        Ok(s) => print_shell_line(io, &alloc::format!(
            "cam: HDMI hpd={} pll_lock={} tile_ready={} input={}x{} frames={} edid_reads={}",
            s.flags&1!=0,s.flags&2!=0,s.flags&4!=0,s.width,s.height,s.frames,s.edid_reads)),
        Err(e) => print_shell_line(io, &alloc::format!("cam: {e}")),
    }
}
#[task]
async fn capture(spawner: Spawner, io: &'static dyn ShellBackend2) {
    report(io);
    let pixels = match crate::tga::camera::read_tile().await {
        Ok(p) => p,
        Err(e) => { print_shell_line(io, &alloc::format!("cam: {e}")); return; }
    };
    let png = match crate::graphics::encoder::png::encode_rgba8_png(64,64,&pixels,256) {
        Ok(p) => p,
        Err(e) => { print_shell_line(io, &alloc::format!("cam: PNG encode failed: {e:?}")); return; }
    };
    let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() else {
        print_shell_line(io,"cam: no TRUEOSFS root");return;
    };
    let path=alloc::format!("camera/hdmi-tile-{}.png",crate::chronos::monotonic_nanos());
    match crate::r::fs::trueosfs::file_in_typed_async(disk,&path,&png,infer::ContentTypeId::PNG).await {
        Ok(true) => {
            print_shell_line(io,&alloc::format!("cam: captured frozen 64x64 HDMI tile to trueosfs:/{path}"));
            crate::log_important!("cam: captured path={} bytes={} source=hdmi-rx tile=64x64\n",path,png.len());
            super::img::try_parse(&spawner,io,&alloc::format!("trueosfs:/{path}"));
        }
        result => print_shell_line(io,&alloc::format!("cam: save failed: {result:?}")),
    }
}
