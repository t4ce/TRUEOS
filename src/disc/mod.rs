pub mod block;
pub mod install;
pub mod layout;

static PROBE_ONCE: spin::Once<()> = spin::Once::new();

pub fn probe_once() {
    PROBE_ONCE.call_once(|| {
        crate::pci::nvme::probe_once();
    });
}
