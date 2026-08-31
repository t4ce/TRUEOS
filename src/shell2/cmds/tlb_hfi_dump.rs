use trueos_executor::Spawner;

use super::super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_line, print_shell_line,
    set_matrix_target_active,
};

#[trueos_executor::task(pool_size = 2)]
async fn dump_task(target: MatrixTarget) {
    let mut out = super::tlb_core::build_dump_text().await;
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("\n=== Intel HFI Hardware Feedback Table ===\n");
    out.push_str(&crate::power::hfi::table_snapshot_text());

    print_matrix_target_line(
        &target,
        alloc::format!(
            "Writing {} bytes to {}...",
            out.len(),
            super::tlb_core::DUMP_FILE_PATH
        )
        .as_str(),
    );

    let bytes = out.into_bytes();
    match super::tlb_core::write_dump_bytes_to_default_path(&bytes).await {
        Ok(()) => print_matrix_target_line(
            &target,
            alloc::format!("tlb dump: wrote {} bytes", bytes.len()).as_str(),
        ),
        Err(error) => print_matrix_target_line(
            &target,
            alloc::format!("tlb dump: write failed: {:?}", error).as_str(),
        ),
    }
    set_matrix_target_active(&target, false);
}

pub(crate) fn start(spawner: &Spawner, io: &'static dyn ShellBackend2) {
    let target = matrix_target_for_backend(io);
    set_matrix_target_active(&target, true);
    match dump_task(target.clone()) {
        Ok(token) => spawner.spawn(token),
        Err(_) => {
            set_matrix_target_active(&target, false);
            print_shell_line(io, "tlb dump: task unavailable");
        }
    }
}
