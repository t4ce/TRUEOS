//! Mouse-operated LINK layout for grouping keyboard, pointer and gamepad devices.
use crate::usb3::hid::hut::{HidDevice, HidDeviceId, HidDeviceKind, InputCombo};
use alloc::{format, string::String, vec, vec::Vec};

const HEADER: usize = 3;
const CARD_HEIGHT: usize = 7;

#[derive(Default)]
pub(super) struct Snapshot {
    pub devices: Vec<HidDevice>,
    pub buckets: Vec<InputCombo>,
}

impl Snapshot {
    fn bucket_name(&self, id: u32) -> String {
        self.buckets
            .iter()
            .position(|bucket| bucket.combo_id == id)
            .map_or_else(
                || String::from("Unassigned"),
                |index| format!("Bucket {}", index + 1),
            )
    }
}

pub(super) fn device_label(device: &HidDevice) -> String {
    let kind = match device.id.kind {
        HidDeviceKind::Mouse => "Mouse",
        HidDeviceKind::Tablet => "Tablet",
        HidDeviceKind::Keyboard => "Keyboard",
        HidDeviceKind::Gamepad => "Gamepad",
    };
    format!(
        "{}{} {}:{}:{}",
        if device.virtual_device { "V " } else { "" },
        kind,
        device.id.controller_id,
        device.id.slot_id,
        device.id.ep_target
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Command {
    Return,
    NewBucket,
    RemoveBucket(u32),
    MoveDevice(HidDeviceId, u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Event {
    Down(usize, usize),
    Move(usize, usize),
    Up(usize, usize),
    Scroll(usize, bool),
    Cancel,
}

/// Keep partial SGR reports across reads. Only left-button gestures may mutate
/// a bucket; modifiers are accepted, other buttons and hover are ignored.
#[derive(Default)]
pub(super) struct Input {
    sequence: Vec<u8>,
    escape_at: u64,
}
impl Input {
    pub fn feed(&mut self, bytes: &[u8], now: u64) -> Vec<Event> {
        let mut events = Vec::new();
        for &byte in bytes {
            if !self.sequence.is_empty() {
                if self.sequence.len() == 1 && !matches!(byte, b'[' | b'O') {
                    self.sequence.clear();
                    events.push(Event::Cancel);
                    continue;
                }
                self.sequence.push(byte);
                // Refresh the deadline for fragmented transport reports.
                self.escape_at = now;
                if self.sequence.len() > 2 && (0x40..=0x7e).contains(&byte) {
                    if let Some(event) = Self::decode(&self.sequence) {
                        events.push(event);
                    }
                    self.sequence.clear();
                } else if self.sequence.len() > 64 {
                    self.sequence.clear();
                }
            } else if byte == 27 {
                self.sequence.push(byte);
                self.escape_at = now;
            } else if matches!(byte, b'q' | 3) {
                events.push(Event::Cancel);
            }
        }
        events
    }

    pub fn timeout(&mut self, now: u64) -> Option<Event> {
        if !self.sequence.is_empty() && now.saturating_sub(self.escape_at) >= 250_000_000 {
            let lone_escape = self.sequence.len() == 1;
            self.sequence.clear();
            return lone_escape.then_some(Event::Cancel);
        }
        None
    }

    fn decode(bytes: &[u8]) -> Option<Event> {
        let report = bytes.strip_prefix(b"\x1b[<")?;
        let (&end, numbers) = report.split_last()?;
        if !matches!(end, b'M' | b'm') {
            return None;
        }
        let mut fields = core::str::from_utf8(numbers).ok()?.split(';');
        let button = fields.next()?.parse::<u16>().ok()? & !28;
        let col = fields.next()?.parse::<usize>().ok()?.checked_sub(1)?;
        let row = fields.next()?.parse::<usize>().ok()?.checked_sub(1)?;
        if fields.next().is_some() {
            return None;
        }
        match (button, end) {
            (0, b'M') => Some(Event::Down(col, row)),
            (0, b'm') => Some(Event::Up(col, row)),
            (32, b'M') => Some(Event::Move(col, row)),
            (64, b'M') => Some(Event::Scroll(col, true)),
            (65, b'M') => Some(Event::Scroll(col, false)),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hit {
    Return,
    NewBucket,
    Device(HidDeviceId),
    Bucket(u32),
    Remove(u32),
    DevicesPage(bool),
    BucketsPage(bool),
}

#[derive(Clone, Copy)]
struct Rect {
    x: usize,
    y: usize,
    w: usize,
    h: usize,
}
impl Rect {
    fn contains(self, x: usize, y: usize) -> bool {
        x >= self.x && y >= self.y && x - self.x < self.w && y - self.y < self.h
    }
}

pub(super) struct Frame {
    pub lines: Vec<String>,
    hits: Vec<(Rect, Hit)>,
}
impl Frame {
    fn hit(&self, x: usize, y: usize) -> Option<Hit> {
        self.hits
            .iter()
            .rev()
            .find(|(rect, _)| rect.contains(x, y))
            .map(|(_, hit)| *hit)
    }

    fn bucket_at(&self, x: usize, y: usize) -> Option<u32> {
        self.hits.iter().find_map(|(rect, hit)| match hit {
            Hit::Bucket(id) if rect.contains(x, y) => Some(*id),
            _ => None,
        })
    }
}

struct Canvas {
    cells: Vec<Vec<char>>,
    hits: Vec<(Rect, Hit)>,
}
impl Canvas {
    fn new(cols: usize, rows: usize) -> Self {
        Self {
            cells: vec![vec![' '; cols]; rows],
            hits: Vec::new(),
        }
    }
    fn text(&mut self, x: usize, y: usize, width: usize, text: &str) {
        if let Some(row) = self.cells.get_mut(y) {
            for (col, glyph) in (x..x.saturating_add(width)).zip(text.chars()) {
                if let Some(cell) = row.get_mut(col) {
                    *cell = if glyph.is_control() { ' ' } else { glyph };
                }
            }
        }
    }
    fn hit(&mut self, mut rect: Rect, hit: Hit) {
        let rows = self.cells.len();
        let cols = self.cells.first().map_or(0, Vec::len);
        rect.w = rect.w.min(cols.saturating_sub(rect.x));
        rect.h = rect.h.min(rows.saturating_sub(rect.y));
        if rect.w > 0 && rect.h > 0 {
            self.hits.push((rect, hit));
        }
    }
    fn box_at(&mut self, rect: Rect, title: &str, heavy: bool) {
        let (tl, tr, bl, br, h, v) = if heavy {
            ('╔', '╗', '╚', '╝', '═', '║')
        } else {
            ('┌', '┐', '└', '┘', '─', '│')
        };
        for row in 0..rect.h {
            for col in 0..rect.w {
                let glyph = match (row, col) {
                    (0, 0) => tl,
                    (0, c) if c + 1 == rect.w => tr,
                    (r, 0) if r + 1 == rect.h => bl,
                    (r, c) if r + 1 == rect.h && c + 1 == rect.w => br,
                    (0, _) => h,
                    (r, _) if r + 1 == rect.h => h,
                    (_, 0) => v,
                    (_, c) if c + 1 == rect.w => v,
                    _ => ' ',
                };
                if let Some(cell) = self
                    .cells
                    .get_mut(rect.y + row)
                    .and_then(|line| line.get_mut(rect.x + col))
                {
                    *cell = glyph;
                }
            }
        }
        self.text(rect.x + 2, rect.y, rect.w.saturating_sub(4), title);
    }
    fn finish(self) -> Frame {
        Frame {
            lines: self
                .cells
                .into_iter()
                .map(|row| row.into_iter().collect())
                .collect(),
            hits: self.hits,
        }
    }
}

#[derive(Clone, Copy)]
struct Press {
    hit: Hit,
    start: (usize, usize),
    at: (usize, usize),
    dragging: bool,
}

#[derive(Default)]
pub(super) struct View {
    pub status: String,
    selected: Option<HidDeviceId>,
    press: Option<Press>,
    devices_page: usize,
    buckets_page: usize,
    split: usize,
    geometry: (usize, usize),
}

impl View {
    /// Parking, resizing, or a disappearing source cancels a pending gesture.
    pub fn cancel_gesture(&mut self) {
        self.press = None;
        if self.selected.take().is_some() {
            self.status.clear();
        }
    }

    pub fn event(&mut self, event: Event, frame: &Frame) -> Option<Command> {
        match event {
            Event::Cancel => {
                let pressed = self.press.take().is_some();
                let selected = self.selected.take().is_some();
                if pressed || selected {
                    return None;
                }
                return Some(Command::Return);
            }
            Event::Down(x, y) => {
                self.press = frame.hit(x, y).map(|hit| Press {
                    hit,
                    start: (x, y),
                    at: (x, y),
                    dragging: false,
                });
                if let Some(Press {
                    hit: Hit::Device(id),
                    ..
                }) = self.press
                {
                    self.selected = Some(id);
                    self.status =
                        String::from("Drag to a bucket, or click a bucket to link this device.");
                }
            }
            Event::Move(x, y) => {
                if let Some(press) = &mut self.press {
                    press.at = (x, y);
                    press.dragging |= press
                        .start
                        .0
                        .abs_diff(x)
                        .saturating_add(press.start.1.abs_diff(y))
                        >= 2;
                }
            }
            Event::Up(x, y) => {
                let press = self.press.take()?;
                let hit = frame.hit(x, y);
                if let Hit::Device(device) = press.hit {
                    if press.dragging || hit != Some(press.hit) {
                        if let Some(bucket) = frame.bucket_at(x, y) {
                            self.selected = None;
                            return Some(Command::MoveDevice(device, bucket));
                        }
                    }
                }
                if press.dragging {
                    self.status = String::from(
                        "Drop cancelled. Choose a bucket to link the selected device.",
                    );
                    return None;
                }
                if hit != Some(press.hit) {
                    return None;
                }
                return match press.hit {
                    Hit::Return => Some(Command::Return),
                    Hit::NewBucket => Some(Command::NewBucket),
                    Hit::Remove(id) => Some(Command::RemoveBucket(id)),
                    Hit::Bucket(id) => self
                        .selected
                        .take()
                        .map(|device| Command::MoveDevice(device, id)),
                    Hit::DevicesPage(previous) => {
                        Self::page(&mut self.devices_page, previous);
                        None
                    }
                    Hit::BucketsPage(previous) => {
                        Self::page(&mut self.buckets_page, previous);
                        None
                    }
                    Hit::Device(_) => None,
                };
            }
            Event::Scroll(x, previous) => {
                self.press = None;
                Self::page(
                    if x < self.split {
                        &mut self.devices_page
                    } else {
                        &mut self.buckets_page
                    },
                    previous,
                );
            }
        }
        None
    }

    fn page(page: &mut usize, previous: bool) {
        *page = if previous {
            page.saturating_sub(1)
        } else {
            page.saturating_add(1)
        };
    }

    pub fn frame(&mut self, data: &Snapshot, cols: usize, rows: usize) -> Frame {
        if self.geometry != (cols, rows) {
            self.cancel_gesture();
            self.geometry = (cols, rows);
        }
        if self
            .selected
            .is_some_and(|id| !data.devices.iter().any(|device| device.id == id))
        {
            self.selected = None;
            self.cancel_gesture();
            self.status = String::from("Device disconnected. Refreshing live HID devices.");
        }
        let mut canvas = Canvas::new(cols, rows);
        canvas.text(0, 0, cols, "[Return]  LINK  HID combos");
        canvas.hit(
            Rect {
                x: 0,
                y: 0,
                w: cols.min(8),
                h: 1,
            },
            Hit::Return,
        );
        canvas.text(0, 1, cols, "[+ Bucket]  Drag devices into buckets");
        canvas.hit(
            Rect {
                x: 0,
                y: 1,
                w: cols.min(10),
                h: 1,
            },
            Hit::NewBucket,
        );
        let summary = if self.status.is_empty() {
            format!(
                "{} devices · {} buckets · one cursor per bucket",
                data.devices.len(),
                data.buckets.len()
            )
        } else {
            self.status.clone()
        };
        canvas.text(0, 2, cols, &summary);
        if cols < 58 || rows < 12 {
            canvas.text(0, HEADER, cols, "Resize to 58 x 12 to arrange HID devices.");
            return canvas.finish();
        }

        let left = (cols / 3).clamp(22, 32);
        self.split = left + 1;
        let height = rows - HEADER - 1;
        let devices = Rect {
            x: 0,
            y: HEADER,
            w: left,
            h: height,
        };
        canvas.box_at(devices, " Devices ", false);
        let count = ((height - 2) / 2).max(1);
        self.devices_page = self
            .devices_page
            .min(data.devices.len().saturating_sub(1) / count);
        let offset = self.devices_page * count;
        for (index, device) in data.devices.iter().skip(offset).take(count).enumerate() {
            let y = HEADER + 1 + index * 2;
            let selected = self.selected == Some(device.id);
            canvas.text(
                1,
                y,
                left - 2,
                &format!(
                    "{} {}",
                    if selected { "█" } else { "░" },
                    device_label(device)
                ),
            );
            canvas.text(
                3,
                y + 1,
                left - 4,
                &format!("→ {}", data.bucket_name(device.combo_id)),
            );
            canvas.hit(
                Rect {
                    x: 1,
                    y,
                    w: left - 2,
                    h: 2,
                },
                Hit::Device(device.id),
            );
        }
        if data.devices.is_empty() {
            canvas.text(2, HEADER + 2, left - 4, "No live HID devices");
        }
        Self::pager(
            &mut canvas,
            0,
            rows - 1,
            left,
            self.devices_page,
            data.devices.len().div_ceil(count).max(1),
            true,
        );

        let available = cols - self.split;
        let columns = (available / 28).max(1);
        let card_width = available / columns;
        let largest_pointer_group = data.buckets.iter().map(|bucket| data.devices.iter().filter(|device| {
            device.combo_id == bucket.combo_id && matches!(device.id.kind, HidDeviceKind::Mouse | HidDeviceKind::Tablet)
        }).count()).max().unwrap_or(0);
        let pointer_rows = largest_pointer_group.max(1).min(height.saturating_sub(6).max(1));
        let card_height = CARD_HEIGHT.max(pointer_rows + 6);
        let per_page = columns * (height / card_height).max(1);
        self.buckets_page = self
            .buckets_page
            .min(data.buckets.len().saturating_sub(1) / per_page);
        let offset = self.buckets_page * per_page;
        for (index, bucket) in data.buckets.iter().skip(offset).take(per_page).enumerate() {
            let rect = Rect {
                x: self.split + index % columns * card_width,
                y: HEADER + index / columns * card_height,
                w: card_width - 1,
                h: card_height,
            };
            let hovered = self
                .press
                .is_some_and(|press| press.dragging && rect.contains(press.at.0, press.at.1));
            canvas.box_at(
                rect,
                &format!(
                    " {}Bucket {} ",
                    if hovered { "▼ " } else { "" },
                    offset + index + 1
                ),
                hovered,
            );
            canvas.hit(rect, Hit::Bucket(bucket.combo_id));
            let pointers: Vec<_> = data.devices.iter().filter(|device| {
                device.combo_id == bucket.combo_id
                    && matches!(device.id.kind, HidDeviceKind::Mouse | HidDeviceKind::Tablet)
            }).collect();
            let keyboard = data.devices.iter().find(|device| {
                device.combo_id == bucket.combo_id && device.id.kind == HidDeviceKind::Keyboard
            });
            let gamepad = data.devices.iter().find(|device| {
                device.combo_id == bucket.combo_id && device.id.kind == HidDeviceKind::Gamepad
            });
            let member_rows = pointers.len().max(1).min(pointer_rows);
            if pointers.is_empty() {
                canvas.text(rect.x + 2, rect.y + 1, rect.w - 4, "░ No cursor");
            }
            for (index, device) in pointers.iter().take(member_rows).enumerate() {
                canvas.text(rect.x + 2, rect.y + 1 + index, rect.w - 4, &format!("█ {}", device_label(device)));
                canvas.hit(Rect { x: rect.x + 1, y: rect.y + 1 + index, w: rect.w - 2, h: 1 }, Hit::Device(device.id));
            }
            let keyboard_row = rect.y + member_rows + 1;
            let gamepad_row = keyboard_row + 1;
            let tag_row = keyboard_row + 2;
            let footer_row = keyboard_row + 3;
            canvas.text(
                rect.x + 2,
                keyboard_row,
                rect.w - 4,
                &keyboard.map_or_else(
                    || String::from("░ No keyboard"),
                    |device| format!("█ {}", device_label(device)),
                ),
            );
            canvas.text(
                rect.x + 2,
                gamepad_row,
                rect.w - 4,
                &gamepad.map_or_else(
                    || String::from("░ No gamepad"),
                    |device| format!("█ {}", device_label(device)),
                ),
            );
            for (row, device) in [
                (keyboard_row, keyboard),
                (gamepad_row, gamepad),
            ] {
                if let Some(device) = device {
                    canvas.hit(
                        Rect {
                            x: rect.x + 1,
                            y: row,
                            w: rect.w - 2,
                            h: 1,
                        },
                        Hit::Device(device.id),
                    );
                }
            }
            let tag = if bucket.source_tag.is_empty() {
                String::from("manual")
            } else {
                format!("{}", bucket.source_tag)
            };
            canvas.text(
                rect.x + 2,
                tag_row,
                rect.w - 4,
                &format!("#{:08x} {}", bucket.combo_id, tag),
            );
            // Check endpoint membership, including unavailable devices and
            // gamepads, before offering removal of an empty bucket.
            let empty = bucket.mouse_slot_id == 0
                && bucket.tablet_slot_id == 0
                && bucket.keyboard_slot_id == 0
                && bucket.gamepad_slot_id == 0
                && !data.devices.iter().any(|device| device.combo_id == bucket.combo_id);
            if empty {
                canvas.text(rect.x + 2, footer_row, rect.w - 4, "[Remove empty]");
                canvas.hit(
                    Rect {
                        x: rect.x + 2,
                        y: footer_row,
                        w: 14.min(rect.w - 4),
                        h: 1,
                    },
                    Hit::Remove(bucket.combo_id),
                );
            } else {
                canvas.text(
                    rect.x + 2,
                    footer_row,
                    rect.w - 4,
                    &if pointers.len() > 1 { format!("Pointers {} → 1 cursor", pointers.len()) } else { format!(
                        "Cursor {}/1 · Pad {}/1",
                        usize::from(!pointers.is_empty()),
                        usize::from(gamepad.is_some())
                    ) },
                );
            }
        }
        if data.buckets.is_empty() {
            canvas.text(
                self.split,
                HEADER + 1,
                available,
                "Click [+ Bucket] to make a combo.",
            );
        }
        Self::pager(
            &mut canvas,
            self.split,
            rows - 1,
            available,
            self.buckets_page,
            data.buckets.len().div_ceil(per_page).max(1),
            false,
        );

        if let Some(press) = self.press.filter(|press| press.dragging) {
            if let Hit::Device(id) = press.hit {
                if let Some(device) = data.devices.iter().find(|device| device.id == id) {
                    let label = device_label(device);
                    let width = (label.chars().count() + 4).min(28).min(cols);
                    let x = press.at.0.saturating_add(1).min(cols - width);
                    let y = press.at.1.saturating_add(1).min(rows - 3).max(HEADER);
                    canvas.box_at(
                        Rect {
                            x,
                            y,
                            w: width,
                            h: 3,
                        },
                        "",
                        false,
                    );
                    canvas.text(x + 2, y + 1, width - 4, &label);
                }
            }
        }
        canvas.finish()
    }

    fn pager(
        canvas: &mut Canvas,
        x: usize,
        y: usize,
        width: usize,
        page: usize,
        pages: usize,
        devices: bool,
    ) {
        canvas.text(x, y, width, &format!("[<] {}/{}", page + 1, pages));
        canvas.text(x + width - 3, y, 3, "[>]");
        let hit = |previous| {
            if devices {
                Hit::DevicesPage(previous)
            } else {
                Hit::BucketsPage(previous)
            }
        };
        canvas.hit(Rect { x, y, w: 3, h: 1 }, hit(true));
        canvas.hit(
            Rect {
                x: x + width - 3,
                y,
                w: 3,
                h: 1,
            },
            hit(false),
        );
    }
}

/// Default body colors also work on SSH. Paint the control strip explicitly
/// there, matching the other system TUIs, and keep the terminal caret hidden.
#[derive(Default)]
pub(super) struct Screen {
    previous: Vec<String>,
    geometry: (usize, usize),
}
impl Screen {
    pub fn diff(&mut self, frame: &Frame, cols: usize, rows: usize) -> String {
        let mut bytes = String::new();
        if self.geometry != (cols, rows) {
            bytes.push_str("\x1b[?25l\x1b[?1000h\x1b[?1002h\x1b[?1006h\x1b[0m\x1b[2J");
            self.geometry = (cols, rows);
            self.previous.clear();
        }
        for (row, line) in frame.lines.iter().take(rows).enumerate() {
            if self.previous.get(row) != Some(line) {
                bytes.push_str(&format!(
                    "\x1b[{};1H{}{}\x1b[0m",
                    row + 1,
                    if row < HEADER {
                        "\x1b[48;2;16;16;16m\x1b[38;2;255;105;180m"
                    } else {
                        "\x1b[0m"
                    },
                    line
                ));
            }
        }
        self.previous.clone_from(&frame.lines);
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usb3::hid::hut::HidSourceKind;

    fn device(index: u32, kind: HidDeviceKind) -> HidDevice {
        HidDevice {
            id: HidDeviceId {
                kind,
                controller_id: 1,
                slot_id: index + 1,
                ep_target: 1,
            },
            combo_id: 0,
            source_kind: HidSourceKind::Human,
            source_tag: heapless::String::new(),
            virtual_device: false,
        }
    }

    fn bucket(id: u32) -> InputCombo {
        InputCombo {
            combo_id: id,
            source_kind: HidSourceKind::Human,
            source_tag: heapless::String::new(),
            color_id: 0,
            flags: 0,
            mouse_controller_id: 0,
            mouse_slot_id: 0,
            mouse_ep_target: 0,
            keyboard_controller_id: 0,
            keyboard_slot_id: 0,
            keyboard_ep_target: 0,
            tablet_controller_id: 0,
            tablet_slot_id: 0,
            tablet_ep_target: 0,
            gamepad_controller_id: 0,
            gamepad_slot_id: 0,
            gamepad_ep_target: 0,
        }
    }

    fn sample() -> Snapshot {
        Snapshot {
            devices: vec![
                device(0, HidDeviceKind::Mouse),
                device(1, HidDeviceKind::Keyboard),
            ],
            buckets: vec![bucket(10), bucket(20)],
        }
    }

    fn coordinate(frame: &Frame, wanted: Hit) -> (usize, usize) {
        let (rect, _) = frame.hits.iter().find(|(_, hit)| *hit == wanted).unwrap();
        (rect.x, rect.y)
    }

    fn click(view: &mut View, frame: &Frame, at: (usize, usize)) -> Option<Command> {
        assert_eq!(view.event(Event::Down(at.0, at.1), frame), None);
        view.event(Event::Up(at.0, at.1), frame)
    }

    #[test]
    fn fragmented_sgr_reports_keep_down_drag_and_release_in_order() {
        let reports = b"\x1b[<0;7;5M\x1b[<32;40;7M\x1b[<0;40;7m";
        let expected = vec![Event::Down(6, 4), Event::Move(39, 6), Event::Up(39, 6)];
        for split in 0..=reports.len() {
            let mut input = Input::default();
            let mut events = input.feed(&reports[..split], 0);
            events.extend(input.feed(&reports[split..], 100_000_000));
            assert_eq!(events, expected, "split at {split}");
        }
        let mut input = Input::default();
        let events: Vec<_> = reports
            .iter()
            .enumerate()
            .flat_map(|(index, byte)| {
                input.feed(core::slice::from_ref(byte), index as u64 * 100_000_000)
            })
            .collect();
        assert_eq!(events, expected);
        assert_eq!(
            input.feed(b"\x1b[<28;2;3M\x1b[<60;4;5M\x1b[<28;4;5m", 0),
            vec![Event::Down(1, 2), Event::Move(3, 4), Event::Up(3, 4)]
        );
    }

    #[test]
    fn malformed_reports_non_left_buttons_and_hover_cannot_mutate_buckets() {
        let invalid: &[&[u8]] = &[
            b"\x1b[<0;0;1M",
            b"\x1b[<0;1;0M",
            b"\x1b[<0;1;1;1M",
            b"\x1b[<1;1;1M",
            b"\x1b[<2;1;1M",
            b"\x1b[<35;1;1M",
            b"\x1b[<32;1;1m",
            b"\x1b[<64;1;1m",
            b"\x1b[<65536;1;1M",
            b"\x1b[<0;18446744073709551616;1M",
            b"\x1b[<0;;1M",
            b"\x1b[A",
        ];
        for report in invalid {
            let mut input = Input::default();
            assert!(input.feed(report, 0).is_empty(), "{report:?}");
            assert_eq!(input.feed(b"\x1b[<0;2;3M", 1), vec![Event::Down(1, 2)]);
        }
        let mut input = Input::default();
        let mut oversized = b"\x1b[<".to_vec();
        oversized.extend(core::iter::repeat_n(b'1', 100));
        assert!(input.feed(&oversized, 0).is_empty());
        assert_eq!(
            input.feed(b"\x1b[<64;2;3M\x1b[<65;2;3M", 1),
            vec![Event::Scroll(1, true), Event::Scroll(1, false)]
        );
    }

    #[test]
    fn escape_timeout_refreshes_on_fragments_and_discards_incomplete_reports() {
        let mut input = Input::default();
        assert!(input.feed(b"\x1b", 0).is_empty());
        assert_eq!(input.timeout(249_999_999), None);
        assert_eq!(input.timeout(250_000_000), Some(Event::Cancel));
        assert!(input.feed(b"\x1b[<0", 300_000_000).is_empty());
        assert!(input.feed(b";2", 500_000_000).is_empty());
        assert_eq!(input.timeout(700_000_000), None);
        assert_eq!(input.timeout(750_000_000), None);
        assert!(input.sequence.is_empty());
        assert_eq!(
            input.feed(b"\x1b[<0;2;3M", 800_000_000),
            vec![Event::Down(1, 2)]
        );
    }

    #[test]
    fn mouse_can_create_a_bucket_drag_a_cursor_and_click_to_link_a_keyboard() {
        let mut data = sample();
        data.buckets.clear();
        let mut view = View::default();
        let frame = view.frame(&data, 100, 24);
        assert_eq!(
            click(&mut view, &frame, coordinate(&frame, Hit::NewBucket)),
            Some(Command::NewBucket)
        );
        data.buckets.push(bucket(10));
        let frame = view.frame(&data, 100, 24);
        let mouse = data.devices[0].id;
        let source = coordinate(&frame, Hit::Device(mouse));
        let target = coordinate(&frame, Hit::Remove(10));
        view.event(Event::Down(source.0, source.1), &frame);
        view.event(Event::Move(target.0, target.1), &frame);
        let frame = view.frame(&data, 100, 24);
        // The empty-card remove control is also part of the drag destination.
        assert_eq!(
            view.event(Event::Up(target.0, target.1), &frame),
            Some(Command::MoveDevice(mouse, 10))
        );
        data.devices[0].combo_id = 10;
        data.buckets[0].mouse_controller_id = mouse.controller_id;
        data.buckets[0].mouse_slot_id = mouse.slot_id;
        data.buckets[0].mouse_ep_target = mouse.ep_target;
        let frame = view.frame(&data, 100, 24);
        assert!(!frame.hits.iter().any(|(_, hit)| *hit == Hit::Remove(10)));
        let keyboard = data.devices[1].id;
        assert_eq!(
            click(&mut view, &frame, coordinate(&frame, Hit::Device(keyboard))),
            None
        );
        assert_eq!(view.selected, Some(keyboard));
        assert_eq!(
            click(&mut view, &frame, coordinate(&frame, Hit::Bucket(10))),
            Some(Command::MoveDevice(keyboard, 10))
        );
        assert_eq!(view.selected, None);
        assert_eq!(
            click(&mut view, &frame, coordinate(&frame, Hit::Return)),
            Some(Command::Return)
        );
    }

    #[test]
    fn linked_device_rows_are_draggable_and_plain_click_keeps_selection() {
        let mut data = sample();
        let mouse = data.devices[0].id;
        data.devices[0].combo_id = 10;
        data.buckets[0].mouse_controller_id = mouse.controller_id;
        data.buckets[0].mouse_slot_id = mouse.slot_id;
        data.buckets[0].mouse_ep_target = mouse.ep_target;
        let mut view = View::default();
        let frame = view.frame(&data, 100, 24);
        let (rect, _) = frame
            .hits
            .iter()
            .find(|(rect, hit)| rect.x >= view.split && *hit == Hit::Device(mouse))
            .unwrap();
        let source = (rect.x, rect.y);
        assert_eq!(click(&mut view, &frame, source), None);
        assert_eq!(view.selected, Some(mouse));
        let target = coordinate(&frame, Hit::Bucket(20));
        view.event(Event::Down(source.0, source.1), &frame);
        view.event(Event::Move(target.0, target.1), &frame);
        assert_eq!(
            view.event(Event::Up(target.0, target.1), &frame),
            Some(Command::MoveDevice(mouse, 20))
        );
    }

    #[test]
    fn grouped_physical_pointer_members_are_visible_and_individually_splittable() {
        let mut data = sample();
        data.devices.push(device(2, HidDeviceKind::Gamepad));
        let mut trackpad = device(8, HidDeviceKind::Mouse);
        let mut finger = device(9, HidDeviceKind::Tablet);
        trackpad.combo_id = 10;
        finger.combo_id = 10;
        data.devices[0].combo_id = 10;
        data.devices[1].combo_id = 10;
        data.devices[2].combo_id = 10;
        let canonical = data.devices[0].id;
        data.buckets[0].mouse_controller_id = canonical.controller_id;
        data.buckets[0].mouse_slot_id = canonical.slot_id;
        data.buckets[0].mouse_ep_target = canonical.ep_target;
        let alias = trackpad.id;
        data.devices.push(trackpad);
        data.devices.push(finger);
        let mut view = View::default();
        let frame = view.frame(&data, 100, 24);
        let (bucket, _) = frame.hits.iter().find(|(_, hit)| *hit == Hit::Bucket(10)).unwrap();
        assert!(frame.lines[bucket.y + 7].contains("Pointers 3 → 1 cursor"));
        for member in &data.devices {
            assert!(frame.hits.iter().any(|(rect, hit)| rect.x >= view.split && *hit == Hit::Device(member.id)));
        }
        let (rect, _) = frame.hits.iter().find(|(rect, hit)| rect.x >= view.split && *hit == Hit::Device(alias)).unwrap();
        let source = (rect.x, rect.y);
        let target = coordinate(&frame, Hit::Bucket(20));
        view.event(Event::Down(source.0, source.1), &frame);
        view.event(Event::Move(target.0, target.1), &frame);
        let frame = view.frame(&data, 100, 24);
        assert_eq!(view.event(Event::Up(target.0, target.1), &frame), Some(Command::MoveDevice(alias, 20)));
        // Per-device membership, not only the canonical wire slots, protects a
        // card from offering deletion while aliased pointers are still live.
        data.buckets[0].mouse_controller_id = 0;
        data.buckets[0].mouse_slot_id = 0;
        data.buckets[0].mouse_ep_target = 0;
        let frame = view.frame(&data, 100, 24);
        assert!(!frame.hits.iter().any(|(_, hit)| *hit == Hit::Remove(10)));
    }

    #[test]
    fn min_size_pagers_and_wheel_reach_every_device_and_bucket() {
        let data = Snapshot {
            devices: (0..20)
                .map(|index| device(index, HidDeviceKind::Mouse))
                .collect(),
            buckets: (1..=20).map(bucket).collect(),
        };
        let mut view = View::default();
        for expected in 0..20 {
            let frame = view.frame(&data, 58, 12);
            assert!(
                frame
                    .hits
                    .iter()
                    .any(|(_, hit)| *hit == Hit::Bucket(expected + 1))
            );
            assert_eq!(
                click(
                    &mut view,
                    &frame,
                    coordinate(&frame, Hit::BucketsPage(false))
                ),
                None
            );
        }
        let frame = view.frame(&data, 58, 12);
        assert_eq!(view.buckets_page, 19);
        view.event(Event::Scroll(view.split, true), &frame);
        view.frame(&data, 58, 12);
        assert_eq!(view.buckets_page, 18);
        for page in 0..7 {
            let frame = view.frame(&data, 58, 12);
            let first = data.devices[page * 3].id;
            assert!(frame.hits.iter().any(|(_, hit)| *hit == Hit::Device(first)));
            click(
                &mut view,
                &frame,
                coordinate(&frame, Hit::DevicesPage(false)),
            );
        }
        let frame = view.frame(&data, 58, 12);
        assert_eq!(view.devices_page, 6);
        view.event(Event::Scroll(0, true), &frame);
        view.frame(&data, 58, 12);
        assert_eq!(view.devices_page, 5);
        assert_eq!(view.buckets_page, 18);
    }

    #[test]
    fn layouts_keep_visible_cells_and_hit_targets_within_all_viewport_sizes() {
        let data = sample();
        let mut view = View::default();
        for cols in [0, 1, 8, 10, 57, 58, 59, 80, 160] {
            for rows in [0, 1, 2, 3, 4, 11, 12, 13, 24, 50] {
                let frame = view.frame(&data, cols, rows);
                assert_eq!(frame.lines.len(), rows);
                assert!(frame.lines.iter().all(|line| line.chars().count() == cols));
                for (rect, hit) in &frame.hits {
                    assert!(
                        rect.w > 0
                            && rect.h > 0
                            && rect.x + rect.w <= cols
                            && rect.y + rect.h <= rows,
                        "{hit:?} outside {cols} x {rows}"
                    );
                }
                assert_eq!(frame.hit(cols, 0), None);
                assert_eq!(frame.hit(0, rows), None);
            }
        }
    }

    #[test]
    fn dragging_never_covers_headers_and_extreme_mouse_coordinates_do_not_overflow() {
        let data = sample();
        let mut view = View::default();
        let frame = view.frame(&data, 80, 24);
        let source = coordinate(&frame, Hit::Device(data.devices[0].id));
        view.event(Event::Down(source.0, source.1), &frame);
        let frame = view.frame(&data, 80, 24);
        let headers = frame.lines[..HEADER].to_vec();
        for (x, y) in [(0, 0), (79, 23), (usize::MAX, usize::MAX)] {
            view.event(Event::Move(x, y), &frame);
            let frame = view.frame(&data, 80, 24);
            assert_eq!(frame.lines[..HEADER], headers);
            assert!(frame.lines.iter().all(|line| line.chars().count() == 80));
        }
    }

    #[test]
    fn resize_disconnection_and_parking_cancel_pending_drops() {
        let mut data = sample();
        let mut view = View::default();
        for interrupt in 0..3 {
            let frame = view.frame(&data, 80, 24);
            let source = coordinate(&frame, Hit::Device(data.devices[0].id));
            let target = coordinate(&frame, Hit::Bucket(10));
            view.event(Event::Down(source.0, source.1), &frame);
            view.event(Event::Move(target.0, target.1), &frame);
            let frame = match interrupt {
                0 => view.frame(&data, 81, 24),
                1 => {
                    view.cancel_gesture();
                    view.frame(&data, 80, 24)
                }
                _ => {
                    data.devices.clear();
                    view.frame(&data, 80, 24)
                }
            };
            assert_eq!(view.event(Event::Up(target.0, target.1), &frame), None);
            assert!(view.press.is_none());
        }
        assert_eq!(view.selected, None);
        assert!(view.status.contains("disconnected"));
    }

    #[test]
    fn screen_colors_only_three_header_rows_and_only_repaints_changed_rows() {
        let data = sample();
        let mut view = View::default();
        let mut screen = Screen::default();
        let frame = view.frame(&data, 80, 24);
        let bytes = screen.diff(&frame, 80, 24);
        assert_eq!(bytes.matches("\x1b[48;2;16;16;16m").count(), HEADER);
        assert!(bytes.contains("\x1b[?1002h\x1b[?1006h"));
        assert!(bytes.contains("\x1b[4;1H\x1b[0m"));
        assert_eq!(screen.diff(&frame, 80, 24), "");
        view.status = String::from("Device linked");
        let frame = view.frame(&data, 80, 24);
        let update = screen.diff(&frame, 80, 24);
        assert!(update.starts_with("\x1b[3;1H"));
        assert_eq!(update.matches("\x1b[48;2;16;16;16m").count(), 1);
        assert!(!update.contains("\x1b[2J"));
    }

    #[test]
    fn actual_terminal_roundtrip_preserves_box_layout_colors_and_drag_reports() {
        use trueos_terminal::{MouseButton, MouseEncoding, MouseTracking, Terminal, TerminalColor};

        let data = sample();
        let mut view = View::default();
        let mut screen = Screen::default();
        let mut input = Input::default();
        let mut terminal = Terminal::new(80, 24);
        let mut frame = view.frame(&data, 80, 24);
        terminal.feed(screen.diff(&frame, 80, 24).as_bytes());
        assert_eq!(terminal.mouse_options().tracking, MouseTracking::Drag);
        assert_eq!(terminal.mouse_options().encoding, MouseEncoding::Sgr);
        assert!(!terminal.cursor().visible);
        assert_eq!(terminal.render_rows(), frame.lines);
        let device = data.devices[0].id;
        let source = coordinate(&frame, Hit::Device(device));
        let target = coordinate(&frame, Hit::Remove(10));
        let reports = [
            terminal
                .mouse_button(MouseButton::Left, true, source.0, source.1)
                .unwrap(),
            terminal
                .mouse_motion(Some(MouseButton::Left), target.0, target.1)
                .unwrap(),
            terminal
                .mouse_button(MouseButton::Left, false, target.0, target.1)
                .unwrap(),
        ];
        let mut commands = Vec::new();
        for report in reports {
            for event in input.feed(&report, 0) {
                if let Some(command) = view.event(event, &frame) {
                    commands.push(command);
                }
                frame = view.frame(&data, 80, 24);
                terminal.feed(screen.diff(&frame, 80, 24).as_bytes());
                assert_eq!(terminal.render_rows(), frame.lines);
                for (index, cell) in terminal.cells().iter().enumerate() {
                    let header = index / 80 < HEADER;
                    assert_eq!(
                        cell.style.background,
                        if header {
                            TerminalColor::Rgb {
                                red: 16,
                                green: 16,
                                blue: 16,
                            }
                        } else {
                            TerminalColor::Default
                        }
                    );
                    assert_eq!(
                        cell.style.foreground,
                        if header {
                            TerminalColor::Rgb {
                                red: 255,
                                green: 105,
                                blue: 180,
                            }
                        } else {
                            TerminalColor::Default
                        }
                    );
                }
            }
        }
        assert_eq!(commands, vec![Command::MoveDevice(device, 10)]);
    }

    #[test]
    fn all_four_live_hid_classes_have_distinct_physical_and_virtual_labels() {
        for (kind, name) in [
            (HidDeviceKind::Mouse, "Mouse"),
            (HidDeviceKind::Tablet, "Tablet"),
            (HidDeviceKind::Keyboard, "Keyboard"),
            (HidDeviceKind::Gamepad, "Gamepad"),
        ] {
            let mut entry = device(4, kind);
            assert_eq!(device_label(&entry), format!("{name} 1:5:1"));
            entry.virtual_device = true;
            assert_eq!(device_label(&entry), format!("V {name} 1:5:1"));
        }
    }

    #[test]
    fn mouse_or_tablet_share_one_cursor_slot_while_keyboard_and_gamepad_remain_independent() {
        for pointer_kind in [
            None,
            Some(HidDeviceKind::Mouse),
            Some(HidDeviceKind::Tablet),
        ] {
            let mut data = Snapshot {
                devices: vec![
                    device(1, HidDeviceKind::Keyboard),
                    device(2, HidDeviceKind::Gamepad),
                ],
                buckets: vec![bucket(10)],
            };
            if let Some(kind) = pointer_kind {
                data.devices.push(device(0, kind));
                match kind {
                    HidDeviceKind::Mouse => {
                        data.buckets[0].mouse_controller_id = 1;
                        data.buckets[0].mouse_slot_id = 1;
                        data.buckets[0].mouse_ep_target = 1;
                    }
                    HidDeviceKind::Tablet => {
                        data.buckets[0].tablet_controller_id = 1;
                        data.buckets[0].tablet_slot_id = 1;
                        data.buckets[0].tablet_ep_target = 1;
                    }
                    _ => unreachable!(),
                }
            }
            for entry in &mut data.devices {
                entry.combo_id = 10;
            }
            data.buckets[0].keyboard_controller_id = 1;
            data.buckets[0].keyboard_slot_id = 2;
            data.buckets[0].keyboard_ep_target = 1;
            data.buckets[0].gamepad_controller_id = 1;
            data.buckets[0].gamepad_slot_id = 3;
            data.buckets[0].gamepad_ep_target = 1;
            let mut view = View::default();
            let frame = view.frame(&data, 80, 24);
            let (rect, _) = frame
                .hits
                .iter()
                .find(|(_, hit)| *hit == Hit::Bucket(10))
                .unwrap();
            let footer = &frame.lines[rect.y + 5];
            assert!(footer.contains(&format!(
                "Cursor {}/1 · Pad 1/1",
                usize::from(pointer_kind.is_some())
            )));
            assert!(frame.lines[rect.y + 2].contains("Keyboard"));
            assert!(frame.lines[rect.y + 3].contains("Gamepad"));
            if let Some(kind) = pointer_kind {
                assert!(frame.lines[rect.y + 1].contains(match kind {
                    HidDeviceKind::Mouse => "Mouse",
                    HidDeviceKind::Tablet => "Tablet",
                    _ => unreachable!(),
                }));
            } else {
                assert!(frame.lines[rect.y + 1].contains("No cursor"));
            }
            assert!(!frame.hits.iter().any(|(_, hit)| *hit == Hit::Remove(10)));
            for entry in &data.devices {
                assert!(frame.hits.iter().any(|(hit_rect, hit)| {
                    hit_rect.x >= view.split && *hit == Hit::Device(entry.id)
                }));
            }
        }
    }

    #[test]
    fn gamepad_can_be_dragged_from_device_list_then_from_its_linked_bucket_row() {
        let mut data = sample();
        let mut pad = device(2, HidDeviceKind::Gamepad);
        pad.virtual_device = true;
        let pad_id = pad.id;
        data.devices.push(pad);
        let mut view = View::default();
        let frame = view.frame(&data, 100, 24);
        let source = coordinate(&frame, Hit::Device(pad_id));
        let target = coordinate(&frame, Hit::Remove(10));
        view.event(Event::Down(source.0, source.1), &frame);
        view.event(Event::Move(target.0, target.1), &frame);
        assert_eq!(
            view.event(Event::Up(target.0, target.1), &frame),
            Some(Command::MoveDevice(pad_id, 10))
        );
        data.devices.last_mut().unwrap().combo_id = 10;
        data.buckets[0].gamepad_controller_id = pad_id.controller_id;
        data.buckets[0].gamepad_slot_id = pad_id.slot_id;
        data.buckets[0].gamepad_ep_target = pad_id.ep_target;
        let frame = view.frame(&data, 100, 24);
        let (source, _) = frame
            .hits
            .iter()
            .find(|(rect, hit)| rect.x >= view.split && *hit == Hit::Device(pad_id))
            .unwrap();
        assert!(frame.lines[source.y].contains("V Gamepad"));
        let target = coordinate(&frame, Hit::Remove(20));
        view.event(Event::Down(source.x, source.y), &frame);
        view.event(Event::Move(target.0, target.1), &frame);
        assert_eq!(
            view.event(Event::Up(target.0, target.1), &frame),
            Some(Command::MoveDevice(pad_id, 20))
        );
    }
}
