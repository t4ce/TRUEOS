use alloc::vec::Vec as AVec;
use core::fmt::Write;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};
use heapless::{Deque, String, Vec as HVec};
use spin::Mutex;

use super::ShellIo;

pub const MAX_SLOTS: usize = 8;
pub const MAX_LINES: usize = 64;
pub const TITLE_LEN: usize = 32;
pub const LINE_LEN: usize = 96;
pub const STATUS_TEXT_LEN: usize = 10;
pub const STATUS_INDICATORS: usize = 5;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SlotState {
    Running,
    Done,
    Failed,
}

pub struct SlotData {
    pub state: SlotState,
    pub title: String<TITLE_LEN>,
    pub lines: Deque<String<LINE_LEN>, MAX_LINES>,
    pub status_left: String<STATUS_TEXT_LEN>,
    pub status_right: String<STATUS_TEXT_LEN>,
    pub status_indicators: [u8; STATUS_INDICATORS],
    pub blob: AVec<u8>,
}

impl SlotData {
    pub const fn empty() -> Self {
        Self {
            state: SlotState::Done,
            title: String::new(),
            lines: Deque::new(),
            status_left: String::new(),
            status_right: String::new(),
            status_indicators: [0u8; STATUS_INDICATORS],
            blob: AVec::new(),
        }
    }

    fn reset(&mut self) {
        self.state = SlotState::Running;
        self.title.clear();
        self.lines.clear();
        self.status_left.clear();
        self.status_right.clear();
        self.status_indicators = [0u8; STATUS_INDICATORS];
        self.blob.clear();
    }

    fn free(&mut self) {
        self.state = SlotState::Done;
        self.title.clear();
        self.lines.clear();
        self.status_left.clear();
        self.status_right.clear();
        self.status_indicators = [0u8; STATUS_INDICATORS];
        self.blob.clear();
    }
}

#[inline]
fn push_line_into_lines(lines: &mut Deque<String<LINE_LEN>, MAX_LINES>, line: &str) {
    let mut s: String<LINE_LEN> = String::new();
    for ch in line.chars() {
        if ch == '\r' || ch == '\n' {
            continue;
        }
        if s.push(ch).is_err() {
            break;
        }
    }

    if lines.is_full() {
        let _ = lines.pop_front();
    }
    let _ = lines.push_back(s);
}

#[inline]
fn refresh_preview_locked(data: &mut SlotData) {
    data.lines.clear();
    if data.blob.is_empty() {
        return;
    }

    let blob = data.blob.as_slice();
    let lines = &mut data.lines;

    if let Ok(text) = core::str::from_utf8(blob) {
        for line in text.split('\n') {
            push_line_into_lines(lines, line.trim_end_matches('\r'));
        }
        return;
    }

    // Lossy UTF-8 decode:
    // - preserves as much readable content as possible
    // - replaces invalid sequences with U+FFFD
    // - keeps the same newline splitting behavior as the UTF-8 fast path
    let mut cur: String<LINE_LEN> = String::new();
    let mut i: usize = 0;
    while i < blob.len() {
        match core::str::from_utf8(&blob[i..]) {
            Ok(s) => {
                for ch in s.chars() {
                    match ch {
                        '\r' => {}
                        '\n' => {
                            if lines.is_full() {
                                let _ = lines.pop_front();
                            }
                            let _ = lines.push_back(cur);
                            cur = String::new();
                        }
                        _ => {
                            let _ = cur.push(ch);
                        }
                    }
                }
                break;
            }
            Err(e) => {
                let valid_up_to = e.valid_up_to();
                if valid_up_to != 0 {
                    if let Ok(s) = core::str::from_utf8(&blob[i..i + valid_up_to]) {
                        for ch in s.chars() {
                            match ch {
                                '\r' => {}
                                '\n' => {
                                    if lines.is_full() {
                                        let _ = lines.pop_front();
                                    }
                                    let _ = lines.push_back(cur);
                                    cur = String::new();
                                }
                                _ => {
                                    let _ = cur.push(ch);
                                }
                            }
                        }
                    }
                    i += valid_up_to;
                }

                // Skip the invalid byte sequence and insert a replacement char.
                let skip = e.error_len().unwrap_or(1).max(1);
                i = i.saturating_add(skip);
                let _ = cur.push('\u{FFFD}');
            }
        }
    }

    if !cur.is_empty() {
        if lines.is_full() {
            let _ = lines.pop_front();
        }
        let _ = lines.push_back(cur);
    }
}

