//! Cursor-scoped, copied drag payloads and previews. Apps never own slot 4.
use super::{CursorFrameKey, Ui4CursorSource, WindowOwner};
use alloc::{sync::Arc, vec, vec::Vec};

pub(crate) const MAX_LABEL_BYTES: usize = 256;
pub(crate) const MAX_PAYLOAD_BYTES: usize = 3072;
const MAX_DRAGS: usize = 32;
const MAX_DROPS: usize = 32;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DragPreview {
    pub width: u32,
    pub height: u32,
    /// MicroFont row runs, rasterized once when the app publishes its label.
    pub runs: Vec<(u32, u32, u32)>,
}

fn preview(label: &str) -> Arc<DragPreview> {
    let text: alloc::string::String = label.chars().take(48).collect();
    let width = (text.chars().count().max(1) * microfont::FWIDTH) as u32;
    let height = microfont::FHEIGHT as u32;
    let mut pixels = vec![0u8; width as usize * height as usize];
    let _ = microfont::stamp_text(&mut pixels, width as usize, height as usize, 0, 0, &text, 1u8);
    let mut runs = Vec::new();
    for (y, row) in pixels.chunks_exact(width as usize).enumerate() {
        let mut x = 0;
        while x < row.len() {
            if row[x] == 0 {
                x += 1;
                continue;
            }
            let start = x;
            while x < row.len() && row[x] != 0 {
                x += 1;
            }
            runs.push((start as u32, y as u32, (x - start) as u32));
        }
    }
    Arc::new(DragPreview {
        width,
        height,
        runs,
    })
}

struct Drag {
    token: i32,
    producer: WindowOwner,
    origin: CursorFrameKey,
    source: Ui4CursorSource,
    preview: Arc<DragPreview>,
    kind: u32,
    data: Vec<u8>,
}
struct Drop {
    recipient: WindowOwner,
    target: CursorFrameKey,
    kind: u32,
    x: i32,
    y: i32,
    data: Vec<u8>,
}
struct Registry {
    next_token: i32,
    drags: Vec<Drag>,
    drops: Vec<Drop>,
    completed: Vec<(WindowOwner, i32, CursorFrameKey, bool)>,
}
impl Registry {
    const fn new() -> Self {
        Self {
            next_token: 0,
            drags: Vec::new(),
            drops: Vec::new(),
            completed: Vec::new(),
        }
    }
    fn begin(
        &mut self,
        producer: WindowOwner,
        origin: CursorFrameKey,
        source: Ui4CursorSource,
        kind: u32,
        label: &str,
        data: &[u8],
    ) -> Result<i32, i32> {
        if label.is_empty()
            || label.len() > MAX_LABEL_BYTES
            || label.chars().any(char::is_control)
            || data.len() > MAX_PAYLOAD_BYTES
        {
            return Err(-1);
        }
        if self.drags.iter().any(|drag| drag.source == source)
            || self.drags.len() >= MAX_DRAGS
            || self.drags.len() + self.completed.len() >= 64
        {
            return Err(-5);
        }
        self.next_token = self.next_token.checked_add(1).ok_or(-4)?;
        let token = self.next_token;
        self.drags.push(Drag {
            token,
            producer,
            origin,
            source,
            preview: preview(label),
            kind,
            data: data.to_vec(),
        });
        Ok(token)
    }
    fn cancel(&mut self, producer: WindowOwner, token: i32) -> bool {
        self.drags
            .retain(|drag| drag.producer != producer || drag.token != token);
        if let Some(index) = self
            .completed
            .iter()
            .position(|entry| entry.0 == producer && entry.1 == token)
        {
            return self.completed.remove(index).3;
        }
        false
    }
    fn finish(
        &mut self,
        source: Ui4CursorSource,
        target: Option<(WindowOwner, CursorFrameKey, i32, i32)>,
    ) {
        let Some(index) = self.drags.iter().position(|drag| drag.source == source) else {
            return;
        };
        let drag = self.drags.remove(index);
        let cross_frame = target.is_some_and(|(_, frame, _, _)| frame != drag.origin);
        self.completed
            .push((drag.producer, drag.token, drag.origin, cross_frame));
        if let Some((recipient, target, x, y)) = target {
            // Local drops keep the app's existing handler; only cross-frame
            // drops enter the receiving frame's mailbox.
            if target == drag.origin {
                return;
            }
            if self.drops.len() == MAX_DROPS {
                self.drops.remove(0);
            }
            self.drops.push(Drop {
                recipient,
                target,
                kind: drag.kind,
                x,
                y,
                data: drag.data,
            });
        }
    }
    fn take(
        &mut self,
        recipient: WindowOwner,
        target: CursorFrameKey,
        out: &mut [u8],
    ) -> Result<usize, i32> {
        let Some(index) = self
            .drops
            .iter()
            .position(|drop| drop.recipient == recipient && drop.target == target)
        else {
            return Ok(0);
        };
        let drop = &self.drops[index];
        let len = 16 + drop.data.len();
        if out.len() < len {
            return Err(-4);
        }
        out[..4].copy_from_slice(&drop.kind.to_le_bytes());
        out[4..8].copy_from_slice(&drop.x.to_le_bytes());
        out[8..12].copy_from_slice(&drop.y.to_le_bytes());
        out[12..16].copy_from_slice(&(drop.data.len() as u32).to_le_bytes());
        out[16..len].copy_from_slice(&drop.data);
        self.drops.remove(index);
        Ok(len)
    }
    fn release_owner(&mut self, owner: WindowOwner) {
        self.drags
            .retain(|drag| drag.producer != owner && drag.origin.owner != owner);
        self.drops
            .retain(|drop| drop.recipient != owner && drop.target.owner != owner);
        self.completed.retain(|entry| entry.0 != owner);
    }
}
static REGISTRY: spin::Mutex<Registry> = spin::Mutex::new(Registry::new());

