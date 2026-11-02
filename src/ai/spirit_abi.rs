//! Spirit presentation ABI, independent of local model inference.

const MAX_SPIRIT_RESPONSE_BYTES: usize = 4 * 1024;
const ERROR_BAD_INPUT: i32 = -3;
const ERROR_UNAVAILABLE: i32 = -5;
const ERROR_TRANSPORT: i32 = -7;

fn validate_silent_spirit_text(turn: u64, text: &[u8]) -> Result<&str, i32> {
    if turn == 0 || text.is_empty() || text.len() > MAX_SPIRIT_RESPONSE_BYTES {
        return Err(ERROR_BAD_INPUT);
    }
    let text = core::str::from_utf8(text).map_err(|_| ERROR_BAD_INPUT)?;
    if text.trim().is_empty() {
        return Err(ERROR_BAD_INPUT);
    }
    Ok(text)
}

/// Present remote or otherwise externally generated text through Spirit's
/// bounded visual ingress.
pub(crate) fn spirit_text_present_silent(turn: u64, text: &[u8]) -> i32 {
    let text = match validate_silent_spirit_text(turn, text) {
        Ok(text) => text,
        Err(error) => return error,
    };
    if !crate::spirit::enqueue_reasoning_response(turn, text) {
        return ERROR_UNAVAILABLE;
    }

    #[cfg(not(feature = "trueos_ttstt"))]
    crate::log_info!(
        target: "r";
        "spirit: voice unavailable reason=ttstt-feature-disabled text_presentation=retained\n"
    );
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_spirit_emotion_play(
    idea_ptr: *const u8,
    idea_len: usize,
) -> i32 {
    if idea_ptr.is_null() || idea_len == 0 || idea_len > 16 {
        return ERROR_BAD_INPUT;
    }
    let idea = unsafe { core::slice::from_raw_parts(idea_ptr, idea_len) };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, data) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_SPIRIT_EMOTION_PLAY,
            0,
            0,
            idea,
            &mut [],
        );
        return if status == trueos_vm::vmcall::STATUS_OK {
            data as i64 as i32
        } else {
            ERROR_TRANSPORT
        };
    }
    let Ok(idea) = core::str::from_utf8(idea) else {
        return ERROR_BAD_INPUT;
    };
    crate::spirit::enqueue_emotion_words(&[idea])
        .map(|_| 0)
        .unwrap_or(ERROR_UNAVAILABLE)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_spirit_text_present_silent(
    turn: u64,
    text_ptr: *const u8,
    text_len: usize,
) -> i32 {
    if text_ptr.is_null() || text_len == 0 || text_len > MAX_SPIRIT_RESPONSE_BYTES {
        return ERROR_BAD_INPUT;
    }
    let text = unsafe { core::slice::from_raw_parts(text_ptr, text_len) };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, data) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_SPIRIT_TEXT_PRESENT_SILENT,
            turn,
            0,
            text,
            &mut [],
        );
        return if status == trueos_vm::vmcall::STATUS_OK {
            data as i64 as i32
        } else {
            ERROR_TRANSPORT
        };
    }
    spirit_text_present_silent(turn, text)
}

pub(crate) fn spirit_move(x_normalized: f32, y_normalized: f32) -> i32 {
    if !x_normalized.is_finite()
        || !y_normalized.is_finite()
        || !(0.0..=1.0).contains(&x_normalized)
        || !(0.0..=1.0).contains(&y_normalized)
    {
        return ERROR_BAD_INPUT;
    }
    crate::spirit::move_spirit_to(x_normalized as f64, y_normalized as f64)
        .map(|_| 0)
        .unwrap_or(ERROR_UNAVAILABLE)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_spirit_move(x_normalized: f32, y_normalized: f32) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, data) = trueos_vm::vmcall::call(
            trueos_vm::vmcall::OP_BP_SPIRIT_MOVE,
            x_normalized.to_bits() as u64,
            y_normalized.to_bits() as u64,
        );
        return if status == trueos_vm::vmcall::STATUS_OK {
            data as i64 as i32
        } else {
            ERROR_TRANSPORT
        };
    }
    spirit_move(x_normalized, y_normalized)
}

