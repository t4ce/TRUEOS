//! Startup commands are admitted through the regular visible Shell3 instance pool.

use alloc::{string::String, vec::Vec};

pub(crate) enum Startup {
    #[cfg(feature = "gridpaper")]
    Script { launch_script: String },
    Blueprint {
        archive: String,
        bytes: Vec<u8>,
        script: Option<String>,
    },
}

impl Startup {
    pub(super) fn launch(self, shell: &mut super::Shell3) {
        match self {
            #[cfg(feature = "gridpaper")]
            Self::Script { .. } => super::MatrixSlots::echo(
                shell.active_matrix_slot.as_deref(),
                shell.active_matrix_lifetime,
                "Blueprint startup is disabled in the kernel baseline".into(),
            ),
            Self::Blueprint {
                archive,
                bytes,
                script,
            } => {
                match super::service::launch_bytes_with_script(
                    archive,
                    bytes,
                    shell.tui_frontend(),
                    script,
                ) {
                    Ok(app) => {
                        let slot = app.slot.clone();
                        shell.select_queued_app(app);
                        // Use the same navigation helper as global §slot entry.
                        shell.select_matrix_slot_name(&slot);
                    }
                    Err(error) => super::MatrixSlots::echo(
                        shell.active_matrix_slot.as_deref(),
                        shell.active_matrix_lifetime,
                        error,
                    ),
                }
            }
        }
    }
}
