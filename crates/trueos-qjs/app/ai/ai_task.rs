#![cfg(feature = "trueos")]

use alloc::string::String;

use embassy_executor::Spawner;

use crate as qjs;

#[derive(Clone)]
pub struct AiInputEntry {
    pub text: String,
    pub web_search: bool,
    pub file_search: bool,
    pub new_conversation: bool,
    pub computer_use: bool,
    pub shell_target_mask: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnsureStartedResult {
    Ready,
    BrowserNotReady,
    SpawnFailed,
}

pub fn queue_ai_input(_next: AiInputEntry) -> bool {
    false
}

pub fn ensure_started(_spawner: &Spawner) -> EnsureStartedResult {
    qjs::trueos_shims::log_info("ai-task: disabled (ai_pc.mjs deleted)\n");
    EnsureStartedResult::SpawnFailed
}

#[embassy_executor::task]
pub async fn run_once() {
    qjs::trueos_shims::log_info("ai-task: disabled (ai_pc.mjs deleted)\n");
}
