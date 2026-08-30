//! Dobby-only UI4 observation and interaction capability.
//!
//! This bridge deliberately sits beside Spirit rather than inside shell2 or
//! Lumen.  It lends Dobby Lilly's already-paired software cursor and virtual
//! keyboard, exposes only generation-checked broker window identifiers, and
//! keeps captured observations private to the calling Blueprint VM.

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use serde::Serialize;
use spin::Mutex;
use trueos_time::{Duration, Timer};

use crate::r::services::keyboard_control_service::{
    KEYBOARD_CONTROL_FLAG_CLEAR_QUEUE, KEYBOARD_CONTROL_OPCODE_STROKE, KeyboardControlCommand,
    KeyboardControlDevice, KeyboardControlPrincipal, keyboard_is_idle, submit_command, submit_text,
};
use crate::ui4::{
    CursorFrameKey, OutputId, WindowId, WindowOwner, WindowPlacement, WindowSnapshot, WindowState,
};

pub(crate) const ERROR_DENIED: i32 = v::bp_abi::DOBBY_UI4_ERROR_DENIED;
pub(crate) const ERROR_BAD_STATE: i32 = v::bp_abi::DOBBY_UI4_ERROR_BAD_STATE;
pub(crate) const ERROR_BAD_INPUT: i32 = v::bp_abi::DOBBY_UI4_ERROR_BAD_INPUT;
pub(crate) const ERROR_BUSY: i32 = v::bp_abi::DOBBY_UI4_ERROR_BUSY;
pub(crate) const ERROR_UNAVAILABLE: i32 = v::bp_abi::DOBBY_UI4_ERROR_UNAVAILABLE;
pub(crate) const ERROR_NOT_FOUND: i32 = v::bp_abi::DOBBY_UI4_ERROR_NOT_FOUND;
const ERROR_TRANSPORT: i32 = v::bp_abi::DOBBY_UI4_ERROR_TRANSPORT;

const INVENTORY_WINDOW_LIMIT: usize = 64;
const INVENTORY_NAME_BYTES: usize = 96;
const MAX_DOBBY_TYPE_BYTES: usize = 1_024;
const MAX_DOBBY_TYPE_SCALARS: usize = 64;
const DOBBY_KEY_STROKE_MS: u32 = 48;
const IO_RETRY_MS: u64 = 8;

pub(crate) const POINTER_ACTION_MOVE: u32 = v::bp_abi::DOBBY_UI4_POINTER_MOVE;
pub(crate) const POINTER_ACTION_PRIMARY_CLICK: u32 = v::bp_abi::DOBBY_UI4_POINTER_PRIMARY_CLICK;
const POINTER_BUTTON_MASK: u32 = v::bp_abi::DOBBY_UI4_POINTER_BUTTON_MASK;
const POINTER_BUTTON_SHIFT: u32 = v::bp_abi::DOBBY_UI4_POINTER_BUTTON_SHIFT;
const POINTER_CLICK_COUNT_SHIFT: u32 = v::bp_abi::DOBBY_UI4_POINTER_CLICK_COUNT_SHIFT;
const POINTER_CLICK_COUNT_MASK: u32 = v::bp_abi::DOBBY_UI4_POINTER_CLICK_COUNT_MASK;
const POINTER_CLICK_COUNT_DEFAULT: u32 = v::bp_abi::DOBBY_UI4_POINTER_CLICK_COUNT_DEFAULT;
const POINTER_CLICK_COUNT_MAX: u32 = v::bp_abi::DOBBY_UI4_POINTER_CLICK_COUNT_MAX;
const POINTER_CLICK_DELAY_SHIFT: u32 = v::bp_abi::DOBBY_UI4_POINTER_CLICK_DELAY_SHIFT;
const POINTER_CLICK_DELAY_MASK: u32 = v::bp_abi::DOBBY_UI4_POINTER_CLICK_DELAY_MASK;
const POINTER_CLICK_DELAY_MIN_MS: u32 = v::bp_abi::DOBBY_UI4_POINTER_CLICK_DELAY_MIN_MS;
const POINTER_CLICK_DELAY_MAX_MS: u32 = v::bp_abi::DOBBY_UI4_POINTER_CLICK_DELAY_MAX_MS;