pub struct Slot {
    used: AtomicBool,
    data: Mutex<SlotData>,
}

impl Slot {
    pub const fn empty() -> Self {
        Self {
            used: AtomicBool::new(false),
            data: Mutex::new(SlotData::empty()),
        }
    }
}

static SLOTS: [Slot; MAX_SLOTS] = [const { Slot::empty() }; MAX_SLOTS];
static ACTIVE_STATUS_SLOT: spin::Mutex<Option<u8>> = spin::Mutex::new(None);

#[inline]
fn slot_ref(slot_id: u8) -> Option<&'static Slot> {
    let idx = slot_id as usize;
    if idx >= MAX_SLOTS {
        return None;
    }
    Some(&SLOTS[idx])
}

pub fn alloc_slot(title: &str) -> Option<u8> {
    for (idx, slot) in SLOTS.iter().enumerate() {
        if slot
            .used
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            continue;
        }

        // We own the slot now; initialize it.
        let mut data = slot.data.lock();
        data.reset();
        for ch in title.chars() {
            if data.title.push(ch).is_err() {
                break;
            }
        }
        return Some(idx as u8);
    }
    None
}

pub fn free_slot(slot_id: u8) -> bool {
    let Some(slot) = slot_ref(slot_id) else {
        return false;
    };

    if slot
        .used
        .compare_exchange(true, false, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return false;
    }

    // Best-effort cleanup.
    let mut data = slot.data.lock();
    data.free();
    let mut active = ACTIVE_STATUS_SLOT.lock();
    if *active == Some(slot_id) {
        *active = None;
    }
    true
}

pub fn push_line(slot_id: u8, line: &str) {
    let Some(slot) = slot_ref(slot_id) else {
        return;
    };
    if !slot.used.load(Ordering::Acquire) {
        return;
    }
    let mut data = slot.data.lock();
    if !slot.used.load(Ordering::Acquire) {
        return;
    }

    push_line_into_lines(&mut data.lines, line);
}

/// Overwrites the slot blob with `bytes` (no size cap).
///
/// Returns `false` only if the slot is missing.

/// Moves an owned blob into the slot and updates preview lines from it.
pub fn set_blob_owned_with_preview(slot_id: u8, blob: AVec<u8>) -> bool {
    let Some(slot) = slot_ref(slot_id) else {
        return false;
    };
    if !slot.used.load(Ordering::Acquire) {
        return false;
    }
    let mut data = slot.data.lock();
    if !slot.used.load(Ordering::Acquire) {
        return false;
    }

    data.blob = blob;
    refresh_preview_locked(&mut data);
    true
}

/// Takes ownership of the slot blob, leaving it empty.
pub fn take_blob(slot_id: u8) -> Option<AVec<u8>> {
    let Some(slot) = slot_ref(slot_id) else {
        return None;
    };
    if !slot.used.load(Ordering::Acquire) {
        return None;
    }
    let mut data = slot.data.lock();
    if !slot.used.load(Ordering::Acquire) {
        return None;
    }
    Some(core::mem::take(&mut data.blob))
}

/// Clones the current slot blob without clearing it.
pub fn clone_blob(slot_id: u8) -> Option<AVec<u8>> {
    let Some(slot) = slot_ref(slot_id) else {
        return None;
    };
    if !slot.used.load(Ordering::Acquire) {
        return None;
    }
    let data = slot.data.lock();
    if !slot.used.load(Ordering::Acquire) {
        return None;
    }
    Some(data.blob.clone())
}

pub fn set_state(slot_id: u8, state: SlotState) {
    let Some(slot) = slot_ref(slot_id) else {
        return;
    };
    if !slot.used.load(Ordering::Acquire) {
        return;
    }
    let mut data = slot.data.lock();
    if !slot.used.load(Ordering::Acquire) {
        return;
    }
    data.state = state;
}

pub fn with_slot<R>(slot_id: u8, f: impl FnOnce(&SlotData) -> R) -> Option<R> {
    let Some(slot) = slot_ref(slot_id) else {
        return None;
    };
    if !slot.used.load(Ordering::Acquire) {
        return None;
    }
    let data = slot.data.lock();
    if !slot.used.load(Ordering::Acquire) {
        return None;
    }
    Some(f(&data))
}

