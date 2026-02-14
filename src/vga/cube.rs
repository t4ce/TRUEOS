use core::sync::atomic::{AtomicBool, Ordering};

static RENDERED_ONCE: AtomicBool = AtomicBool::new(false);

pub fn tick() {
    if RENDERED_ONCE.swap(true, Ordering::AcqRel) {
        return;
    }
    let _ = crate::gfx::with_context(|ctx| {
        crate::gfx::demo::tick(ctx);
    });

    // Draw the boot logo after the gfx proof render so it remains visible.
    crate::efi::acpi::bgrt::log_once();
}