pub(crate) const KEY_ENTER: u32 = v::bp_abi::DOBBY_UI4_KEY_ENTER;
pub(crate) const KEY_ESCAPE: u32 = v::bp_abi::DOBBY_UI4_KEY_ESCAPE;
pub(crate) const KEY_BACKSPACE: u32 = v::bp_abi::DOBBY_UI4_KEY_BACKSPACE;
pub(crate) const KEY_TAB: u32 = v::bp_abi::DOBBY_UI4_KEY_TAB;
pub(crate) const KEY_SPACE: u32 = v::bp_abi::DOBBY_UI4_KEY_SPACE;
pub(crate) const KEY_ARROW_RIGHT: u32 = v::bp_abi::DOBBY_UI4_KEY_ARROW_RIGHT;
pub(crate) const KEY_ARROW_LEFT: u32 = v::bp_abi::DOBBY_UI4_KEY_ARROW_LEFT;
pub(crate) const KEY_ARROW_DOWN: u32 = v::bp_abi::DOBBY_UI4_KEY_ARROW_DOWN;
pub(crate) const KEY_ARROW_UP: u32 = v::bp_abi::DOBBY_UI4_KEY_ARROW_UP;
pub(crate) const KEY_DELETE: u32 = v::bp_abi::DOBBY_UI4_KEY_DELETE;
pub(crate) const KEY_HOME: u32 = v::bp_abi::DOBBY_UI4_KEY_HOME;
pub(crate) const KEY_END: u32 = v::bp_abi::DOBBY_UI4_KEY_END;
pub(crate) const KEY_PAGE_UP: u32 = v::bp_abi::DOBBY_UI4_KEY_PAGE_UP;
pub(crate) const KEY_PAGE_DOWN: u32 = v::bp_abi::DOBBY_UI4_KEY_PAGE_DOWN;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum LillyIoLease {
    Response,
    Dobby(u8),
}

struct LillyIoState {
    keyboard: Option<KeyboardControlDevice>,
    lease: Option<LillyIoLease>,
}

impl LillyIoState {
    const fn new() -> Self {
        Self {
            keyboard: None,
            lease: None,
        }
    }
}

static LILLY_IO: Mutex<LillyIoState> = Mutex::new(LillyIoState::new());

struct ObservationCache {
    owner_instance: [u8; 16],
    owner_generation: u64,
    window_id: u32,
    placement: Option<WindowPlacement>,
    metadata: Vec<u8>,
    png: Vec<u8>,
}

impl ObservationCache {
    const fn new() -> Self {
        Self {
            owner_instance: [0; 16],
            owner_generation: 0,
            window_id: 0,
            placement: None,
            metadata: Vec::new(),
            png: Vec::new(),
        }
    }

    fn clear(&mut self) {
        self.owner_instance = [0; 16];
        self.owner_generation = 0;
        self.window_id = 0;
        self.placement = None;
        self.metadata.clear();
        self.png.clear();
    }
}

static OBSERVATIONS: [Mutex<ObservationCache>; crate::allcaps::hv::VM_ID_LIMIT] =
    [const { Mutex::new(ObservationCache::new()) }; crate::allcaps::hv::VM_ID_LIMIT];

#[derive(Serialize)]
struct WindowInventory {
    windows: Vec<WindowInventoryEntry>,
    truncated: bool,
}

#[derive(Serialize)]
struct WindowInventoryEntry {
    id: String,
    name: String,
    rect: [i64; 4],
    plane: usize,
    z: i32,
    input: bool,
    selected: bool,
}

#[derive(Serialize)]
struct ObservationMetadata {
    id: String,
    name: String,
    native: [u32; 2],
    capture: [u32; 2],
    rect: [i64; 4],
    grid_extent: u16,
    grid_major_step: u16,
    png_bytes: usize,
    revision: u64,
    publish_serial: u64,
}

fn observation_slot(owner: u8) -> Result<&'static Mutex<ObservationCache>, i32> {
    OBSERVATIONS.get(owner as usize).ok_or(ERROR_DENIED)
}