/// Collects all allocated slots as (1-based-id, state) pairs in ascending order.
pub fn collect_symbols(out: &mut HVec<(u8, SlotState), MAX_SLOTS>) {
    out.clear();
    for (idx, slot) in SLOTS.iter().enumerate() {
        if !slot.used.load(Ordering::Acquire) {
            continue;
        }
        let data = slot.data.lock();
        if !slot.used.load(Ordering::Acquire) {
            continue;
        }
        let _ = out.push((idx as u8 + 1, data.state));
    }
}

pub fn list_slots(out: &mut String<512>) {
    out.clear();
    for (idx, slot) in SLOTS.iter().enumerate() {
        if !slot.used.load(Ordering::Acquire) {
            continue;
        }
        let data = slot.data.lock();
        if !slot.used.load(Ordering::Acquire) {
            continue;
        }
        let _ = write!(out, "#{} {:?} {}\r\n", idx + 1, data.state, data.title.as_str());
    }
    if out.is_empty() {
        let _ = out.push_str("(no async jobs)\r\n");
    }
}

pub fn dump_slot(out: &mut String<1024>, slot_id: u8) -> bool {
    out.clear();
    let idx = slot_id as usize;
    if idx >= MAX_SLOTS {
        return false;
    }
    let slot = &SLOTS[idx];
    if !slot.used.load(Ordering::Acquire) {
        return false;
    }
    let data = slot.data.lock();
    if !slot.used.load(Ordering::Acquire) {
        return false;
    }

    let _ = write!(out, "#{} {:?} {}\r\n", idx + 1, data.state, data.title.as_str());
    for line in data.lines.iter() {
        let _ = write!(out, "{}\r\n", line.as_str());
    }
    true
}

pub fn set_active_status_slot(slot_id: u8) -> bool {
    let Some(slot) = slot_ref(slot_id) else {
        return false;
    };
    if !slot.used.load(Ordering::Acquire) {
        return false;
    }
    let mut active = ACTIVE_STATUS_SLOT.lock();
    *active = Some(slot_id);
    true
}

pub fn active_status_slot() -> Option<u8> {
    *ACTIVE_STATUS_SLOT.lock()
}

pub fn clear_active_status_slot() {
    let mut active = ACTIVE_STATUS_SLOT.lock();
    *active = None;
}

#[inline]
fn assign_status_text(dst: &mut String<STATUS_TEXT_LEN>, text: &str) {
    dst.clear();
    for ch in text.chars() {
        if ch == '\r' || ch == '\n' {
            continue;
        }
        if dst.push(ch).is_err() {
            break;
        }
    }
}

pub fn status_set_left(slot_id: u8, text: &str) -> bool {
    let Some(slot) = slot_ref(slot_id) else {
        return false;
    };
    if !slot.used.load(Ordering::Acquire) {
        return false;
    }
    let mut data = slot.data.lock();
    if !slot.used.load(Ordering::Acquire) {
        return false;
    }
    assign_status_text(&mut data.status_left, text);
    true
}

pub fn status_set_right(slot_id: u8, text: &str) -> bool {
    let Some(slot) = slot_ref(slot_id) else {
        return false;
    };
    if !slot.used.load(Ordering::Acquire) {
        return false;
    }
    let mut data = slot.data.lock();
    if !slot.used.load(Ordering::Acquire) {
        return false;
    }
    assign_status_text(&mut data.status_right, text);
    true
}

pub fn status_set_indicator(slot_id: u8, index: usize, color_code: u8) -> bool {
    let Some(slot) = slot_ref(slot_id) else {
        return false;
    };
    if !slot.used.load(Ordering::Acquire) {
        return false;
    }
    if index >= STATUS_INDICATORS {
        return false;
    }
    let mut data = slot.data.lock();
    if !slot.used.load(Ordering::Acquire) {
        return false;
    }
    data.status_indicators[index] = color_code;
    true
}

pub fn status_set_left_active(text: &str) -> bool {
    let Some(slot_id) = active_status_slot() else {
        return false;
    };
    status_set_left(slot_id, text)
}

pub fn status_set_right_active(text: &str) -> bool {
    let Some(slot_id) = active_status_slot() else {
        return false;
    };
    status_set_right(slot_id, text)
}

pub fn status_set_indicator_active(index: usize, color_code: u8) -> bool {
    let Some(slot_id) = active_status_slot() else {
        return false;
    };
    status_set_indicator(slot_id, index, color_code)
}

pub struct StatusBarSnapshot {
    pub indicators: [u8; STATUS_INDICATORS],
    pub left: String<STATUS_TEXT_LEN>,
    pub right: String<STATUS_TEXT_LEN>,
}

