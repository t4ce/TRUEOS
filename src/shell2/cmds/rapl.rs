use embassy_executor::Spawner;

use crate::shell2::shell2_cmd::ParseOutcome;
use crate::shell2::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_line,
    print_shell_line, set_matrix_target_active,
};

const USAGE: &str = "rapl: usage `rapl store`";

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    if args.next() != Some("store") || args.next().is_some() {
        print_shell_line(io, USAGE);
        return ParseOutcome::Handled;
    }

    let target = matrix_target_for_backend(io);
    set_matrix_target_active(&target, true);
    match rapl_store_task(target.clone()) {
        Ok(token) => spawner.spawn(token),
        Err(_) => {
            set_matrix_target_active(&target, false);
            print_shell_line(io, "rapl store: task unavailable");
        }
    }

    ParseOutcome::Handled
}

#[embassy_executor::task(pool_size = 2)]
async fn rapl_store_task(target: MatrixTarget) {
    match crate::power::rapl::store_history_to_trueosfs().await {
        Ok(bytes) => print_matrix_target_line(
            &target,
            alloc::format!(
                "rapl store: saved {} bytes -> {}",
                bytes,
                crate::power::rapl::RAPL_TRUEOSFS_PATH
            )
            .as_str(),
        ),
        Err(crate::power::rapl::RaplStoreError::RootNotMounted) => {
            print_matrix_target_line(&target, "rapl store: TRUEOSFS root is not mounted")
        }
        Err(crate::power::rapl::RaplStoreError::RootUnavailable) => {
            print_matrix_target_line(&target, "rapl store: TRUEOSFS root is unavailable")
        }
        Err(crate::power::rapl::RaplStoreError::NoSpaceOrFs) => {
            print_matrix_target_line(&target, "rapl store: no space or filesystem unavailable")
        }
        Err(crate::power::rapl::RaplStoreError::Io(err)) => print_matrix_target_line(
            &target,
            alloc::format!("rapl store: write failed: {:?}", err).as_str(),
        ),
    }
    set_matrix_target_active(&target, false);
}