fn authorized(owner: u8) -> bool {
    let Some(archive) = crate::hv::app_vm_archive(owner) else {
        return false;
    };
    let leaf = archive
        .rsplit(|ch| ch == '/' || ch == '\\')
        .next()
        .unwrap_or(archive.as_str());
    leaf.eq_ignore_ascii_case("dobby") || leaf.eq_ignore_ascii_case("dobby.bp")
}

fn require_authorized(owner: u8) -> Result<(), i32> {
    authorized(owner).then_some(()).ok_or(ERROR_DENIED)
}

fn owner_identity(owner: u8) -> Result<([u8; 16], u64), i32> {
    let identity = crate::hv::blueprint_instance_identity(owner).ok_or(ERROR_DENIED)?;
    Ok((identity.instance, identity.generation))
}

fn cache_belongs_to(cache: &ObservationCache, owner: u8) -> bool {
    owner_identity(owner).is_ok_and(|(instance, generation)| {
        cache.owner_instance == instance && cache.owner_generation == generation
    })
}

fn compact_name(window: WindowSnapshot) -> String {
    let name = match window.owner {
        WindowOwner::Vm(vm_id) => crate::hv::app_vm_display_label(vm_id)
            .unwrap_or_else(|| window.producer_name.to_string()),
        _ => window.producer_name.to_string(),
    };
    if name.len() <= INVENTORY_NAME_BYTES {
        return name;
    }
    let mut end = INVENTORY_NAME_BYTES;
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    name[..end].to_string()
}

fn live_windows() -> Result<Vec<WindowSnapshot>, i32> {
    let output = OutputId::from_slot(0).ok_or(ERROR_UNAVAILABLE)?;
    let mut windows = crate::ui4::visible_windows_for_output(output);
    windows
        .sort_unstable_by_key(|window| (window.plane.slot(), window.placement.z, window.id.raw()));
    Ok(windows)
}

fn live_window(raw: u64) -> Result<WindowSnapshot, i32> {
    let raw = u32::try_from(raw).map_err(|_| ERROR_BAD_INPUT)?;
    let id = WindowId::from_raw(raw).ok_or(ERROR_BAD_INPUT)?;
    live_windows()?
        .into_iter()
        .find(|window| window.id == id && window.state == WindowState::Ready)
        .ok_or(ERROR_NOT_FOUND)
}

fn selected_lilly_window() -> Result<WindowSnapshot, i32> {
    let source = super::lilly_cursor::selection_source().map_err(|_| ERROR_UNAVAILABLE)?;
    let key = crate::ui4::selected_frame_for_source(source).ok_or(ERROR_BAD_STATE)?;
    live_windows()?
        .into_iter()
        .find(|window| {
            window.owner == key.owner
                && window.id == key.window
                && window.state == WindowState::Ready
        })
        .ok_or(ERROR_NOT_FOUND)
}

fn copy_complete(bytes: &[u8], out: &mut [u8]) -> isize {
    let Ok(required) = isize::try_from(bytes.len()) else {
        return ERROR_UNAVAILABLE as isize;
    };
    if out.len() < bytes.len() {
        return required;
    }
    out[..bytes.len()].copy_from_slice(bytes);
    required
}

pub(crate) fn windows(owner: u8, out: &mut [u8]) -> isize {
    if let Err(error) = require_authorized(owner) {
        return error as isize;
    }
    let all = match live_windows() {
        Ok(windows) => windows,
        Err(error) => return error as isize,
    };
    let selected = super::lilly_cursor::selection_source()
        .ok()
        .and_then(crate::ui4::selected_frame_for_source);
    let truncated = all.len() > INVENTORY_WINDOW_LIMIT;
    let windows = all
        .into_iter()
        .take(INVENTORY_WINDOW_LIMIT)
        .map(|window| WindowInventoryEntry {
            id: format!("{}", window.id.raw()),
            name: compact_name(window),
            rect: [
                i64::from(window.placement.x),
                i64::from(window.placement.y),
                i64::from(window.placement.width),
                i64::from(window.placement.height),
            ],
            plane: window.plane.slot(),
            z: window.placement.z,
            input: window.interaction.receives_input,
            selected: selected == Some(CursorFrameKey::new(window.owner, window.id)),
        })
        .collect();
    let bytes = match serde_json::to_vec(&WindowInventory { windows, truncated }) {
        Ok(bytes) => bytes,
        Err(_) => return ERROR_UNAVAILABLE as isize,
    };
    copy_complete(bytes.as_slice(), out)
}