pub(crate) fn begin(
    producer: WindowOwner,
    origin: CursorFrameKey,
    source: Ui4CursorSource,
    kind: u32,
    label: &str,
    data: &[u8],
) -> Result<i32, i32> {
    let token = REGISTRY
        .lock()
        .begin(producer, origin, source, kind, label, data)?;
    super::input_broker::notify_slot4_visual_change();
    Ok(token)
}
pub(crate) fn cancel(producer: WindowOwner, token: i32) -> bool {
    let dropped_elsewhere = REGISTRY.lock().cancel(producer, token);
    super::input_broker::notify_slot4_visual_change();
    dropped_elsewhere
}
pub(crate) fn cursor_preview(source: Ui4CursorSource) -> Option<Arc<DragPreview>> {
    REGISTRY
        .lock()
        .drags
        .iter()
        .find(|drag| drag.source == source)
        .map(|drag| drag.preview.clone())
}
pub(crate) fn pointer_released(
    source: Ui4CursorSource,
    target: Option<(WindowOwner, CursorFrameKey, i32, i32)>,
) {
    REGISTRY.lock().finish(source, target);
    super::input_broker::notify_slot4_visual_change();
}
pub(crate) fn take(
    recipient: WindowOwner,
    target: CursorFrameKey,
    out: &mut [u8],
) -> Result<usize, i32> {
    REGISTRY.lock().take(recipient, target, out)
}
pub(crate) fn release_owner(owner: WindowOwner) {
    REGISTRY.lock().release_owner(owner);
    super::input_broker::notify_slot4_visual_change();
}
pub(crate) fn release_terminal(owner: WindowOwner, window: super::WindowId) {
    let mut registry = REGISTRY.lock();
    registry
        .drags
        .retain(|drag| drag.producer != owner || drag.origin.window != window);
    registry
        .drops
        .retain(|drop| drop.recipient != owner || drop.target.window != window);
    registry
        .completed
        .retain(|entry| entry.0 != owner || entry.2.window != window);
    drop(registry);
    super::input_broker::notify_slot4_visual_change();
}
pub(crate) fn frame_closed(frame: CursorFrameKey) {
    let mut registry = REGISTRY.lock();
    registry.drags.retain(|drag| drag.origin != frame);
    registry.drops.retain(|drop| drop.target != frame);
    registry.completed.retain(|entry| entry.2 != frame);
    drop(registry);
    super::input_broker::notify_slot4_visual_change();
}
pub(crate) fn cursor_retired(source: Ui4CursorSource) {
    pointer_released(source, None);
}

