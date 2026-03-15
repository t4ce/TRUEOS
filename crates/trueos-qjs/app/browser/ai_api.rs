#![cfg(feature = "trueos")]

extern crate alloc;

use alloc::string::String;
use core::ffi::c_char;

use crate as qjs;

use super::helpers;

const AI_API_CONTRACT_JSON: &str = r#"{"version":1,"available":["getApiContract","listUnavailable","getWindowId","getHtml","getTextRows","getDomSnapshot","getTrueosFsTreeHtml","setNodeHtml","setBodyHtml","insertHtml","getViewport","paint","setScroll","navigate","keyboard","typeText","pressKey","captureScreenshot"],"unavailable":["moveCursor","click"],"notes":{"intent":"Worker-facing browser contract for the AI task. Keep this surface explicit so agent logic remains isolated from the browser VM.","targetShape":"Close to future computer-use style APIs while still reflecting TRUEOS capabilities today.","domSnapshotShape":"Returns a rooted tree object; use snap.nodes for a flat compatibility index.","clickShape":"click() and moveCursor() are ui2/kernel-owned input paths, not browser-runtime methods.","keyboardShape":"keyboard(...) accepts Unicode text entries and strict key entries with optional modifiers; pressKey(...) and typeText(...) compile into that canonical event list."}}"#;

pub unsafe fn install_globals(ctx: *mut qjs::JSContext) -> bool {
    let mut src = String::new();
    src.push_str("(function(G){if(!G)return;G.__trueosBrowserAiApiContract=");
    src.push_str(AI_API_CONTRACT_JSON);
    src.push_str(";G.__trueosBrowserAiApiUnavailableCode='TRUEOS_BROWSER_API_UNAVAILABLE';})(typeof globalThis !== 'undefined' ? globalThis : this);");

    helpers::eval_or_log(
        ctx,
        src.as_bytes(),
        b"<browser-ai-api>\0".as_ptr() as *const c_char,
        qjs::JS_EVAL_TYPE_GLOBAL,
        "browser ai api",
    )
}
