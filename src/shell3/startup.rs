//! Startup commands are admitted through the regular visible Shell3 instance pool.

use alloc::{string::String, vec::Vec};

pub(crate) enum Startup {
    
    Blueprint {
        archive: String,
        bytes: Vec<u8>,
        script: Option<String>,
    },
}

impl Startup {
    pub(super) fn launch(self, shell: &mut super::Shell3) {
        match self {
            
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
                        // This selects the admitted foreground route as well
                        // as its Matrix slot. Ordinary slot navigation would
                        // clear that route before a TUI's initial lease claim.
                        shell.select_queued_app(app);
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
