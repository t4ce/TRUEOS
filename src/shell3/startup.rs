//! Startup commands are admitted through the regular visible Shell3 instance pool.

use alloc::{string::String, vec::Vec};

pub(crate) enum Startup {
    GuestShell {
        access_account: Option<trueos_crypto::AccountId>,
    },
    Link {
        source: crate::ui4::Ui4CursorSource,
        anchor: (u32, u32),
    },
    AppDb {
        source: crate::ui4::Ui4CursorSource,
        anchor: (u32, u32),
    },
    Blueprint {
        archive: String,
        bytes: Vec<u8>,
        script: Option<String>,
        access_account: Option<trueos_crypto::AccountId>,
    },
}

impl Startup {
    pub(super) fn guest_account(&self) -> Option<Option<trueos_crypto::AccountId>> {
        match self {
            Self::GuestShell { access_account } | Self::Blueprint { access_account, .. } => {
                Some(*access_account)
            }
            Self::Link { .. } | Self::AppDb { .. } => None,
        }
    }

    /// Desktop entry points retain their placement across first-paint retries.
    pub(super) fn configure_window(&self, shell: &mut super::Shell3) {
        let anchor = match self {
            Self::Link { anchor, .. } | Self::AppDb { anchor, .. } => anchor,
            _ => return,
        };
        let Some((cell_width, cell_height)) =
            crate::r::services::shell3_font_service::cell_extent(super::service::microfont_scale())
        else {
            return;
        };
        let Some((screen_width, screen_height)) = crate::ui4::output_dimensions() else {
            return;
        };
        let width = (super::DEFAULT_COLUMNS as u32)
            .saturating_mul(cell_width)
            .min(screen_width);
        let height = (super::DEFAULT_ROWS as u32)
            .saturating_mul(cell_height)
            .min(screen_height);
        shell.configure_window(super::service::WindowGeometry {
            x: anchor.0.min(screen_width.saturating_sub(width)) as i32,
            y: anchor.1.min(screen_height.saturating_sub(height)) as i32,
            width,
            height,
        });
    }

    pub(super) fn launch(self, shell: &mut super::Shell3) {
        let desktop = match &self {
            Self::Link { source, anchor } | Self::AppDb { source, anchor } => Some((*source, *anchor)),
            _ => None,
        };
        match self {
            Self::GuestShell { .. } => {}
            Self::AppDb { .. } => shell.launch_named_app("AppDB", false),
            Self::Link { .. } => {
                match super::link::start(shell.tui_frontend()) {
                    Ok(()) => shell.enter_native_terminal("link"),
                    Err(error) => super::MatrixSlots::echo(
                        shell.active_matrix_slot.as_deref(),
                        shell.active_matrix_lifetime,
                        error,
                    ),
                }
            }
            Self::Blueprint {
                archive,
                bytes,
                script,
                access_account,
            } => {
                match super::service::launch_bytes_with_script_for_account(
                    archive,
                    bytes,
                    shell.tui_frontend(),
                    script,
                    access_account,
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
        if let Some((source, anchor)) = desktop {
            if let Some(window) = shell.show.window() {
                if let Err(error) = crate::ui4::select_window_for_cursor_at(
                    source,
                    crate::ui4::WindowOwner::SHELL3_SERVICE,
                    window,
                    anchor.0,
                    anchor.1,
                ) {
                    crate::log_warn!(target: "ui4";
                        "ui4: desktop terminal selection failed window={} cursor={}:{}:{} error={:?}\n",
                        window.raw(), source.controller_id, source.slot_id, source.ep_target, error,
                    );
                }
            }
        }
    }
}
