//! Host path subscriptions. Notifications follow successful filesystem commits.
//! Callbacks only signal consumers; they must not perform filesystem I/O.
use alloc::{string::String, vec::Vec};
use crate::disc::block::DiscId;
type Callback = fn(DiscId, &str);
static WATCHERS: spin::Mutex<Vec<(String, Callback)>> = spin::Mutex::new(Vec::new());

pub(crate) fn subscribe(path: &str, callback: Callback) {
    let mut watchers = WATCHERS.lock();
    if !watchers.iter().any(|(name, cb)| name == path && core::ptr::fn_addr_eq(*cb, callback)) {
        watchers.push((path.into(), callback));
    }
}
pub(crate) fn changed(disk: DiscId, path: &str) {
    let callbacks: Vec<_> = WATCHERS.lock().iter()
        .filter(|(name, _)| name == path.trim_start_matches('/')).map(|(_, cb)| *cb).collect();
    for callback in callbacks { callback(disk, path); }
}
