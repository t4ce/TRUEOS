extern crate alloc;

use core::ffi::CStr;
use core::ffi::{c_char, c_int, c_long, c_void};
use core::ptr;

unsafe extern "C" {
    fn trueos_cabi_write(stream: u32, bytes: *const u8, len: usize);
    pub fn trueos_cabi_poll_once();
    fn trueos_cabi_boot_timestamp_secs() -> u64;
    fn trueos_cabi_alloc(size: usize) -> *mut u8;
    fn trueos_cabi_calloc(nmemb: usize, size: usize) -> *mut u8;
    fn trueos_cabi_free(ptr: *mut u8);
    fn trueos_cabi_realloc(ptr: *mut u8, size: usize) -> *mut u8;
    fn trueos_cabi_malloc_usable_size(ptr: *const u8) -> usize;
    pub fn trueos_cabi_fs_read_file(
        path_ptr: *const u8,
        path_len: usize,
        out_ptr: *mut u8,
        out_cap: usize,
    ) -> isize;
    pub fn trueos_cabi_fs_write_begin(
        path_ptr: *const u8,
        path_len: usize,
        total_len: u64,
        out_handle: *mut u32,
    ) -> i32;
    pub fn trueos_cabi_fs_write_chunk(handle: u32, data_ptr: *const u8, data_len: usize) -> i32;
    pub fn trueos_cabi_fs_write_finish(handle: u32) -> i32;
    pub fn trueos_cabi_fs_write_abort(handle: u32) -> i32;
    pub fn trueos_cabi_trueosfs_primary_html_tree(
        max_entries: u32,
        out_ptr: *mut u8,
        out_cap: usize,
    ) -> isize;
    pub fn trueos_cabi_fs_remove(path_ptr: *const u8, path_len: usize) -> i32;
    pub fn trueos_cabi_net_fetch_start(
        url_ptr: *const u8,
        url_len: usize,
        path_ptr: *const u8,
        path_len: usize,
    ) -> u32;
    pub fn trueos_cabi_net_fetch_post_json_start(
        url_ptr: *const u8,
        url_len: usize,
        path_ptr: *const u8,
        path_len: usize,
        body_ptr: *const u8,
        body_len: usize,
        bearer_ptr: *const u8,
        bearer_len: usize,
    ) -> u32;
    pub fn trueos_cabi_net_fetch_result(op_id: u32) -> i32;
    pub fn trueos_cabi_net_fetch_discard(op_id: u32) -> i32;
    pub fn trueos_cabi_net_fetch_wait(op_id: u32, timeout_ms: u64) -> i32;
    pub fn trueos_cabi_input_pop_mouse(
        out_buttons: *mut u8,
        out_dx: *mut i8,
        out_dy: *mut i8,
        out_wheel: *mut i8,
    ) -> i32;
    pub fn trueos_cabi_input_cursor_pos(cursor_id: u32, out_x: *mut i32, out_y: *mut i32) -> i32;
    pub fn trueos_cabi_input_cursor_buttons(cursor_id: u32, out_buttons_down: *mut u32) -> i32;
    pub fn trueos_cabi_input_read_cursor_events_since(
        read_seq: u64,
        out: *mut TrueosHidCursorEvent,
        out_cap: u32,
        out_next_seq: *mut u64,
        out_dropped: *mut u32,
    ) -> u32;
    pub fn trueos_cabi_input_write_cursor(
        slot_id: u32,
        x: i32,
        y: i32,
        buttons_down: u32,
        wheel: i32,
        flags: u32,
    ) -> i32;
    pub fn trueos_cabi_uart1_shell_write(data_ptr: *const u8, data_len: usize) -> usize;
    pub fn trueos_cabi_shell1_submit_input(data_ptr: *const u8, data_len: usize) -> usize;
    pub fn trueos_cabi_shell1_command_registry_json(out_ptr: *mut u8, out_cap: usize) -> isize;
    pub fn trueos_cabi_shell_qjs_init();
    pub fn trueos_cabi_shell_qjs_write(data_ptr: *const u8, data_len: usize) -> usize;
    pub fn trueos_cabi_shell_qjs_write_byte(byte: u8) -> i32;
    pub fn trueos_cabi_shell_qjs_read(out_ptr: *mut u8, out_cap: usize) -> isize;
    pub fn trueos_cabi_shell_qjs_read_byte() -> i32;

    pub fn trueos_cabi_mouse_poll(out: *mut TrueosMouseState) -> i32;
    pub fn trueos_cabi_qjs_mouse_pop(out: *mut TrueosMouseState) -> i32;
    pub fn trueos_cabi_gfx_capture_screenshot_data_url(out_ptr: *mut u8, out_cap: usize) -> isize;
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueosMouseState {
    pub x: i32,
    pub y: i32,
    pub dx: i32,
    pub dy: i32,
    pub wheel: i32,
    pub buttons: u32,
    pub seq: u32,
    pub slot_id: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueosHidCursorEvent {
    pub t_ms: u32,
    pub seq: u32,
    pub controller_id: u32,
    pub slot_id: u32,
    pub ep_target: u32,
    pub hid_kind: u8,
    pub reserved0: u8,
    pub reserved1: u16,
    pub buttons_down: u32,
    pub wheel: i16,
    pub reserved2: u16,
    pub x: f64,
    pub y: f64,
    pub flags: u32,
}

#[inline]
fn log_bytes(bytes: &[u8]) {
    unsafe { trueos_cabi_write(2, bytes.as_ptr(), bytes.len()) }
}

#[inline]
pub fn write_log_stream(stream: u32, s: &str) {
    if s.is_empty() {
        return;
    }
    unsafe { trueos_cabi_write(stream, s.as_ptr(), s.len()) }
}

#[inline]
pub fn log_info(s: &str) {
    write_log_stream(1, s);
}

#[inline]
pub fn log_error(s: &str) {
    write_log_stream(2, s);
}

#[inline]
pub fn uart1_shell_write(bytes: &[u8]) -> usize {
    if bytes.is_empty() {
        return 0;
    }
    unsafe { trueos_cabi_uart1_shell_write(bytes.as_ptr(), bytes.len()) }
}

#[inline]
pub fn shell1_submit_input(bytes: &[u8]) -> usize {
    if bytes.is_empty() {
        return 0;
    }
    unsafe { trueos_cabi_shell1_submit_input(bytes.as_ptr(), bytes.len()) }
}

#[inline]
pub fn shell1_command_registry_json() -> Option<alloc::string::String> {
    let len = unsafe { trueos_cabi_shell1_command_registry_json(core::ptr::null_mut(), 0) };
    if len <= 0 {
        return None;
    }

    let mut bytes = alloc::vec![0u8; len as usize];
    let got = unsafe { trueos_cabi_shell1_command_registry_json(bytes.as_mut_ptr(), bytes.len()) };
    if got <= 0 {
        return None;
    }
    bytes.truncate(got as usize);
    alloc::string::String::from_utf8(bytes).ok()
}

#[inline]
pub fn shell_qjs_init() {
    unsafe { trueos_cabi_shell_qjs_init() }
}

#[inline]
pub fn shell_qjs_write(bytes: &[u8]) -> usize {
    if bytes.is_empty() {
        return 0;
    }
    unsafe { trueos_cabi_shell_qjs_write(bytes.as_ptr(), bytes.len()) }
}

#[inline]
pub fn shell_qjs_write_byte(byte: u8) -> bool {
    unsafe { trueos_cabi_shell_qjs_write_byte(byte) == 0 }
}

#[inline]
pub fn shell_qjs_read(out: &mut [u8]) -> usize {
    if out.is_empty() {
        return 0;
    }
    let got = unsafe { trueos_cabi_shell_qjs_read(out.as_mut_ptr(), out.len()) };
    if got <= 0 {
        0
    } else {
        got as usize
    }
}

#[inline]
pub fn shell_qjs_read_byte() -> Option<u8> {
    let v = unsafe { trueos_cabi_shell_qjs_read_byte() };
    if (0..=255).contains(&v) {
        Some(v as u8)
    } else {
        None
    }
}

#[inline]
pub fn gfx_capture_screenshot_data_url() -> Option<alloc::string::String> {
    let len = unsafe { trueos_cabi_gfx_capture_screenshot_data_url(core::ptr::null_mut(), 0) };
    if len <= 0 {
        return None;
    }

    let mut bytes = alloc::vec![0u8; len as usize];
    let got = unsafe { trueos_cabi_gfx_capture_screenshot_data_url(bytes.as_mut_ptr(), bytes.len()) };
    if got <= 0 {
        return None;
    }
    bytes.truncate(got as usize);
    alloc::string::String::from_utf8(bytes).ok()
}

#[inline]
fn log_str(s: &str) {
    log_bytes(s.as_bytes())
}

#[inline]
fn log_cstr(ptr: *const c_char) {
    if ptr.is_null() {
        log_str("<null>");
        return;
    }
    // If the pointer is invalid, this will still fault, but in practice this is
    // what we want for debugging: see the actual assert payload.
    let bytes = unsafe { CStr::from_ptr(ptr).to_bytes() };
    log_bytes(bytes);
}

#[inline]
fn log_i32_dec(v: c_int) {
    let mut n = v as i64;
    if n == 0 {
        log_str("0");
        return;
    }
    if n < 0 {
        log_str("-");
        n = -n;
    }
    let mut buf = [0u8; 16];
    let mut i = buf.len();
    let mut x = n as u64;
    while x != 0 {
        i -= 1;
        buf[i] = b'0' + (x % 10) as u8;
        x /= 10;
    }
    log_bytes(&buf[i..]);
}

// --- Abort/assert shims ---

#[unsafe(no_mangle)]
pub unsafe extern "C" fn abort() -> ! {
    log_str("abort()\n");
    core::arch::asm!("cli", options(nomem, nostack));
    loop {
        core::arch::asm!("hlt", options(nomem, nostack));
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __assert_fail(
    assertion: *const c_char,
    file: *const c_char,
    line: c_int,
    function: *const c_char,
) -> ! {
    log_str("__assert_fail(assertion='");
    log_cstr(assertion);
    log_str("' file='");
    log_cstr(file);
    log_str("' line=");
    log_i32_dec(line);
    log_str(" function='");
    log_cstr(function);
    log_str("')\n");
    abort()
}

// --- Small math shims QuickJS expects from libm/libc ---

#[unsafe(no_mangle)]
pub unsafe extern "C" fn abs(x: c_int) -> c_int {
    if x < 0 { -x } else { x }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lrint(x: f64) -> c_long {
    let v = libm::rint(x);
    v as c_long
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn modf(x: f64, iptr: *mut f64) -> f64 {
    let int_part = libm::trunc(x);
    if !iptr.is_null() {
        *iptr = int_part;
    }
    x - int_part
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn acosh(x: f64) -> f64 {
    libm::acosh(x)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn asinh(x: f64) -> f64 {
    libm::asinh(x)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn atanh(x: f64) -> f64 {
    libm::atanh(x)
}

const CLOCK_REALTIME: c_int = 0;
const CLOCK_MONOTONIC: c_int = 1;

#[repr(C)]
pub struct TimeVal {
    tv_sec: i64,
    tv_usec: i64,
}

#[repr(C)]
pub struct TimeSpec {
    tv_sec: i64,
    tv_nsec: i64,
}

#[repr(C)]
pub struct Tm {
    tm_sec: c_int,
    tm_min: c_int,
    tm_hour: c_int,
    tm_mday: c_int,
    tm_mon: c_int,
    tm_year: c_int,
    tm_wday: c_int,
    tm_yday: c_int,
    tm_isdst: c_int,
}

#[inline]
fn uptime_secs_and_subsec_micros() -> (u64, u32) {
    let ticks = embassy_time_driver::now();
    let hz = embassy_time_driver::TICK_HZ as u64;
    let secs = ticks / hz;
    let sub = (ticks % hz) as u64;
    let micros = (sub * 1_000_000u64 / hz) as u32;
    (secs, micros)
}

#[inline]
fn realtime_secs_and_subsec_micros() -> (u64, u32) {
    let (up_secs, up_micros) = uptime_secs_and_subsec_micros();
    let base = unsafe { trueos_cabi_boot_timestamp_secs() };
    (base.saturating_add(up_secs), up_micros)
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

fn month_lengths(year: i64) -> [i64; 12] {
    if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    }
}

fn unix_timestamp_to_ymdhms(ts: i64) -> (i64, i64, i64, i64, i64, i64, i64, i64) {
    const SECS_PER_MIN: i64 = 60;
    const SECS_PER_HOUR: i64 = 60 * SECS_PER_MIN;
    const SECS_PER_DAY: i64 = 24 * SECS_PER_HOUR;

    let mut days = ts / SECS_PER_DAY;
    let mut rem = ts % SECS_PER_DAY;
    if rem < 0 {
        rem += SECS_PER_DAY;
        days -= 1;
    }

    let hour = rem / SECS_PER_HOUR;
    rem %= SECS_PER_HOUR;
    let min = rem / SECS_PER_MIN;
    let sec = rem % SECS_PER_MIN;

    let mut year: i64 = 1970;
    let mut yday: i64 = days;
    while yday < 0 {
        year -= 1;
        yday += if is_leap_year(year) { 366 } else { 365 };
    }
    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if yday < days_in_year {
            break;
        }
        yday -= days_in_year;
        year += 1;
    }

    let month_lengths = month_lengths(year);
    let mut month = 0i64;
    let mut day_in_year = yday;
    while month < 12 {
        let len = month_lengths[month as usize];
        if day_in_year < len {
            break;
        }
        day_in_year -= len;
        month += 1;
    }
    let mday = day_in_year + 1;
    let wday = ((days + 4).rem_euclid(7)) as i64; // 1970-01-01 = Thursday

    (year, month, mday, hour, min, sec, wday, yday)
}

fn ymdhms_to_unix_timestamp(
    year: i64,
    month0: i64,
    mday: i64,
    hour: i64,
    min: i64,
    sec: i64,
) -> Option<i64> {
    if month0 < 0 || month0 > 11 || mday < 1 || mday > 31 {
        return None;
    }
    let mut days: i64 = 0;
    if year >= 1970 {
        let mut y = 1970;
        while y < year {
            days += if is_leap_year(y) { 366 } else { 365 };
            y += 1;
        }
    } else {
        let mut y = year;
        while y < 1970 {
            days -= if is_leap_year(y) { 366 } else { 365 };
            y += 1;
        }
    }
    let month_lengths = month_lengths(year);
    for i in 0..month0 {
        days += month_lengths[i as usize];
    }
    days += mday - 1;

    let secs = days * 86400 + hour * 3600 + min * 60 + sec;
    Some(secs)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn malloc(size: usize) -> *mut c_void {
    trueos_cabi_alloc(size) as *mut c_void
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn calloc(nmemb: usize, size: usize) -> *mut c_void {
    trueos_cabi_calloc(nmemb, size) as *mut c_void
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn free(ptr: *mut c_void) {
    trueos_cabi_free(ptr as *mut u8)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn realloc(ptr: *mut c_void, size: usize) -> *mut c_void {
    trueos_cabi_realloc(ptr as *mut u8, size) as *mut c_void
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn malloc_usable_size(ptr: *const c_void) -> usize {
    trueos_cabi_malloc_usable_size(ptr as *const u8)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcpy(dest: *mut c_void, src: *const c_void, n: usize) -> *mut c_void {
    if n == 0 || dest == src as *mut c_void {
        return dest;
    }

    core::arch::asm!(
        "cld",
        "rep movsb",
        inout("rcx") n => _,
        inout("rdi") dest as *mut u8 => _,
        inout("rsi") src as *const u8 => _,
        options(nostack)
    );

    dest
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memmove(dest: *mut c_void, src: *const c_void, n: usize) -> *mut c_void {
    if n == 0 || dest == src as *mut c_void {
        return dest;
    }

    let dest_u8 = dest as *mut u8;
    let src_u8 = src as *const u8;

    if (dest_u8 as usize) < (src_u8 as usize) || (dest_u8 as usize) >= (src_u8 as usize + n) {
        core::arch::asm!(
            "cld",
            "rep movsb",
            inout("rcx") n => _,
            inout("rdi") dest_u8 => _,
            inout("rsi") src_u8 => _,
            options(nostack)
        );
    } else {
        let d = dest_u8.add(n - 1);
        let s = src_u8.add(n - 1);
        core::arch::asm!(
            "std",
            "rep movsb",
            "cld",
            inout("rcx") n => _,
            inout("rdi") d => _,
            inout("rsi") s => _,
            options(nostack)
        );
    }

    dest
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memset(s: *mut c_void, c: c_int, n: usize) -> *mut c_void {
    if n == 0 {
        return s;
    }

    core::arch::asm!(
        "cld",
        "rep stosb",
        in("al") (c as u8),
        inout("rcx") n => _,
        inout("rdi") s as *mut u8 => _,
        options(nostack)
    );

    s
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcmp(a: *const c_void, b: *const c_void, n: usize) -> c_int {
    let a = a as *const u8;
    let b = b as *const u8;
    for i in 0..n {
        let av = *a.add(i);
        let bv = *b.add(i);
        if av != bv {
            return av as c_int - bv as c_int;
        }
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memchr(s: *const c_void, c: c_int, n: usize) -> *mut c_void {
    let s = s as *const u8;
    let needle = c as u8;
    for i in 0..n {
        if *s.add(i) == needle {
            return s.add(i) as *mut c_void;
        }
    }
    ptr::null_mut()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strlen(s: *const c_char) -> usize {
    if s.is_null() {
        return 0;
    }
    let mut len = 0usize;
    while *s.add(len) != 0 {
        len += 1;
    }
    len
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strcmp(a: *const c_char, b: *const c_char) -> c_int {
    let mut i = 0usize;
    loop {
        let av = *a.add(i) as u8;
        let bv = *b.add(i) as u8;
        if av != bv {
            return av as c_int - bv as c_int;
        }
        if av == 0 {
            return 0;
        }
        i += 1;
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strncmp(a: *const c_char, b: *const c_char, n: usize) -> c_int {
    let mut i = 0usize;
    while i < n {
        let av = *a.add(i) as u8;
        let bv = *b.add(i) as u8;
        if av != bv {
            return av as c_int - bv as c_int;
        }
        if av == 0 {
            return 0;
        }
        i += 1;
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strchr(s: *const c_char, c: c_int) -> *mut c_char {
    let mut i = 0usize;
    let needle = c as u8;
    loop {
        let v = *s.add(i) as u8;
        if v == needle {
            return s.add(i) as *mut c_char;
        }
        if v == 0 {
            return ptr::null_mut();
        }
        i += 1;
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strrchr(s: *const c_char, c: c_int) -> *mut c_char {
    let mut last: *mut c_char = ptr::null_mut();
    let mut i = 0usize;
    let needle = c as u8;
    loop {
        let v = *s.add(i) as u8;
        if v == needle {
            last = s.add(i) as *mut c_char;
        }
        if v == 0 {
            return last;
        }
        i += 1;
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gettimeofday(tv: *mut TimeVal, _tz: *mut c_void) -> c_int {
    if tv.is_null() {
        return -1;
    }
    let (secs, micros) = realtime_secs_and_subsec_micros();
    (*tv).tv_sec = secs as i64;
    (*tv).tv_usec = micros as i64;
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn clock_gettime(clk_id: c_int, tp: *mut TimeSpec) -> c_int {
    if tp.is_null() {
        return -1;
    }
    match clk_id {
        CLOCK_REALTIME => {
            let (secs, micros) = realtime_secs_and_subsec_micros();
            (*tp).tv_sec = secs as i64;
            (*tp).tv_nsec = (micros as i64) * 1000;
            0
        }
        CLOCK_MONOTONIC => {
            let (secs, micros) = uptime_secs_and_subsec_micros();
            (*tp).tv_sec = secs as i64;
            (*tp).tv_nsec = (micros as i64) * 1000;
            0
        }
        _ => -1,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn time(tloc: *mut i64) -> i64 {
    let (secs, _) = realtime_secs_and_subsec_micros();
    if !tloc.is_null() {
        *tloc = secs as i64;
    }
    secs as i64
}

static mut TM_BUF: Tm = Tm {
    tm_sec: 0,
    tm_min: 0,
    tm_hour: 0,
    tm_mday: 1,
    tm_mon: 0,
    tm_year: 70,
    tm_wday: 4,
    tm_yday: 0,
    tm_isdst: 0,
};

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gmtime(timep: *const i64) -> *mut Tm {
    if timep.is_null() {
        return ptr::null_mut();
    }
    let t = *timep;
    let (year, month, mday, hour, min, sec, wday, yday) = unix_timestamp_to_ymdhms(t);
    TM_BUF = Tm {
        tm_sec: sec as c_int,
        tm_min: min as c_int,
        tm_hour: hour as c_int,
        tm_mday: mday as c_int,
        tm_mon: month as c_int,
        tm_year: (year - 1900) as c_int,
        tm_wday: wday as c_int,
        tm_yday: yday as c_int,
        tm_isdst: 0,
    };
    &raw mut TM_BUF as *mut Tm
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn gmtime_r(timep: *const i64, result: *mut Tm) -> *mut Tm {
    if timep.is_null() || result.is_null() {
        return ptr::null_mut();
    }
    let t = *timep;
    let (year, month, mday, hour, min, sec, wday, yday) = unix_timestamp_to_ymdhms(t);
    *result = Tm {
        tm_sec: sec as c_int,
        tm_min: min as c_int,
        tm_hour: hour as c_int,
        tm_mday: mday as c_int,
        tm_mon: month as c_int,
        tm_year: (year - 1900) as c_int,
        tm_wday: wday as c_int,
        tm_yday: yday as c_int,
        tm_isdst: 0,
    };
    result
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn localtime(timep: *const i64) -> *mut Tm {
    gmtime(timep)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn localtime_r(timep: *const i64, result: *mut Tm) -> *mut Tm {
    gmtime_r(timep, result)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn mktime(tm: *mut Tm) -> i64 {
    if tm.is_null() {
        return -1;
    }
    let year = (*tm).tm_year as i64 + 1900;
    let month0 = (*tm).tm_mon as i64;
    let mday = (*tm).tm_mday as i64;
    let hour = (*tm).tm_hour as i64;
    let min = (*tm).tm_min as i64;
    let sec = (*tm).tm_sec as i64;

    match ymdhms_to_unix_timestamp(year, month0, mday, hour, min, sec) {
        Some(v) => v,
        None => -1,
    }
}
