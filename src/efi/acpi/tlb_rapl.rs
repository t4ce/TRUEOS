use crate::workers::WorkerSpawner as Spawner;

use super::{MatrixTarget, Output, print_target_line};

pub(crate) fn store(spawner: &Spawner, io: &Output) {
    let target = Output::target(io);

    match rapl_store_task(target.clone()) {
        Ok(token) => spawner.spawn(token),
        Err(_) => {
            Output::line(io, "tlb rapl store: task unavailable");
        }
    }
}

#[trueos_executor::task(pool_size = 2)]
async fn rapl_store_task(target: MatrixTarget) {
    match crate::power::rapl::store_history_to_trueosfs().await {
        Ok(bytes) => print_target_line(
            &target,
            alloc::format!(
                "tlb rapl store: saved {} bytes -> {}",
                bytes,
                crate::power::rapl::RAPL_TRUEOSFS_PATH
            )
            .as_str(),
        ),
        Err(crate::power::rapl::RaplStoreError::RootNotMounted) => {
            print_target_line(&target, "tlb rapl store: TRUEOSFS root is not mounted")
        }
        Err(crate::power::rapl::RaplStoreError::RootUnavailable) => {
            print_target_line(&target, "tlb rapl store: TRUEOSFS root is unavailable")
        }
        Err(crate::power::rapl::RaplStoreError::NoSpaceOrFs) => {
            print_target_line(&target, "tlb rapl store: no space or filesystem unavailable")
        }
        Err(crate::power::rapl::RaplStoreError::Io(err)) => print_target_line(
            &target,
            alloc::format!("tlb rapl store: write failed: {:?}", err).as_str(),
        ),
    }
}