fn dobby_io_is_idle(keyboard: KeyboardControlDevice) -> bool {
    let cursor_idle = super::lilly_cursor::window_approach_complete().unwrap_or(false);
    let keyboard_idle =
        keyboard_is_idle(KeyboardControlPrincipal::Kernel, keyboard.handle).unwrap_or(false);
    cursor_idle && keyboard_idle
}

fn reap_dobby_io() {
    let candidate = {
        let state = LILLY_IO.lock();
        match (state.lease, state.keyboard) {
            (Some(LillyIoLease::Dobby(owner)), Some(keyboard)) => Some((owner, keyboard)),
            _ => None,
        }
    };
    let Some((owner, keyboard)) = candidate else {
        return;
    };
    if !dobby_io_is_idle(keyboard) {
        return;
    }
    let mut state = LILLY_IO.lock();
    if state.lease == Some(LillyIoLease::Dobby(owner)) && state.keyboard == Some(keyboard) {
        state.lease = None;
    }
}

fn claim_dobby_io(owner: u8) -> Result<KeyboardControlDevice, i32> {
    require_authorized(owner)?;
    reap_dobby_io();
    let mut state = LILLY_IO.lock();
    let keyboard = state.keyboard.ok_or(ERROR_UNAVAILABLE)?;
    if state.lease.is_some() {
        return Err(ERROR_BUSY);
    }
    state.lease = Some(LillyIoLease::Dobby(owner));
    Ok(keyboard)
}

fn release_dobby_io(owner: u8) {
    let mut state = LILLY_IO.lock();
    if state.lease == Some(LillyIoLease::Dobby(owner)) {
        state.lease = None;
    }
}

pub(super) fn register_lilly_keyboard(keyboard: KeyboardControlDevice) {
    let mut state = LILLY_IO.lock();
    state.keyboard = Some(keyboard);
}

pub(super) async fn acquire_response_io(keyboard: KeyboardControlDevice) {
    loop {
        reap_dobby_io();
        {
            let mut state = LILLY_IO.lock();
            if state.keyboard == Some(keyboard) && state.lease.is_none() {
                state.lease = Some(LillyIoLease::Response);
                return;
            }
        }
        Timer::after(Duration::from_millis(IO_RETRY_MS)).await;
    }
}

pub(super) fn release_response_io(keyboard: KeyboardControlDevice) {
    let mut state = LILLY_IO.lock();
    if state.keyboard == Some(keyboard) && state.lease == Some(LillyIoLease::Response) {
        state.lease = None;
    }
}

pub(crate) fn focus(owner: u8, window_id: u64) -> i32 {
    let _keyboard = match claim_dobby_io(owner) {
        Ok(keyboard) => keyboard,
        Err(error) => return error,
    };
    let result = (|| {
        let window = live_window(window_id)?;
        let source = super::lilly_cursor::selection_source().map_err(|_| ERROR_UNAVAILABLE)?;
        crate::ui4::select_window_for_cursor(source, window.owner, window.id)
            .map_err(|_| ERROR_BAD_STATE)?;
        if let Ok(slot) = observation_slot(owner) {
            slot.lock().clear();
        }
        Ok::<(), i32>(())
    })();
    release_dobby_io(owner);
    result.map(|()| 0).unwrap_or_else(|error| error)
}

