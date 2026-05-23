use crate::signal::unix::{OsExtraData, OsStorage};
use crate::sync::watch;

use core::ops;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

pub(crate) type EventId = usize;

/// State for a specific event, whether a notification is pending delivery,
/// and what listeners are registered.
#[derive(Debug)]
pub(crate) struct EventInfo {
    pending: AtomicBool,
    tx: watch::Sender<()>,
}

impl Default for EventInfo {
    fn default() -> Self {
        let (tx, _rx) = watch::channel(());

        Self {
            pending: AtomicBool::new(false),
            tx,
        }
    }
}

/// An interface for retrieving the `EventInfo` for a particular `eventId`.
pub(crate) trait Storage {
    /// Gets the `EventInfo` for `id` if it exists.
    fn event_info(&self, id: EventId) -> Option<&EventInfo>;

    /// Invokes `f` once for each defined `EventInfo` in this storage.
    fn for_each<'a, F>(&'a self, f: F)
    where
        F: FnMut(&'a EventInfo);
}

impl Storage for Vec<EventInfo> {
    fn event_info(&self, id: EventId) -> Option<&EventInfo> {
        self.get(id)
    }

    fn for_each<'a, F>(&'a self, f: F)
    where
        F: FnMut(&'a EventInfo),
    {
        self.iter().for_each(f);
    }
}

/// Manages and distributes event notifications to any registered listeners.
///
/// Generic over the underlying storage to allow for domain specific
/// optimizations (e.g. `eventIds` may or may not be contiguous).
#[derive(Debug)]
pub(crate) struct Registry<S> {
    storage: S,
}

impl<S> Registry<S> {
    fn new(storage: S) -> Self {
        Self { storage }
    }
}

impl<S: Storage> Registry<S> {
    /// Registers a new listener for `event_id`.
    fn register_listener(&self, event_id: EventId) -> watch::Receiver<()> {
        self.storage
            .event_info(event_id)
            .unwrap_or_else(|| panic!("invalid event_id: {event_id}"))
            .tx
            .subscribe()
    }

    /// Marks `event_id` as having been delivered, without broadcasting it to
    /// any listeners.
    fn record_event(&self, event_id: EventId) {
        if let Some(event_info) = self.storage.event_info(event_id) {
            event_info.pending.store(true, Ordering::SeqCst);
        }
    }

    /// Broadcasts all previously recorded events to their respective listeners.
    ///
    /// Returns `true` if an event was delivered to at least one listener.
    fn broadcast(&self) -> bool {
        let mut did_notify = false;
        self.storage.for_each(|event_info| {
            // Any signal of this kind arrived since we checked last?
            if !event_info.pending.swap(false, Ordering::SeqCst) {
                return;
            }

            // Ignore errors if there are no listeners
            if event_info.tx.send(()).is_ok() {
                did_notify = true;
            }
        });

        did_notify
    }
}

pub(crate) struct Globals {
    extra: OsExtraData,
    registry: Registry<OsStorage>,
}

impl ops::Deref for Globals {
    type Target = OsExtraData;

    fn deref(&self) -> &Self::Target {
        &self.extra
    }
}

impl Globals {
    /// Registers a new listener for `event_id`.
    pub(crate) fn register_listener(&self, event_id: EventId) -> watch::Receiver<()> {
        self.registry.register_listener(event_id)
    }

    /// Marks `event_id` as having been delivered, without broadcasting it to
    /// any listeners.
    pub(crate) fn record_event(&self, event_id: EventId) {
        self.registry.record_event(event_id);
    }

    /// Broadcasts all previously recorded events to their respective listeners.
    ///
    /// Returns `true` if an event was delivered to at least one listener.
    pub(crate) fn broadcast(&self) -> bool {
        self.registry.broadcast()
    }

    #[cfg(unix)]
    pub(crate) fn storage(&self) -> &OsStorage {
        &self.registry.storage
    }
}

fn globals_init() -> Globals
where
    OsExtraData: 'static + Send + Sync + Default,
    OsStorage: 'static + Send + Sync + Default,
{
    Globals {
        extra: OsExtraData::default(),
        registry: Registry::new(OsStorage::default()),
    }
}

pub(crate) fn globals() -> &'static Globals
where
    OsExtraData: 'static + Send + Sync + Default,
    OsStorage: 'static + Send + Sync + Default,
{
    static GLOBALS: OnceLock<Globals> = OnceLock::new();

    GLOBALS.get_or_init(globals_init)
}
