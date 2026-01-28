pub mod block;
pub mod detect;
pub mod files;
pub mod layout;
pub mod nvme;
pub mod partition;

static PROBE_ONCE: spin::Once<()> = spin::Once::new();

pub fn probe_once() {
    PROBE_ONCE.call_once(|| {
        nvme::probe_once();
    });
}
