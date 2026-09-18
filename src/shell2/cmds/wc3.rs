use trueos_executor::{Spawner, task};

use super::super::{ShellBackend2, print_shell_line};
use crate::shell2::shell2_cmd::ParseOutcome;

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "wc3_probe: usage `wc3_probe <vm-id>`");
}

const LAUNCHER_VM_ID: u8 = 0;
const LAUNCHER_PATH: &str = "apps/common/Warcraft III/Warcraft III.exe";

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> ParseOutcome {
    let rest = rest.trim();
    if matches!(rest, "help" | "-h" | "--help") {
        usage(io);
        return ParseOutcome::Handled;
    }

    let Ok(vm_id) = rest.parse::<u8>() else {
        usage(io);
        return ParseOutcome::Handled;
    };

    match crate::hv::start_wc3_launcher_test(vm_id, spawner) {
        Ok(()) => print_shell_line(
            io,
            alloc::format!(
                "wc3_probe: queued Gate-0 on vm{}; expect `wc3: gate-0 complete ... fs=ok`",
                vm_id
            )
            .as_str(),
        ),
        Err(error) => print_shell_line(
            io,
            alloc::format!("wc3_probe: start failed vm{} error={error:?}", vm_id).as_str(),
        ),
    }

    ParseOutcome::Handled
}

pub(crate) fn try_parse_launcher(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> ParseOutcome {
    if !rest.trim().is_empty() {
        print_shell_line(io, "wc3_launcher: usage `wc3_launcher`");
        return ParseOutcome::Handled;
    }
    match load_launcher(*spawner, io) {
        Ok(token) => spawner.spawn(token),
        Err(_) => print_shell_line(io, "wc3_launcher: loader task already running"),
    }
    ParseOutcome::Handled
}

#[task]
async fn load_launcher(spawner: Spawner, io: &'static dyn ShellBackend2) {
    let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() else {
        print_shell_line(io, "wc3_launcher: no TRUEOSFS root");
        return;
    };
    let bytes = match crate::r::fs::trueosfs::file_out_async(disk, LAUNCHER_PATH).await {
        Ok(Some(bytes)) => bytes,
        Ok(None) => {
            print_shell_line(io, "wc3_launcher: artifact not found");
            return;
        }
        Err(error) => {
            print_shell_line(
                io,
                alloc::format!("wc3_launcher: artifact read failed {error:?}").as_str(),
            );
            return;
        }
    };
    match crate::hv::start_wc3_launcher(LAUNCHER_VM_ID, &spawner, &bytes) {
        Ok(()) => print_shell_line(
            io,
            "wc3_launcher: queued Gate-1A on vm0",
        ),
        Err(error) => print_shell_line(
            io,
            alloc::format!("wc3_launcher: start failed vm0 error={error:?}").as_str(),
        ),
    }
}
