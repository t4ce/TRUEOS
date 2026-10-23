//! Install the single compressed font asset before any framebuffer text is drawn.

#[cfg(target_os = "trueos")]
use alloc::boxed::Box;

#[cfg(target_os = "trueos")]
static INITIALIZED: spin::Once<Result<(), &'static str>> = spin::Once::new();

/// Called on the BSP immediately after the kernel heap is installed, before
/// application, display, console-surface, or secondary CPU initialization.
pub(crate) fn init() -> Result<(), &'static str> {
    #[cfg(target_os = "trueos")]
    {
        *INITIALIZED.call_once(|| {
            let bytes = super::tar::decode_named_single_lz4(
                ::microfont::EMBEDDED_ARCHIVE,
                ::microfont::UNCOMPRESSED_BUNDLE_BYTES,
                "microfont.bin",
            )
            .map_err(|_| "cannot decode embedded MicroFont tar/LZ4 archive")?;
            if bytes.len() != ::microfont::UNCOMPRESSED_BUNDLE_BYTES {
                return Err("embedded MicroFont bundle has the wrong uncompressed size");
            }
            let bytes = Box::leak(bytes.into_boxed_slice());
            ::microfont::install_bundle(bytes)?;
            crate::log_info!(target: "boot";
                "microfont: initialized source=tar.lz4 archive_bytes={} bundle_bytes={} storage=single-archive lifetime=kernel\n",
                ::microfont::EMBEDDED_ARCHIVE.len(), bytes.len());
            Ok(())
        })
    }
    #[cfg(not(target_os = "trueos"))]
    {
        // Portable targets already embed the uncompressed, dependency-free file.
        Ok(())
    }
}