pub(crate) fn recipient(frame: CursorFrameKey) -> WindowOwner {
    if frame.owner == WindowOwner::SHELL3_SERVICE {
        if let Some(vm) = crate::shell3::tui::vm_for_window(frame.window) {
            return WindowOwner::Vm(vm);
        }
    }
    frame.owner
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(vm: u8, id: u32) -> CursorFrameKey {
        CursorFrameKey::new(WindowOwner::Vm(vm), super::super::WindowId::from_raw(id).unwrap())
    }
    fn source(id: u32) -> Ui4CursorSource {
        Ui4CursorSource {
            controller_id: id,
            slot_id: 1,
            ep_target: 1,
            hid_kind: 1,
        }
    }

    #[test]
    fn each_cursor_owns_one_drag_and_another_owner_cannot_cancel_it() {
        let mut r = Registry::new();
        let owner = WindowOwner::Vm(1);
        let a = r
            .begin(owner, frame(1, 1), source(1), 9, "hello", b"payload")
            .unwrap();
        assert_eq!(r.begin(WindowOwner::Vm(2), frame(2, 2), source(1), 9, "other", b""), Err(-5));
        let b = r
            .begin(WindowOwner::Vm(2), frame(2, 2), source(2), 9, "other", b"")
            .unwrap();
        assert!(!r.cancel(WindowOwner::Vm(2), a));
        assert_eq!(r.drags.len(), 2);
        r.cancel(owner, a);
        assert_eq!(r.drags.len(), 1);
        assert_eq!(r.drags[0].token, b);
    }
    #[test]
    fn copied_payload_is_delivered_once_to_the_exact_receiving_frame() {
        let mut r = Registry::new();
        let mut bytes = b"copied".to_vec();
        let token = r
            .begin(WindowOwner::Vm(1), frame(1, 1), source(1), 9, "hello", &bytes)
            .unwrap();
        bytes.fill(0);
        r.finish(source(1), Some((WindowOwner::Vm(2), frame(2, 2), 100, 20)));
        assert!(r.drags.is_empty());
        let mut out = [0u8; 32];
        assert_eq!(r.take(WindowOwner::Vm(3), frame(2, 2), &mut out), Ok(0));
        assert_eq!(r.take(WindowOwner::Vm(2), frame(2, 3), &mut out), Ok(0));
        assert_eq!(r.take(WindowOwner::Vm(2), frame(2, 2), &mut out[..16]), Err(-4));
        assert_eq!(r.take(WindowOwner::Vm(2), frame(2, 2), &mut out), Ok(22));
        assert_eq!(&out[16..22], b"copied");
        assert_eq!(u32::from_le_bytes(out[..4].try_into().unwrap()), 9);
        assert_eq!(i32::from_le_bytes(out[4..8].try_into().unwrap()), 100);
        assert_eq!(r.take(WindowOwner::Vm(2), frame(2, 2), &mut out), Ok(0));
        // Source learns the drop was external even after the receiver read it.
        assert!(r.cancel(WindowOwner::Vm(1), token));
    }
    #[test]
    fn local_drop_and_desktop_release_leave_local_app_behavior_intact() {
        let mut r = Registry::new();
        let token = r
            .begin(WindowOwner::Vm(1), frame(1, 1), source(1), 1, "hello", b"payload")
            .unwrap();
        r.finish(source(1), Some((WindowOwner::Vm(1), frame(1, 1), 10, 20)));
        assert!(r.drops.is_empty());
        assert!(!r.cancel(WindowOwner::Vm(1), token));
        let token = r
            .begin(WindowOwner::Vm(1), frame(1, 1), source(1), 1, "hello", b"payload")
            .unwrap();
        r.finish(source(1), None);
        assert!(r.drags.is_empty());
        assert!(!r.cancel(WindowOwner::Vm(1), token));
    }
    #[test]
    fn owner_shutdown_and_limits_do_not_leave_previews_or_unbounded_mailboxes() {
        let mut r = Registry::new();
        assert_eq!(
            r.begin(WindowOwner::Vm(1), frame(1, 1), source(1), 1, "bad\nlabel", b""),
            Err(-1)
        );
        assert_eq!(
            r.begin(
                WindowOwner::Vm(1),
                frame(1, 1),
                source(1),
                1,
                "label",
                &vec![0; MAX_PAYLOAD_BYTES + 1]
            ),
            Err(-1)
        );
        for _ in 0..100 {
            let token = r
                .begin(WindowOwner::Vm(1), frame(1, 1), source(1), 1, "label", b"x")
                .unwrap();
            r.finish(source(1), Some((WindowOwner::Vm(2), frame(2, 2), 0, 0)));
            assert!(r.cancel(WindowOwner::Vm(1), token));
        }
        assert_eq!(r.drops.len(), MAX_DROPS);
        for _ in 0..64 {
            r.begin(WindowOwner::Vm(1), frame(1, 1), source(1), 1, "label", b"x")
                .unwrap();
            r.finish(source(1), None);
        }
        assert_eq!(r.completed.len(), 64);
        assert_eq!(r.begin(WindowOwner::Vm(1), frame(1, 1), source(1), 1, "label", b"x"), Err(-5));
        r.release_owner(WindowOwner::Vm(1));
        r.release_owner(WindowOwner::Vm(2));
        assert!(r.drops.is_empty());
        r.begin(WindowOwner::Vm(1), frame(1, 1), source(1), 1, "label", b"x")
            .unwrap();
        r.release_owner(WindowOwner::Vm(1));
        assert!(r.drags.is_empty());
        assert!(r.completed.is_empty());
    }
    #[test]
    fn preview_is_cached_as_microfont_runs_with_bounded_width() {
        let p = preview("hello");
        assert!(!p.runs.is_empty());
        assert_eq!(p.width, 5 * microfont::FWIDTH as u32);
        assert!(
            p.runs
                .iter()
                .all(|&(x, y, len)| len > 0 && x + len <= p.width && y < p.height)
        );
        assert_eq!(preview(&"x".repeat(100)).width, 48 * microfont::FWIDTH as u32);
    }

    #[test]
    fn terminal_parking_and_frame_close_preserve_other_frames_of_the_same_vm() {
        let owner = WindowOwner::Vm(12);
        begin(owner, frame(12, 901), source(901), 1, "terminal", b"x").unwrap();
        begin(owner, frame(12, 902), source(902), 1, "another frame", b"x").unwrap();
        release_terminal(owner, frame(12, 901).window);
        assert!(cursor_preview(source(901)).is_none());
        assert!(cursor_preview(source(902)).is_some());
        frame_closed(frame(12, 902));
        assert!(cursor_preview(source(902)).is_none());
    }
}