pub(crate) fn observe_prepare(owner: u8) -> isize {
    let _keyboard = match claim_dobby_io(owner) {
        Ok(keyboard) => keyboard,
        Err(error) => return error as isize,
    };
    let result = (|| {
        let slot = observation_slot(owner)?;
        slot.lock().clear();
        let window = selected_lilly_window()?;
        let observation = crate::ui4::capture_compact_window_observation(window)
            .map_err(|_| ERROR_UNAVAILABLE)?;
        if observation.png.len() > crate::ui4::COMPACT_WINDOW_OBSERVATION_MAX_PNG_BYTES {
            return Err(ERROR_UNAVAILABLE);
        }
        let metadata = serde_json::to_vec(&ObservationMetadata {
            id: format!("{}", observation.window_id.raw()),
            name: compact_name(window),
            native: [observation.native_width, observation.native_height],
            capture: [observation.capture_width, observation.capture_height],
            rect: [
                i64::from(observation.placement.x),
                i64::from(observation.placement.y),
                i64::from(observation.placement.width),
                i64::from(observation.placement.height),
            ],
            grid_extent: observation.grid_extent,
            grid_major_step: observation.grid_major_step,
            png_bytes: observation.png.len(),
            revision: observation.revision,
            publish_serial: observation.publish_serial,
        })
        .map_err(|_| ERROR_UNAVAILABLE)?;
        let png_len = isize::try_from(observation.png.len()).map_err(|_| ERROR_UNAVAILABLE)?;
        let mut cache = slot.lock();
        let (owner_instance, owner_generation) = owner_identity(owner)?;
        cache.owner_instance = owner_instance;
        cache.owner_generation = owner_generation;
        cache.window_id = observation.window_id.raw();
        cache.placement = Some(observation.placement);
        cache.metadata = metadata;
        cache.png = observation.png;
        Ok::<isize, i32>(png_len)
    })();
    release_dobby_io(owner);
    result.unwrap_or_else(|error| error as isize)
}

pub(crate) fn observe_metadata(owner: u8, out: &mut [u8]) -> isize {
    if let Err(error) = require_authorized(owner) {
        return error as isize;
    }
    let slot = match observation_slot(owner) {
        Ok(slot) => slot,
        Err(error) => return error as isize,
    };
    let cache = slot.lock();
    if !cache_belongs_to(&cache, owner) || cache.window_id == 0 || cache.metadata.is_empty() {
        return ERROR_BAD_STATE as isize;
    }
    copy_complete(cache.metadata.as_slice(), out)
}

pub(crate) fn observe_read(owner: u8, offset: usize, out: &mut [u8]) -> isize {
    if let Err(error) = require_authorized(owner) {
        return error as isize;
    }
    let slot = match observation_slot(owner) {
        Ok(slot) => slot,
        Err(error) => return error as isize,
    };
    let cache = slot.lock();
    if !cache_belongs_to(&cache, owner) || cache.window_id == 0 || cache.png.is_empty() {
        return ERROR_BAD_STATE as isize;
    }
    if offset > cache.png.len() {
        return ERROR_BAD_INPUT as isize;
    }
    let available = cache.png.len() - offset;
    let copied = available.min(out.len());
    out[..copied].copy_from_slice(&cache.png[offset..offset + copied]);
    isize::try_from(copied).unwrap_or(ERROR_UNAVAILABLE as isize)
}

fn normalized_axis(origin: i32, extent: u32, normalized: u16) -> Result<i32, i32> {
    if extent == 0 || normalized > crate::ui4::COMPACT_WINDOW_GRID_EXTENT as u16 {
        return Err(ERROR_BAD_INPUT);
    }
    let offset = u64::from(extent.saturating_sub(1))
        .saturating_mul(u64::from(normalized))
        .saturating_add(u64::from(crate::ui4::COMPACT_WINDOW_GRID_EXTENT / 2))
        / u64::from(crate::ui4::COMPACT_WINDOW_GRID_EXTENT);
    Ok(i64::from(origin)
        .saturating_add(i64::try_from(offset).unwrap_or(i64::MAX))
        .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32)
}

fn parse_pointer_action(action: u32) -> Result<(u32, u32, u32, u32), i32> {
    let action_code = action & (POINTER_ACTION_MOVE | POINTER_ACTION_PRIMARY_CLICK);
    let click_count = (action & POINTER_CLICK_COUNT_MASK) >> POINTER_CLICK_COUNT_SHIFT;
    let click_count = if click_count == 0 {
        POINTER_CLICK_COUNT_DEFAULT
    } else {
        click_count
    };
    let click_delay_ms = (action & POINTER_CLICK_DELAY_MASK) >> POINTER_CLICK_DELAY_SHIFT;
    if click_count > POINTER_CLICK_COUNT_MAX {
        return Err(ERROR_BAD_INPUT);
    }
    if click_delay_ms != 0
        && !(POINTER_CLICK_DELAY_MIN_MS..=POINTER_CLICK_DELAY_MAX_MS).contains(&click_delay_ms)
    {
        return Err(ERROR_BAD_INPUT);
    }
    let click_delay_ms = if click_delay_ms == 0 {
        POINTER_CLICK_DELAY_MIN_MS
    } else {
        click_delay_ms
    };
    let flags = (action & POINTER_BUTTON_MASK) >> POINTER_BUTTON_SHIFT;
    match action_code {
        POINTER_ACTION_MOVE | POINTER_ACTION_PRIMARY_CLICK => {
            Ok((action_code, flags, click_count, click_delay_ms))
        }
        _ => Err(ERROR_BAD_INPUT),
    }
}

