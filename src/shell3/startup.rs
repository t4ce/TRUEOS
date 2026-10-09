//! Startup commands are admitted through the regular visible Shell3 instance pool.

use alloc::string::String;

pub(crate) struct Startup {
    pub(crate) launch_script: String,
}

impl Startup {
    pub(super) fn launch(self, shell: &mut super::Shell3) {
        match crate::shell2::cmds::os::enqueue_to_shell3(
            shell.tui_frontend(),
            Some(self.launch_script),
        ) {
            Ok(app) => shell.select_queued_app(app),
            Err(error) => super::MatrixSlots::echo(
                shell.active_matrix_slot.as_deref(),
                shell.active_matrix_lifetime,
                error,
            ),
        }
    }
}

/// Preserve ordinary kernel progress after the Blueprint releases its terminal lease.
pub(crate) fn record_host_line(target: &crate::shell2::MatrixTarget, line: &str) {
    if !super::tui::supports(target) {
        return;
    }
    let lease = crate::shell2::matrix_target_slot_lease(target);
    let lifetime = super::matrix_slots()
        .lock()
        .lifetimes
        .iter()
        .find(|(name, _)| name == lease.name())
        .map(|(_, lifetime)| *lifetime);
    if let Some(lifetime) = lifetime {
        super::MatrixSlots::echo(Some(lease.name()), Some(lifetime), String::from(line));
    }
}
