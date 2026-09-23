//! Whole-device admission gate. Transactions include the gaps between block I/O.
use super::block::{self, DeviceHandle, DiscId};
use alloc::vec::Vec;

#[derive(Default)]
struct State {
    active: usize,
    exclusive: bool,
}
static ACCESS: spin::Mutex<Vec<(DiscId, State)>> = spin::Mutex::new(Vec::new());

fn root(mut disk: DeviceHandle) -> DiscId {
    while let Some(parent) = disk.parent().and_then(block::device_handle) {
        disk = parent;
    }
    disk.id()
}

pub struct Activity {
    id: DiscId,
}
impl Activity {
    pub fn begin(disk: DeviceHandle) -> block::Result<Self> {
        let id = root(disk);
        let mut states = ACCESS.lock();
        let index = match states.iter().position(|(key, _)| *key == id) {
            Some(index) => index,
            None => {
                states.push((id, State::default()));
                states.len() - 1
            }
        };
        let state = &mut states[index].1;
        if state.exclusive {
            return Err(block::Error::NotReady);
        }
        state.active += 1;
        Ok(Self { id })
    }
}
impl Drop for Activity {
    fn drop(&mut self) {
        if let Some((_, state)) = ACCESS.lock().iter_mut().find(|(id, _)| *id == self.id) {
            state.active -= 1;
        }
    }
}

pub(super) struct Exclusive {
    id: DiscId,
}
impl Exclusive {
    pub(super) fn acquire(disk: DeviceHandle) -> block::Result<Self> {
        if disk.parent().is_some() {
            return Err(block::Error::InvalidParam);
        }
        let id = disk.id();
        let mut states = ACCESS.lock();
        let index = match states.iter().position(|(key, _)| *key == id) {
            Some(index) => index,
            None => {
                states.push((id, State::default()));
                states.len() - 1
            }
        };
        let state = &mut states[index].1;
        if state.exclusive || state.active != 0 {
            return Err(block::Error::NotReady);
        }
        state.exclusive = true;
        Ok(Self { id })
    }
}
impl Drop for Exclusive {
    fn drop(&mut self) {
        if let Some((_, state)) = ACCESS.lock().iter_mut().find(|(id, _)| *id == self.id) {
            state.exclusive = false;
        }
    }
}