pub(crate) fn pointer(owner: u8, x: u16, y: u16, action: u32) -> i32 {
    let (action, buttons, click_count, click_delay_ms) = match parse_pointer_action(action) {
        Ok(result) => result,
        Err(error) => return error,
    };
    if x > crate::ui4::COMPACT_WINDOW_GRID_EXTENT as u16
        || y > crate::ui4::COMPACT_WINDOW_GRID_EXTENT as u16
    {
        return ERROR_BAD_INPUT;
    }
    let _keyboard = match claim_dobby_io(owner) {
        Ok(keyboard) => keyboard,
        Err(error) => return error,
    };
    let result = (|| {
        let window = selected_lilly_window()?;
        if action == POINTER_ACTION_PRIMARY_CLICK && !window.interaction.receives_input {
            return Err(ERROR_BAD_STATE);
        }
        let placement = {
            let cache = observation_slot(owner)?.lock();
            if cache_belongs_to(&cache, owner) && cache.window_id == window.id.raw() {
                cache.placement
            } else {
                None
            }
        }
        .ok_or(ERROR_BAD_STATE)?;
        let target_x = normalized_axis(placement.x, placement.width, x)?;
        let target_y = normalized_axis(placement.y, placement.height, y)?;
        let source = super::lilly_cursor::selection_source().map_err(|_| ERROR_UNAVAILABLE)?;
        crate::ui4::select_window_for_cursor(source, window.owner, window.id)
            .map_err(|_| ERROR_BAD_STATE)?;
        super::lilly_cursor::queue_pointer_action(
            target_x,
            target_y,
            buttons,
            action == POINTER_ACTION_PRIMARY_CLICK,
            click_count,
            click_delay_ms,
        )
        .map_err(|_| ERROR_UNAVAILABLE)?;
        Ok::<(), i32>(())
    })();
    if result.is_err() {
        release_dobby_io(owner);
    }
    result.map(|()| 0).unwrap_or_else(|error| error)
}
#[cfg(test)]
mod tests {
    use super::{
        POINTER_ACTION_MOVE, POINTER_ACTION_PRIMARY_CLICK, POINTER_CLICK_COUNT_MASK,
        POINTER_CLICK_COUNT_SHIFT, POINTER_CLICK_DELAY_MASK, POINTER_CLICK_DELAY_SHIFT,
        parse_pointer_action,
    };

    #[test]
    fn parse_pointer_action_handles_click_move_and_button_bits() {
        let click = POINTER_ACTION_PRIMARY_CLICK | (1 << 16);
        let move_without_buttons = POINTER_ACTION_MOVE;
        assert_eq!(parse_pointer_action(click), Ok((POINTER_ACTION_PRIMARY_CLICK, 1, 1, 100)));
        assert_eq!(
            parse_pointer_action(move_without_buttons),
            Ok((POINTER_ACTION_MOVE, 0, 1, 100))
        );
    }

    #[test]
    fn parse_pointer_action_handles_click_repeat_metadata() {
        let click = POINTER_ACTION_PRIMARY_CLICK
            | (5u32 << POINTER_CLICK_COUNT_SHIFT)
            | (250u32 << POINTER_CLICK_DELAY_SHIFT);
        assert_eq!(parse_pointer_action(click), Ok((POINTER_ACTION_PRIMARY_CLICK, 0, 5, 250)));
        assert_eq!(
            parse_pointer_action(
                POINTER_ACTION_PRIMARY_CLICK | (3u32 << POINTER_CLICK_COUNT_SHIFT)
            ),
            Ok((POINTER_ACTION_PRIMARY_CLICK, 0, 3, 100))
        );
        assert_eq!(
            parse_pointer_action(
                POINTER_ACTION_PRIMARY_CLICK
                    | (3u32 << POINTER_CLICK_COUNT_SHIFT)
                    | (250 << POINTER_CLICK_DELAY_SHIFT)
            ),
            Ok((POINTER_ACTION_PRIMARY_CLICK, 0, 3, 250))
        );
        let over_count =
            POINTER_ACTION_PRIMARY_CLICK | POINTER_CLICK_COUNT_MASK | POINTER_CLICK_DELAY_MASK;
        assert!(parse_pointer_action(over_count).is_err());
    }

