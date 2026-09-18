use alloc::string::String;
use trueos_executor::{Spawner, task};

use super::super::{ShellBackend2, print_shell_line};
use crate::shell2::shell2_cmd::ParseOutcome;

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "wc3_probe: usage `wc3_probe <vm-id>`");
}

fn launcher_usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "wc3_launcher: usage `wc3_launcher <vm-id> <trueosfs-path>`");
}

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
    let mut parts = rest.split_whitespace();
    let Some(vm_id) = parts.next().and_then(|value| value.parse::<u8>().ok()) else {
        launcher_usage(io);
        return ParseOutcome::Handled;
    };
    let Some(path) = parts.next() else {
        launcher_usage(io);
        return ParseOutcome::Handled;
    };
    if parts.next().is_some() {
        launcher_usage(io);
        return ParseOutcome::Handled;
    }
    match load_launcher(*spawner, io, vm_id, String::from(path)) {
        Ok(token) => spawner.spawn(token),
        Err(_) => print_shell_line(io, "wc3_launcher: loader task already running"),
    }
    ParseOutcome::Handled
}

#[task]
async fn load_launcher(spawner: Spawner, io: &'static dyn ShellBackend2, vm_id: u8, path: String) {
    let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() else {
        print_shell_line(io, "wc3_launcher: no TRUEOSFS root");
        return;
    };
    let bytes = match crate::r::fs::trueosfs::file_out_async(disk, &path).await {
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
    match crate::hv::start_wc3_launcher(vm_id, &spawner, &bytes) {
        Ok(()) => print_shell_line(
            io,
            alloc::format!("wc3_launcher: queued Gate-1A on vm{}", vm_id).as_str(),
        ),
        Err(error) => print_shell_line(
            io,
            alloc::format!("wc3_launcher: start failed vm{} error={error:?}", vm_id).as_str(),
        ),
    }
}