pub fn active_status_snapshot() -> Option<StatusBarSnapshot> {
    let slot_id = active_status_slot()?;
    with_slot(slot_id, |s| StatusBarSnapshot {
        indicators: s.status_indicators,
        left: s.status_left.clone(),
        right: s.status_right.clone(),
    })
}

#[inline]
pub(crate) fn refresh_matrix_symbols(io: &dyn ShellIo, term_cols: usize) {
    io.write_str(crate::ecma48::SAVE_CURSOR);

    let mut symbols: HVec<(u8, SlotState), MAX_SLOTS> = HVec::new();
    collect_symbols(&mut symbols);

    let mut visible_len: usize = 0;
    for (i, (id, state)) in symbols.iter().enumerate() {
        if i != 0 {
            visible_len += 1;
        }
        match state {
            SlotState::Running => {
                // "§⣿"
                visible_len += 2;
            }
            _ => {
                // "§<id>"
                visible_len += 1; // '§'
                let mut n = *id as usize;
                let mut digits = 1;
                while n >= 10 {
                    digits += 1;
                    n /= 10;
                }
                visible_len += digits;
            }
        }
    }

    // Clear the center area (between Title and Time) so shrinking/empty updates don't
    // leave stale characters behind.
    if term_cols > 16 {
        let start_col = 9;
        // Leave space for the right-side Time (~8 chars).
        let end_col = term_cols.saturating_sub(8); 
        if end_col > start_col {
            io.write_fmt(format_args!("{}", crate::ecma48::pos(1, start_col)));
            for _ in 0..(end_col - start_col) {
                io.write_byte(b' ');
            }
        }
    }

    if term_cols != 0 && visible_len != 0 {
        // Center alignment
        let mut start_col = term_cols
            .saturating_sub(visible_len)
            .saturating_div(2)
            .saturating_add(1);
        
        // Clamp to avoid overwriting title
        start_col = start_col.max(9);

        if start_col <= term_cols {
            io.write_fmt(format_args!("{}", crate::ecma48::pos(1, start_col)));
            for (i, (id, state)) in symbols.iter().enumerate() {
                if i != 0 {
                    io.write_byte(b' ');
                }
                match *state {
                    SlotState::Running => {
                        let mut s: String<4> = String::new();
                        let _ = s.push('§');
                        let _ = s.push(super::MATRIX_RUNNING_GLYPH);
                        io.write_str(s.as_str());
                    }
                    _ => {
                        let mut s: String<8> = String::new();
                        let _ = write!(s, "§{}", id);
                        if *state == SlotState::Done {
                            io.write_fmt(format_args!(
                                "{}",
                                crate::ecma48::color(s.as_str(), super::PROMPT_RGB)
                            ));
                        } else {
                            io.write_str(s.as_str());
                        }
                    }
                }
            }
        }
    }

    io.write_str(crate::ecma48::RESTORE_CURSOR);
}

#[inline]
fn append_blob_line(blob: &mut AVec<u8>, line: &str) {
    blob.extend_from_slice(line.as_bytes());
    blob.extend_from_slice(b"\r\n");
}

#[inline]
fn log_line(slot_id: u8, blob: &mut AVec<u8>, line: &str) {
    push_line(slot_id, line);
    append_blob_line(blob, line);
    crate::runtime::poll_local_executor();
}

