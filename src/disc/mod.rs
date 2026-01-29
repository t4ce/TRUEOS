pub mod block;
pub mod detect;
pub mod fat32;
pub mod files;
pub mod install;
pub mod layout;
pub mod nvme;
pub mod partition;
pub mod trueosfs;

static PROBE_ONCE: spin::Once<()> = spin::Once::new();

pub fn probe_once() {
    PROBE_ONCE.call_once(|| {
        nvme::probe_once();
    });
}