    #[test]
    fn parse_pointer_action_rejects_unknown_actions() {
        assert!(parse_pointer_action(2).is_err());
        assert_eq!(
            parse_pointer_action(POINTER_ACTION_MOVE | (99 << 16)),
            Ok((POINTER_ACTION_MOVE, 99, 1, 100))
        );
    }
}

pub(crate) fn type_text(owner: u8, bytes: &[u8]) -> i32 {
    if bytes.is_empty() || bytes.len() > MAX_DOBBY_TYPE_BYTES {
        return ERROR_BAD_INPUT;
    }
    let Ok(text) = core::str::from_utf8(bytes) else {
        return ERROR_BAD_INPUT;
    };
    let scalar_count = text.chars().count();
    if scalar_count == 0 || scalar_count > MAX_DOBBY_TYPE_SCALARS {
        return ERROR_BAD_INPUT;
    }
    let keyboard = match claim_dobby_io(owner) {
        Ok(keyboard) => keyboard,
        Err(error) => return error,
    };
    if !selected_lilly_window().is_ok_and(|window| window.interaction.receives_input) {
        release_dobby_io(owner);
        return ERROR_BAD_STATE;
    }
    match submit_text(
        KeyboardControlPrincipal::Kernel,
        keyboard.handle,
        text,
        DOBBY_KEY_STROKE_MS,
        true,
    ) {
        Ok(count) if count == scalar_count => 0,
        _ => {
            release_dobby_io(owner);
            ERROR_UNAVAILABLE
        }
    }
}

fn named_key_usage(key: u32) -> Option<u16> {
    match key {
        KEY_ENTER => Some(0x28),
        KEY_ESCAPE => Some(0x29),
        KEY_BACKSPACE => Some(0x2A),
        KEY_TAB => Some(0x2B),
        KEY_SPACE => Some(0x2C),
        KEY_ARROW_RIGHT => Some(0x4F),
        KEY_ARROW_LEFT => Some(0x50),
        KEY_ARROW_DOWN => Some(0x51),
        KEY_ARROW_UP => Some(0x52),
        KEY_DELETE => Some(0x4C),
        KEY_HOME => Some(0x4A),
        KEY_END => Some(0x4D),
        KEY_PAGE_UP => Some(0x4B),
        KEY_PAGE_DOWN => Some(0x4E),
        _ => None,
    }
}

pub(crate) fn key(owner: u8, key: u32) -> i32 {
    let Some(key_code) = named_key_usage(key) else {
        return ERROR_BAD_INPUT;
    };
    let keyboard = match claim_dobby_io(owner) {
        Ok(keyboard) => keyboard,
        Err(error) => return error,
    };
    if !selected_lilly_window().is_ok_and(|window| window.interaction.receives_input) {
        release_dobby_io(owner);
        return ERROR_BAD_STATE;
    }
    let command = KeyboardControlCommand {
        opcode: KEYBOARD_CONTROL_OPCODE_STROKE,
        flags: KEYBOARD_CONTROL_FLAG_CLEAR_QUEUE,
        duration_ms: DOBBY_KEY_STROKE_MS,
        key_code,
        ..KeyboardControlCommand::default()
    };
    if submit_command(KeyboardControlPrincipal::Kernel, keyboard.handle, command).is_ok() {
        0
    } else {
        release_dobby_io(owner);
        ERROR_UNAVAILABLE
    }
}

fn current_direct_owner() -> Option<u8> {
    crate::hv::current_guest_execution_context_vm_id()
}

