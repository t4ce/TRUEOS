use alloc::vec::Vec;
use core::str::SplitWhitespace;

use spin::Mutex;

use super::super::{
    MatrixTarget, ShellBackend2, print_matrix_target_native_line, print_native_line,
    print_shell_line,
};
use crate::shell2::CommandSessionInputResult;
use crate::shell2::ecma48;
use crate::shell2::matrix::MatrixSlotId;
use crate::shell2::shell2_cmd::{CommandSessionKind, ParseOutcome};

const AMPLE_MENU_ROWS: [[&str; 2]; 8] = [
    ["help", "Show Ample commands"],
    ["look", "Describe the current room"],
    ["north", "Walk toward the north door"],
    ["south", "Walk back to the terminal bench"],
    ["inventory", "Show carried items"],
    ["take badge", "Pick up the visitor badge"],
    ["use badge", "Swipe the visitor badge"],
    ["quit", "Exit Ample and return to shell2"],
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum AmplePhase {
    Selecting,
    Playing,
}

#[derive(Clone)]
struct AmpleSession {
    slot_id: MatrixSlotId,
    phase: AmplePhase,
}

static AMPLE_SESSIONS: Mutex<Vec<AmpleSession>> = Mutex::new(Vec::new());

const AMPLE_HIGHLIGHT_RGB: (u8, u8, u8) = (255, 255, 0);
const AMPLE_NPC_RGB: (u8, u8, u8) = (50, 200, 50);
const AMPLE_DESC_RGB: (u8, u8, u8) = (102, 208, 250);

fn styled_heading(text: &str) -> alloc::string::String {
    alloc::format!("{}", ecma48::style(text).fg(AMPLE_HIGHLIGHT_RGB))
}

fn styled_world_title(text: &str) -> alloc::string::String {
    alloc::format!("{}", ecma48::style(text).fg(AMPLE_HIGHLIGHT_RGB))
}

fn styled_author(text: &str) -> alloc::string::String {
    alloc::format!("{}", ecma48::style(text).fg(AMPLE_NPC_RGB).underline())
}

fn styled_description(text: &str) -> alloc::string::String {
    alloc::format!("{}", ecma48::style(text).fg(AMPLE_DESC_RGB))
}

fn print_usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "ample: usage `ample`");
}

fn set_phase(slot_id: &MatrixSlotId, phase: AmplePhase) {
    let mut sessions = AMPLE_SESSIONS.lock();
    if let Some(session) = sessions
        .iter_mut()
        .find(|session| session.slot_id == *slot_id)
    {
        session.phase = phase;
        return;
    }
    sessions.push(AmpleSession {
        slot_id: slot_id.clone(),
        phase,
    });
}

fn phase_for(slot_id: &MatrixSlotId) -> AmplePhase {
    AMPLE_SESSIONS
        .lock()
        .iter()
        .find(|session| session.slot_id == *slot_id)
        .map(|session| session.phase)
        .unwrap_or(AmplePhase::Selecting)
}

fn clear_session(slot_id: &MatrixSlotId) {
    let mut sessions = AMPLE_SESSIONS.lock();
    if let Some(idx) = sessions
        .iter()
        .position(|session| session.slot_id == *slot_id)
    {
        let _ = sessions.remove(idx);
    }
}

fn print_help_to_target(target: &MatrixTarget) {
    for row in AMPLE_MENU_ROWS {
        let line = alloc::format!("{:<12} {}", row[0], row[1]);
        print_matrix_target_native_line(target, line.as_str());
    }
}

fn print_intro(target: &MatrixTarget) {
    print_matrix_target_native_line(target, "Ample 0: text mode boot");
    print_matrix_target_native_line(target, "You are standing in a clean white test chamber.");
    print_matrix_target_native_line(
        target,
        "A visitor badge rests on a terminal bench beside the north door.",
    );
    print_matrix_target_native_line(
        target,
        "Type `help` for commands. Type `quit` to return to shell2.",
    );
}

fn print_startup_menu(io: &'static dyn ShellBackend2) {
    let choose = styled_heading("Choose a world or save:");
    print_native_line(io, choose.as_str());
    let worlds = alloc::format!("{}", ecma48::style("Worlds").bold());
    print_native_line(io, worlds.as_str());
    let world1 = alloc::format!(
        "  1. {} (by {}, v0.67.0-alpha)",
        styled_world_title("AMBLE: An Absurd Adventure"),
        styled_author("Dave")
    );
    print_native_line(io, world1.as_str());
    let desc1a =
        styled_description("      A surreal sci-fi comedy adventure about cake, curiosity,");
    let desc1b = styled_description("      and improbable facilities.");
    print_native_line(io, desc1a.as_str());
    print_native_line(io, desc1b.as_str());
    let world2 = alloc::format!(
        "  2. {} (by {}, v0.66.0)",
        styled_world_title("AMBLE: An Absurd Adventure"),
        styled_author("Dave")
    );
    print_native_line(io, world2.as_str());
    let desc2a =
        styled_description("      A surreal sci-fi comedy adventure about cake, curiosity,");
    let desc2b = styled_description("      and improbable facilities.");
    print_native_line(io, desc2a.as_str());
    print_native_line(io, desc2b.as_str());
    let world3 = alloc::format!(
        "  3. {} (by {}, v0.1.0)",
        styled_world_title("Hospital Game TBD"),
        styled_author("pygmy-twylyte")
    );
    print_native_line(io, world3.as_str());
    let desc3 =
        styled_description("      A mystery game set in a nearly abandoned small hospital.");
    print_native_line(io, desc3.as_str());
    print_native_line(io, "");
    print_native_line(io, "Select a world or save [1-3] (Enter=1): ");
}

