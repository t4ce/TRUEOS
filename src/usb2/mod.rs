pub(crate) mod hid {
    pub use trueos_v::vinput::TrueosHidCursorEvent;

    #[inline]
    pub fn pop_cursor_event() -> Option<TrueosHidCursorEvent> {
        None
    }

    #[inline]
    pub fn read_cursor_events_since(
        read_seq: u64,
        _out: &mut [TrueosHidCursorEvent],
    ) -> (u64, u32, usize) {
        (read_seq, 0, 0)
    }

    #[inline]
    pub fn inject_virtual_cursor_event(
        _slot_id: u32,
        _x: f64,
        _y: f64,
        _buttons_down: u32,
        _wheel: i16,
        _flags: u32,
    ) {
    }
}

mod crabusb_service;

pub(crate) use self::crabusb_service::{
    audio_task as crabusb_audio_task, bsp_service as crabusb_bsp_service,
    event_pump_task as crabusb_event_pump_task, truekey_task as crabusb_truekey_task,
};
