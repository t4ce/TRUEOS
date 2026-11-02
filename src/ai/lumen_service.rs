//! Retired Lumen ABI compatibility boundary.
//!
//! Local inference is unavailable until its replacement is implemented. Keep
//! existing Blueprint imports resolvable, without queues, model allocations,
//! session state, workers, or boot warmup. Spirit presentation lives separately.

const ERROR_UNAVAILABLE: i32 = -5;

pub(crate) fn template_open(_owner: u8, _system: &[u8]) -> i32 {
    ERROR_UNAVAILABLE
}

pub(crate) fn prompt_submit(_owner: u8, _turn: u64, _tail: &[u32], _prompt: &[u8]) -> i32 {
    ERROR_UNAVAILABLE
}

pub(crate) fn prompt_submit_with_no_argument_tool(
    _owner: u8,
    _turn: u64,
    _tail: &[u32],
    _prompt: &[u8],
    _tool_name: &[u8],
) -> i32 {
    ERROR_UNAVAILABLE
}

pub(crate) fn tool_result_submit(_owner: u8, _turn: u64, _tail: &[u32], _result: &[u8]) -> i32 {
    ERROR_UNAVAILABLE
}

pub(crate) fn status(_owner: u8) -> Option<v::bp_abi::TrueosLumenStatus> {
    Some(v::bp_abi::TrueosLumenStatus {
        phase: v::bp_abi::LUMEN_PHASE_ERROR,
        error: ERROR_UNAVAILABLE,
        position: 0,
        reply_len: 0,
        checkpoint_len: 0,
        reply_tail: [0; 2],
        reply_tail_len: 0,
        reserved: 0,
    })
}

pub(crate) fn reply_read(_owner: u8, _out: &mut [u8]) -> isize {
    ERROR_UNAVAILABLE as isize
}

pub(crate) fn checkpoint_request(_owner: u8) -> i32 {
    ERROR_UNAVAILABLE
}

pub(crate) fn checkpoint_read(_owner: u8, _offset: usize, _out: &mut [u8]) -> isize {
    ERROR_UNAVAILABLE as isize
}

pub(crate) fn restore_begin(_owner: u8, _total_len: usize) -> i32 {
    ERROR_UNAVAILABLE
}

pub(crate) fn restore_write(_owner: u8, _offset: usize, _data: &[u8]) -> i32 {
    ERROR_UNAVAILABLE
}

pub(crate) fn restore_commit(_owner: u8) -> i32 {
    ERROR_UNAVAILABLE
}

pub(crate) fn close(_owner: u8) -> i32 {
    0
}

pub(crate) fn spirit_response_present(_owner: u8, _turn: u64, _text: &[u8]) -> i32 {
    ERROR_UNAVAILABLE
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_lumen_template_open(
    _system_ptr: *const u8,
    _system_len: usize,
) -> i32 {
    ERROR_UNAVAILABLE
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_lumen_prompt_submit(
    _turn: u64,
    _tail_ptr: *const u32,
    _tail_len: usize,
    _prompt_ptr: *const u8,
    _prompt_len: usize,
) -> i32 {
    ERROR_UNAVAILABLE
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_lumen_prompt_submit_with_no_argument_tool_v1(
    _turn: u64,
    _tail_ptr: *const u32,
    _tail_len: usize,
    _prompt_ptr: *const u8,
    _prompt_len: usize,
    _tool_name_ptr: *const u8,
    _tool_name_len: usize,
) -> i32 {
    ERROR_UNAVAILABLE
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_lumen_tool_result_submit(
    _turn: u64,
    _tail_ptr: *const u32,
    _tail_len: usize,
    _result_ptr: *const u8,
    _result_len: usize,
) -> i32 {
    ERROR_UNAVAILABLE
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_lumen_status(_out: *mut v::bp_abi::TrueosLumenStatus) -> i32 {
    ERROR_UNAVAILABLE
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_lumen_reply_read(_out_ptr: *mut u8, _out_cap: usize) -> isize {
    ERROR_UNAVAILABLE as isize
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_lumen_checkpoint_request() -> i32 {
    ERROR_UNAVAILABLE
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_lumen_checkpoint_read(
    _offset: usize,
    _out_ptr: *mut u8,
    _out_cap: usize,
) -> isize {
    ERROR_UNAVAILABLE as isize
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_lumen_restore_begin(_total_len: usize) -> i32 {
    ERROR_UNAVAILABLE
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_lumen_restore_write(
    _offset: usize,
    _data_ptr: *const u8,
    _data_len: usize,
) -> i32 {
    ERROR_UNAVAILABLE
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_lumen_restore_commit() -> i32 {
    ERROR_UNAVAILABLE
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_lumen_close() -> i32 {
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_spirit_response_present(
    _turn: u64,
    _text_ptr: *const u8,
    _text_len: usize,
) -> i32 {
    ERROR_UNAVAILABLE
}

