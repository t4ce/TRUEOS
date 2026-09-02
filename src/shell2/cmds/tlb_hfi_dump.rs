use trueos_executor::Spawner;

use super::super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_line,
    print_shell_line, set_matrix_target_active,
};

const CPUID_SECTION_STALE_SCOPE: &str =
    "capture_policy=registration-time-cpuid-only msr_programming=no hardware_table=unconfigured scheduler_consumer=none";
const CPUID_SECTION_EXPLICIT_SCOPE: &str =
    "section_scope=cpuid-profile-metadata capture_policy=registration-time-cpuid-only section_msr_access=no live_table_state=reported-separately scheduler_consumer=none";

fn qualify_cpuid_section_scope(out: &mut alloc::string::String) {
    let Some(start) = out.find(CPUID_SECTION_STALE_SCOPE) else {
        return;
    };
    let end = start.saturating_add(CPUID_SECTION_STALE_SCOPE.len());
    out.replace_range(start..end, CPUID_SECTION_EXPLICIT_SCOPE);
}

#[trueos_executor::task(pool_size = 2)]
async fn dump_task(target: MatrixTarget) {
    let mut out = super::tlb_core::build_dump_text().await;
    qualify_cpuid_section_scope(&mut out);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("\n=== Intel HFI Hardware Feedback Table ===\n");
    out.push_str(&crate::power::hfi::table_snapshot_text());

    print_matrix_target_line(
        &target,
        alloc::format!("Writing {} bytes to {}...", out.len(), super::tlb_core::DUMP_FILE_PATH)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qualifies_cpuid_metadata_without_claiming_live_table_state() {
        let mut out = alloc::format!("before\n{}\nafter\n", CPUID_SECTION_STALE_SCOPE);
        qualify_cpuid_section_scope(&mut out);
        assert!(!out.contains(CPUID_SECTION_STALE_SCOPE));
        assert!(out.contains(CPUID_SECTION_EXPLICIT_SCOPE));
    }
}