#[embassy_executor::task]
pub(crate) async fn install_matrix_job(
    slot_id: u8,
    disk: crate::disc::block::DeviceHandle,
    bootx64: &'static [u8],
    kernel: &'static [u8],
) {
    async move {
    // Give the shell a moment to print the prompt and update the header.
    Timer::after(EmbassyDuration::from_millis(1)).await;

    let mut blob: AVec<u8> = AVec::new();

    let info = disk.info();
    log_line(slot_id, &mut blob, "install: starting");
    log_line(
        slot_id,
        &mut blob,
        alloc::format!(
            "install: target id={} ({}) blocks={} bs={} writable={} label={:?}",
            info.id.raw(),
            info.id,
            info.block_count,
            info.block_size,
            info.writable,
            info.label,
        )
        .as_str(),
    );

    let (status, err) = crate::v::disc::detect::detect_physical_disk_detail(disk).await;
    log_line(
        slot_id,
        &mut blob,
        alloc::format!(
            "install: initial status: {}{}",
            status.short(),
            match (&status, err) {
                (crate::v::disc::detect::DiscStatus::Unknown, Some(e)) => alloc::format!(" (err={:?})", e),
                _ => alloc::string::String::new(),
            }
        )
        .as_str(),
    );

    log_line(slot_id, &mut blob, "install: DANGER: this may REPARTITION and FORMAT the disk");
    log_line(slot_id, &mut blob, "install: creating/updating GPT + ESP + TRUEOSFS boot files");
    log_line(slot_id, &mut blob, "install: existing TRUEOSFS will be preserved if detected");

    // UEFI-only install: if the installer was booted in legacy/CSM mode, warn loudly.
    let uefi = crate::limine::efi_system_table_address().unwrap_or(0) != 0;
    if !uefi {
        log_line(
            slot_id,
            &mut blob,
            "install: WARNING: running without UEFI (legacy/CSM boot). Installed TRUEOS is UEFI-only.",
        );
        log_line(
            slot_id,
            &mut blob,
            "install: After install, reboot and select the UEFI boot entry for this disk.",
        );
    }

    log_line(
        slot_id,
        &mut blob,
        alloc::format!(
            "install: BOOTX64.EFI={} bytes, TRUEOS.elf={} bytes",
            bootx64.len(),
            kernel.len()
        )
        .as_str(),
    );

    let result = crate::disc::install::install_bootable_uefi_gpt_with_log(
        disk,
        bootx64,
        kernel,
        &mut |line| {
            log_line(slot_id, &mut blob, line);
        },
    )
    .await;
    match result {
        Ok(()) => {
            let (status, err) = crate::v::disc::detect::detect_physical_disk_detail(disk).await;
            log_line(
                slot_id,
                &mut blob,
                alloc::format!(
                    "install: ok (status now: {}{})",
                    status.short(),
                    match (&status, err) {
                        (crate::v::disc::detect::DiscStatus::Unknown, Some(e)) => {
                            alloc::format!("; err={:?}", e)
                        }
                        _ => alloc::string::String::new(),
                    }
                )
                .as_str(),
            );
            set_state(slot_id, SlotState::Done);
        }
        Err(e) => {
            log_line(
                slot_id,
                &mut blob,
                alloc::format!("install: failed ({:?})", e).as_str(),
            );
            set_state(slot_id, SlotState::Failed);
        }
    }

    let _ = set_blob_owned_with_preview(slot_id, blob);
    }.await;
}

