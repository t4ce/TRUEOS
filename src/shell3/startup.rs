//! Startup commands are admitted through the regular visible Shell3 instance pool.

use alloc::{string::String, vec::Vec};

pub(crate) enum Startup {
    Script { launch_script: String },
    Blueprint { archive: String, bytes: Vec<u8>, script: Option<String> },
}

impl Startup {
    pub(super) fn launch(self, shell: &mut super::Shell3) {
        match self {
            Self::Script { .. } => super::MatrixSlots::echo(shell.active_matrix_slot.as_deref(), shell.active_matrix_lifetime, "Blueprint startup is disabled in the kernel baseline".into()),
            Self::Blueprint { archive, bytes, script } => {
                match super::service::launch_bytes_with_script(archive, bytes, shell.tui_frontend(), script) {
                    Ok(app) => {
                        let slot = app.slot.clone();
                        shell.select_queued_app(app);
                        // Use the same navigation helper as global §slot entry.
                        shell.select_matrix_slot_name(&slot);
                    }
                    Err(error) => super::MatrixSlots::echo(shell.active_matrix_slot.as_deref(), shell.active_matrix_lifetime, error),
                }
            }
        }
    }
}

/// Preserve ordinary kernel progress after the Blueprint releases its terminal lease.
pub(crate) fn record_host_line(target: &crate::shell3::MatrixTarget, line: &str) {
    if !super::tui::supports(target) {
        return;
    }
    let lease = crate::shell3::matrix_target_slot_lease(target);
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
