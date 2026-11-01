//! Initial virtual scanout sizing, independent of the GTK window's size.

pub(super) fn initial_extent(host: (u32, u32), loader: Option<(u32, u32)>) -> ((u32, u32), &'static str) {
    // A small host window is a display hint, not a useful UI4 desktop size.
    // Retain larger modes and preserve their aspect ratio without stretching.
    let usable = |(w, h): (u32, u32)| w >= 1920 && h >= 1080 && w <= 8192 && h <= 8192;
    if usable(host) { return (host, "host"); }
    if let Some(mode) = loader.filter(|&mode| usable(mode)) {
        return (mode, "loader");
    }
    ((1920, 1080), "fullhd-fallback")
}

#[cfg(test)]
mod tests {
    use super::initial_extent;

    #[test]
    fn small_host_window_does_not_shrink_loader_desktop() {
        assert_eq!(initial_extent((1280, 720), Some((2560, 1440))), ((2560, 1440), "loader"));
    }

    #[test]
    fn fullhd_fallback_handles_missing_or_small_loader_mode() {
        for loader in [None, Some((800, 600)), Some((0, 0)), Some((u32::MAX, u32::MAX))] {
            assert_eq!(initial_extent((1024, 768), loader), ((1920, 1080), "fullhd-fallback"));
        }
    }

    #[test]
    fn larger_host_mode_keeps_its_aspect_ratio() {
        assert_eq!(initial_extent((1920, 1200), Some((2560, 1440))), ((1920, 1200), "host"));
    }
}
