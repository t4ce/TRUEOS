//! Lumen ownership boundary for fixed TRUEGA functions.
//!
//! Lumen supplies the typed asynchronous dispatch contract. TRUEOS owns the
//! concrete device backend: sealed model I/O, inline BAR work packages, the
//! single fpga-offload worker, MSI wakeup, and Rust completion callbacks.
//! There is deliberately no synchronous CPU callback or secondary scheduler.

extern crate alloc;

use alloc::vec::Vec;
use core::future::Future;

use ::lumen::async_module::AsyncModule;
use ::lumen::backend::AsyncBackend;

use crate::r::lfm25_ffn;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct TruegaBackend;

/// Fixed-shape, exact Q30 tensor returned by a TRUEGA function.
pub(crate) struct Q30Tensor<const ELEMENTS: usize> {
    values: Vec<i64>,
}

impl<const ELEMENTS: usize> Q30Tensor<ELEMENTS> {
    fn new(values: Vec<i64>) -> Result<Self, lfm25_ffn::Error> {
        if values.len() != ELEMENTS {
            return Err(lfm25_ffn::Error::Tensor);
        }
        Ok(Self { values })
    }

    pub(crate) const fn shape(&self) -> [usize; 1] {
        [ELEMENTS]
    }

    pub(crate) fn as_slice(&self) -> &[i64] {
        &self.values
    }
}

/// Input identifying the pinned activation used by the first sealed module.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct SealedLfm25Ffn0Input;

/// A real Lumen forward result: hardware tensor plus completion evidence.
pub(crate) struct Lfm25Ffn0Output {
    tensor: Q30Tensor<{ lfm25_ffn::FFN_OUTPUT_ELEMENTS }>,
    report: lfm25_ffn::Report,
}

impl Lfm25Ffn0Output {
    pub(crate) fn tensor(&self) -> &Q30Tensor<{ lfm25_ffn::FFN_OUTPUT_ELEMENTS }> {
        &self.tensor
    }

    pub(crate) const fn report(&self) -> &lfm25_ffn::Report {
        &self.report
    }
}

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
    type Output = lfm25_ffn::Execution;
    type Error = lfm25_ffn::Error;

    fn execute(
        &self,
        operation: SealedLfm25Ffn0<Progress>,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>> {
        lfm25_ffn::run_with_output(operation.progress)
    }
}

/// Lumen module representing the ahead-of-time layer-0 LFM2.5 FFN function.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Lfm25Ffn0 {
    backend: TruegaBackend,
}

impl AsyncModule for Lfm25Ffn0 {
    type Input = SealedLfm25Ffn0Input;
    type Output = Lfm25Ffn0Output;
    type Error = lfm25_ffn::Error;

    fn forward(
        &self,
        _input: Self::Input,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>> {
        let backend = self.backend;
        async move {
            let execution =
                ::lumen::backend::execute(&backend, SealedLfm25Ffn0 { progress: |_| {} }).await?;
            Ok(Lfm25Ffn0Output {
                tensor: Q30Tensor::new(execution.output_q30)?,
                report: execution.report,
            })
        }
    }
}

pub(crate) async fn run_sealed_lfm25_ffn0(
    progress: impl FnMut(lfm25_ffn::Progress),
) -> Result<lfm25_ffn::Report, lfm25_ffn::Error> {
    Ok(::lumen::backend::execute(&TruegaBackend, SealedLfm25Ffn0 { progress })
        .await?
        .report)
}

pub(crate) async fn hello() -> Result<Lfm25Ffn0Output, lfm25_ffn::Error> {
    ::lumen::async_module::forward(&Lfm25Ffn0::default(), SealedLfm25Ffn0Input).await
}

pub(crate) fn log_backend_once() {
    crate::log_info!(
        target: "boot";
        "lumen: backend={} execution=async-function transport=inline-bar completion=msi-worker-callback model_io=trueosfs\n",
        ::lumen::backend::default_backend_name(),
    );
}