unsafe fn output_slice<'a>(out_ptr: *mut u8, out_cap: usize) -> Result<&'a mut [u8], i32> {
    if out_cap == 0 {
        return Ok(&mut []);
    }
    if out_ptr.is_null() || out_cap > isize::MAX as usize {
        return Err(ERROR_BAD_INPUT);
    }
    Ok(unsafe { core::slice::from_raw_parts_mut(out_ptr, out_cap) })
}

fn guest_result(status: u32, data: u64) -> isize {
    if status == trueos_vm::vmcall::STATUS_OK {
        data as i64 as isize
    } else {
        ERROR_TRANSPORT as isize
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_dobby_ui4_windows(out_ptr: *mut u8, out_cap: usize) -> isize {
    let out = match unsafe { output_slice(out_ptr, out_cap) } {
        Ok(out) => out,
        Err(error) => return error as isize,
    };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, data) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_DOBBY_UI4_WINDOWS,
            out_cap as u64,
            0,
            &[],
            out,
        );
        return guest_result(status, data);
    }
    current_direct_owner()
        .map(|owner| windows(owner, out))
        .unwrap_or(ERROR_DENIED as isize)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_dobby_ui4_focus(window_id: u64) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, data) =
            trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_DOBBY_UI4_FOCUS, window_id, 0);
        return guest_result(status, data) as i32;
    }
    current_direct_owner()
        .map(|owner| focus(owner, window_id))
        .unwrap_or(ERROR_DENIED)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_dobby_ui4_observe_prepare() -> isize {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, data) =
            trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_DOBBY_UI4_OBSERVE_PREPARE, 0, 0);
        return guest_result(status, data);
    }
    current_direct_owner()
        .map(observe_prepare)
        .unwrap_or(ERROR_DENIED as isize)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_dobby_ui4_observe_metadata(
    out_ptr: *mut u8,
    out_cap: usize,
) -> isize {
    let out = match unsafe { output_slice(out_ptr, out_cap) } {
        Ok(out) => out,
        Err(error) => return error as isize,
    };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, data) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_DOBBY_UI4_OBSERVE_METADATA,
            out_cap as u64,
            0,
            &[],
            out,
        );
        return guest_result(status, data);
    }
    current_direct_owner()
        .map(|owner| observe_metadata(owner, out))
        .unwrap_or(ERROR_DENIED as isize)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_dobby_ui4_observe_read(
    offset: usize,
    out_ptr: *mut u8,
    out_cap: usize,
) -> isize {
    let out = match unsafe { output_slice(out_ptr, out_cap) } {
        Ok(out) => out,
        Err(error) => return error as isize,
    };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, data) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_DOBBY_UI4_OBSERVE_READ,
            offset as u64,
            out_cap as u64,
            &[],
            out,
        );
        return guest_result(status, data);
    }
    current_direct_owner()
        .map(|owner| observe_read(owner, offset, out))
        .unwrap_or(ERROR_DENIED as isize)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_dobby_ui4_pointer(x: u16, y: u16, action: u32) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let packed = u64::from(x) | (u64::from(y) << 16);
        let (status, data) = trueos_vm::vmcall::call(
            trueos_vm::vmcall::OP_BP_DOBBY_UI4_POINTER,
            packed,
            u64::from(action),
        );
        return guest_result(status, data) as i32;
    }
    current_direct_owner()
        .map(|owner| pointer(owner, x, y, action))
        .unwrap_or(ERROR_DENIED)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_dobby_ui4_type(text_ptr: *const u8, text_len: usize) -> i32 {
    if text_ptr.is_null() || text_len == 0 || text_len > MAX_DOBBY_TYPE_BYTES {
        return ERROR_BAD_INPUT;
    }
    let text = unsafe { core::slice::from_raw_parts(text_ptr, text_len) };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, data) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_DOBBY_UI4_TYPE,
            0,
            0,
            text,
            &mut [],
        );
        return guest_result(status, data) as i32;
    }
    current_direct_owner()
        .map(|owner| type_text(owner, text))
        .unwrap_or(ERROR_DENIED)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_dobby_ui4_key(key_code: u32) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, data) =
            trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_DOBBY_UI4_KEY, u64::from(key_code), 0);
        return guest_result(status, data) as i32;
    }
    current_direct_owner()
        .map(|owner| key(owner, key_code))
        .unwrap_or(ERROR_DENIED)
}