#[embassy_executor::task]
pub(crate) async fn update_matrix_job(slot_id: u8, disk: crate::disc::block::DeviceHandle) {
    async move {
    use embassy_time::Timer;

    // Give the shell a moment to print the prompt and update the header.
    Timer::after(EmbassyDuration::from_millis(1)).await;

    let mut blob: AVec<u8> = AVec::new();
    log_line(slot_id, &mut blob, "update: waiting for net");
    crate::v::readiness::wait_for(crate::v::readiness::NET_GATEWAY_REACHABLE).await;

    // These URLs are expected to be hosted alongside the published installer artifacts.
    // The server should serve the raw files (not inside a .7z) so the kernel doesn't need
    // archive/ISO parsing just to update.
    const BOOTX64_URL: &str = "https://trueos.eu/EFI/BOOT/BOOTX64.EFI";
    const KERNEL_URL: &str = "https://trueos.eu/TRUEOS.elf";

    let info = disk.info();
    log_line(slot_id, &mut blob, "update: starting");
    log_line(
        slot_id,
        &mut blob,
        alloc::format!(
            "update: target id={} ({}) blocks={} bs={} writable={} label={:?}",
            info.id.raw(),
            info.id,
            info.block_count,
            info.block_size,
            info.writable,
            info.label,
        )
        .as_str(),
    );

    let (status, err) = crate::v::disc::detect::detect_physical_disk_detail(disk).await;
    log_line(
        slot_id,
        &mut blob,
        alloc::format!(
            "update: initial status: {}{}",
            status.short(),
            match (&status, err) {
                (crate::v::disc::detect::DiscStatus::Unknown, Some(e)) => alloc::format!(" (err={:?})", e),
                _ => alloc::string::String::new(),
            }
        )
        .as_str(),
    );

    // Safety: `update` is intended to refresh an existing TRUEOS install.
    // Refuse to proceed if TRUEOSFS is not detected.
    match crate::v::fs::trueosfs::locate_async(disk).await {
        Ok(Some(loc)) => {
            log_line(
                slot_id,
                &mut blob,
                alloc::format!(
                    "update: TRUEOSFS detected (bootable={}, super_lba={}, data_lba={}, data_end={:?})",
                    loc.bootable,
                    loc.super_lba,
                    loc.data_lba,
                    loc.data_end_lba_exclusive,
                )
                .as_str(),
            );
        }
        Ok(None) => {
            log_line(slot_id, &mut blob, "update: refused (no TRUEOSFS detected on target disk)");
            log_line(slot_id, &mut blob, "update: use `install` for a fresh install");
            set_state(slot_id, SlotState::Failed);
            let _ = set_blob_owned_with_preview(slot_id, blob);
            return;
        }
        Err(e) => {
            log_line(
                slot_id,
                &mut blob,
                alloc::format!("update: locate_async failed ({:?}); refusing", e).as_str(),
            );
            set_state(slot_id, SlotState::Failed);
            let _ = set_blob_owned_with_preview(slot_id, blob);
            return;
        }
    }

    log_line(slot_id, &mut blob, "update: fetching BOOTX64.EFI + TRUEOS.elf over HTTPS");
    log_line(slot_id, &mut blob, alloc::format!("update: BOOTX64 url={}", BOOTX64_URL).as_str());
    log_line(slot_id, &mut blob, alloc::format!("update: kernel url={}", KERNEL_URL).as_str());

    let bootx64 = match crate::v::net::https::fetch_https_body_async(
        BOOTX64_URL,
        60_000,
        8 * 1024 * 1024,
    )
    .await
    {
        Ok(b) => b,
        Err(e) => {
            log_line(
                slot_id,
                &mut blob,
                alloc::format!("update: BOOTX64 download failed ({:?})", e).as_str(),
            );
            set_state(slot_id, SlotState::Failed);
            let _ = set_blob_owned_with_preview(slot_id, blob);
            return;
        }
    };

    let kernel = match crate::v::net::https::fetch_https_body_async(
        KERNEL_URL,
        60_000,
        64 * 1024 * 1024,
    )
    .await
    {
        Ok(b) => b,
        Err(e) => {
            log_line(
                slot_id,
                &mut blob,
                alloc::format!("update: kernel download failed ({:?})", e).as_str(),
            );
            set_state(slot_id, SlotState::Failed);
            let _ = set_blob_owned_with_preview(slot_id, blob);
            return;
        }
    };

    // Sanity checks to avoid obviously-bad installs.
    let bootx64_ok = bootx64.get(0..2) == Some(b"MZ");
    let kernel_ok = kernel.get(0..4) == Some(b"\x7FELF");
    log_line(
        slot_id,
        &mut blob,
        alloc::format!(
            "update: downloaded BOOTX64.EFI={} bytes (mz={}), TRUEOS.elf={} bytes (elf={})",
            bootx64.len(),
            bootx64_ok,
            kernel.len(),
            kernel_ok
        )
        .as_str(),
    );
    if !bootx64_ok || !kernel_ok {
        log_line(slot_id, &mut blob, "update: refusing to install (payload format looks wrong)");
        set_state(slot_id, SlotState::Failed);
        let _ = set_blob_owned_with_preview(slot_id, blob);
        return;
    }

    log_line(slot_id, &mut blob, "update: updating GPT + ESP + TRUEOSFS boot files");
    log_line(slot_id, &mut blob, "update: existing TRUEOSFS will be preserved if detected");

    let result = crate::disc::install::install_bootable_uefi_gpt_with_log(
        disk,
        &bootx64,
        &kernel,
        &mut |line| {
            log_line(slot_id, &mut blob, line);
        },
    )
    .await;

    match result {
        Ok(()) => {
            let (status, err) = crate::v::disc::detect::detect_physical_disk_detail(disk).await;
            log_line(
                slot_id,
                &mut blob,
                alloc::format!(
                    "update: ok (status now: {}{})",
                    status.short(),
                    match (&status, err) {
                        (crate::v::disc::detect::DiscStatus::Unknown, Some(e)) => {
                            alloc::format!("; err={:?}", e)
                        }
                        _ => alloc::string::String::new(),
                    }
                )
                .as_str(),
            );
            set_state(slot_id, SlotState::Done);
        }
        Err(e) => {
            log_line(
                slot_id,
                &mut blob,
                alloc::format!("update: failed ({:?})", e).as_str(),
            );
            set_state(slot_id, SlotState::Failed);
        }
    }

    let _ = set_blob_owned_with_preview(slot_id, blob);
    }.await;
}