pub(crate) fn try_parse(
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    if args.next().is_some() {
        print_usage(io);
        return ParseOutcome::Handled;
    }

    let slot_id =
        crate::shell2::matrix::active_slot_id(crate::shell2::output_target_for_backend(io));
    set_phase(&slot_id, AmplePhase::Selecting);
    print_startup_menu(io);
    ParseOutcome::StartSession(CommandSessionKind::Ample)
}

pub(crate) fn handle_session_input(
    target: &MatrixTarget,
    submitted: &str,
) -> CommandSessionInputResult {
    match phase_for(&target.slot_id) {
        AmplePhase::Selecting => {
            let trimmed = submitted.trim();
            let selected = if trimmed.is_empty() { "1" } else { trimmed };
            match selected {
                "1" | "2" | "3" => {
                    set_phase(&target.slot_id, AmplePhase::Playing);
                    print_matrix_target_native_line(
                        target,
                        alloc::format!("ample: selected world {}", selected).as_str(),
                    );
                    print_intro(target);
                    CommandSessionInputResult::KeepRunning
                }
                _ => {
                    print_matrix_target_native_line(
                        target,
                        "Invalid selection. Please enter a number between 1 and 3.",
                    );
                    print_matrix_target_native_line(
                        target,
                        "Select a world or save [1-3] (Enter=1): ",
                    );
                    CommandSessionInputResult::KeepRunning
                }
            }
        }
        AmplePhase::Playing => handle_game_input(target, submitted),
    }
}

fn handle_game_input(target: &MatrixTarget, submitted: &str) -> CommandSessionInputResult {
    let trimmed = submitted.trim();
    if trimmed.is_empty() {
        print_matrix_target_native_line(target, "ample: type `help`");
        return CommandSessionInputResult::KeepRunning;
    }

    if trimmed.eq_ignore_ascii_case("quit")
        || trimmed.eq_ignore_ascii_case("exit")
        || trimmed.eq_ignore_ascii_case("q")
    {
        clear_session(&target.slot_id);
        print_matrix_target_native_line(target, "ample: session closed");
        return CommandSessionInputResult::CompleteIdle;
    }

    if trimmed.eq_ignore_ascii_case("help") || trimmed == "?" {
        print_matrix_target_native_line(target, "ample: available commands");
        print_help_to_target(target);
        return CommandSessionInputResult::KeepRunning;
    }

    if trimmed.eq_ignore_ascii_case("look") {
        print_matrix_target_native_line(
            target,
            "ample: the chamber is bright, quiet, and suspiciously polite.",
        );
        print_matrix_target_native_line(
            target,
            "ample: north leads to a locked door; south returns to the shell prompt wall.",
        );
        return CommandSessionInputResult::KeepRunning;
    }

    if trimmed.eq_ignore_ascii_case("inventory") || trimmed.eq_ignore_ascii_case("inv") {
        print_matrix_target_native_line(target, "ample: inventory -> visitor badge, dry humor");
        return CommandSessionInputResult::KeepRunning;
    }

    if trimmed.eq_ignore_ascii_case("north") || trimmed.eq_ignore_ascii_case("go north") {
        print_matrix_target_native_line(
            target,
            "ample: the north door blinks amber and stays locked.",
        );
        print_matrix_target_native_line(target, "ample: maybe the badge reader wants attention.");
        return CommandSessionInputResult::KeepRunning;
    }

    if trimmed.eq_ignore_ascii_case("south") || trimmed.eq_ignore_ascii_case("go south") {
        print_matrix_target_native_line(target, "ample: you drift back toward the terminal bench.");
        return CommandSessionInputResult::KeepRunning;
    }

    if trimmed.eq_ignore_ascii_case("take badge") || trimmed.eq_ignore_ascii_case("get badge") {
        print_matrix_target_native_line(target, "ample: you pick up the visitor badge.");
        return CommandSessionInputResult::KeepRunning;
    }

    if trimmed.eq_ignore_ascii_case("use badge") || trimmed.eq_ignore_ascii_case("swipe badge") {
        print_matrix_target_native_line(
            target,
            "ample: the badge reader chirps once and turns green.",
        );
        print_matrix_target_native_line(
            target,
            "ample: omega simple success. The door is now unlocked in spirit.",
        );
        return CommandSessionInputResult::KeepRunning;
    }

    print_matrix_target_native_line(
        target,
        alloc::format!("ample: unknown command `{}`", trimmed).as_str(),
    );
    print_matrix_target_native_line(target, "ample: type `help`");
    CommandSessionInputResult::KeepRunning
}
