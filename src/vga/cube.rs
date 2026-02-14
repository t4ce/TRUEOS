pub fn tick() {
    let _ = crate::gfx::with_device(|dev, pres| {
        crate::gfx::demo::tick(dev, pres);
    });
}
