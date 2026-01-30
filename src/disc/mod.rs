pub mod block;
pub mod install;
pub mod layout;
pub mod nvme;

static PROBE_ONCE: spin::Once<()> = spin::Once::new();

pub fn probe_once() {
    PROBE_ONCE.call_once(|| {
        nvme::probe_once();
    });
}
