//! Lumen ownership boundary for fixed TRUEGA functions.
//!
//! Lumen supplies the typed asynchronous dispatch contract. TRUEOS owns the
//! concrete device backend: sealed model I/O, inline BAR work packages, the
//! single fpga-offload worker, MSI wakeup, and Rust completion callbacks.
//! There is deliberately no synchronous CPU callback or secondary scheduler.

use core::future::Future;

use ::lumen::backend::AsyncBackend;

use crate::r::lfm25_ffn;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct TruegaBackend;

/// The first Lumen-owned TRUEGA operation is the sealed layer-0 FFN proof.
///
/// Keeping progress in the operation makes the backend useful to shell and
/// service callers without giving Lumen ownership of logging or kernel I/O.
pub(crate) struct SealedLfm25Ffn0<Progress> {
    progress: Progress,
}

impl<Progress> AsyncBackend<SealedLfm25Ffn0<Progress>> for TruegaBackend
where
    Progress: FnMut(lfm25_ffn::Progress),
{
    type Output = lfm25_ffn::Report;
    type Error = lfm25_ffn::Error;

    fn execute(
        &self,
        operation: SealedLfm25Ffn0<Progress>,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>> {
        lfm25_ffn::run(operation.progress)
    }
}

pub(crate) async fn run_sealed_lfm25_ffn0(
    progress: impl FnMut(lfm25_ffn::Progress),
) -> Result<lfm25_ffn::Report, lfm25_ffn::Error> {
    ::lumen::backend::execute(&TruegaBackend, SealedLfm25Ffn0 { progress }).await
}

pub(crate) fn log_backend_once() {
    crate::log_info!(
        target: "boot";
        "lumen: backend={} execution=async-function transport=inline-bar completion=msi-worker-callback model_io=trueosfs\n",
        ::lumen::backend::default_backend_name(),
    );
}
