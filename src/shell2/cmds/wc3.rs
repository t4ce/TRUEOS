use trueos_executor::Spawner;

use super::super::{print_shell_line, ShellBackend2};
use crate::shell2::shell2_cmd::ParseOutcome;

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "wc3_probe: usage `wc3_probe <vm-id>`");
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