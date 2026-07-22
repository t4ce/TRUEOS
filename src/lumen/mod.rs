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
use ::lumen::backend::{AotInvocation, AsyncBackend, TruegaAotBackend, TruegaCustomOp};
use sha2::{Digest, Sha256};
use trueos_fpga_abi::{AotFirmwareCapability, AotTransportKind};

use crate::r::{fpga_offload, lfm25_ffn};

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct TruegaBackend;

impl TruegaAotBackend for TruegaBackend {
    type Error = lfm25_ffn::Error;

    fn execute_aot<Op>(
        &self,
        operation: AotInvocation<Op>,
    ) -> impl Future<Output = Result<Op::Output, Self::Error>>
    where
        Op: TruegaCustomOp,
    {
        async move {
            match Op::DESCRIPTOR.transport {
                AotTransportKind::InlineBar0WorkPackage => {
                    let (function, symbol_hash) = match Op::DESCRIPTOR.firmware {
                        AotFirmwareCapability::FunctionSlot {
                            function,
                            symbol_hash,
                        } => (function, symbol_hash),
                        _ => return Err(lfm25_ffn::Error::Tensor),
                    };
                    let generated = trueos_fpga_abi::builtins::FUNCTIONS
                        .get(function.raw() as usize)
                        .ok_or(lfm25_ffn::Error::Tensor)?;
                    if generated.symbol_hash != symbol_hash {
                        return Err(lfm25_ffn::Error::Tensor);
                    }

                    let mut input = [0u8; trueos_fpga_abi::INLINE_INPUT_BYTES];
                    let input_len = Op::encode(&operation.input, &mut input)
                        .map_err(|_| lfm25_ffn::Error::Tensor)?;
                    let output_capacity = Op::DESCRIPTOR
                        .outputs
                        .iter()
                        .try_fold(0usize, |total, output| {
                            total.checked_add(output.encoded_bytes as usize)
                        })
                        .ok_or(lfm25_ffn::Error::Arithmetic)?;
                    let completion =
                        fpga_offload::call(function, &input[..input_len], output_capacity).await?;
                    Op::decode(completion.output()).map_err(|_| lfm25_ffn::Error::Tensor)
                }
                AotTransportKind::FixedBar2RowStream => {
                    // BAR2 currently exposes one generated composite operation. Matching
                    // its complete AOT seal keeps this a compile-time dispatch, not a
                    // runtime name/graph registry.
                    let expected = trueos_fpga_abi::builtins::lfm25_ffn::AOT_DESCRIPTOR;
                    if Op::DESCRIPTOR.contract_sha256 != expected.contract_sha256
                        || Op::DESCRIPTOR.firmware != expected.firmware
                    {
                        return Err(lfm25_ffn::Error::Tensor);
                    }

                    let encoded_bytes = Op::DESCRIPTOR
                        .inputs
                        .iter()
                        .try_fold(0usize, |total, input| {
                            total.checked_add(input.encoded_bytes as usize)
                        })
                        .ok_or(lfm25_ffn::Error::Arithmetic)?;
                    let mut encoded = try_zeroed_bytes(encoded_bytes)?;
                    let used = Op::encode(&operation.input, &mut encoded)
                        .map_err(|_| lfm25_ffn::Error::Tensor)?;
                    if used != trueos_fpga_abi::builtins::lfm25_ffn::ENCODED_INPUT_BYTES {
                        return Err(lfm25_ffn::Error::Tensor);
                    }
                    let layer = u16::from_le_bytes(
                        encoded[..2]
                            .try_into()
                            .map_err(|_| lfm25_ffn::Error::Tensor)?,
                    );
                    let mut blocks = Vec::new();
                    blocks
                        .try_reserve_exact(
                            lfm25_ffn::FFN_INPUT_ELEMENTS / lfm25_ffn::Q8_0_BLOCK_VALUES,
                        )
                        .map_err(|_| lfm25_ffn::Error::BufferUnavailable)?;
                    for block in encoded[2..used].chunks_exact(lfm25_ffn::Q8_0_BLOCK_BYTES) {
                        blocks.push(block.try_into().map_err(|_| lfm25_ffn::Error::Tensor)?);
                    }

                    let execution = lfm25_ffn::execute_layer(layer as u8, blocks, |_| {}).await?;
                    let mut output =
                        try_zeroed_bytes(trueos_fpga_abi::builtins::lfm25_ffn::OUTPUT_BYTES)?;
                    for (destination, value) in output
                        .chunks_exact_mut(core::mem::size_of::<i64>())
                        .zip(execution.output_q30)
                    {
                        destination.copy_from_slice(&value.to_le_bytes());
                    }
                    Op::decode(&output).map_err(|_| lfm25_ffn::Error::Tensor)
                }
            }
        }
    }
}

fn try_zeroed_bytes(bytes: usize) -> Result<Vec<u8>, lfm25_ffn::Error> {
    let mut output = Vec::new();
    output
        .try_reserve_exact(bytes)
        .map_err(|_| lfm25_ffn::Error::BufferUnavailable)?;
    output.resize(bytes, 0);
    Ok(output)
}

/// Fixed-shape native GGML Q8_0 tensor accepted by TRUEGA operations.
///
/// Each block owns one F16 scale and 32 signed quantized values. Construction
/// validates the shape once, so the backend receives exactly 32 blocks for an
/// LFM2.5 hidden activation and performs no runtime format discovery.
#[allow(non_camel_case_types)]
#[derive(Clone, Debug)]
pub(crate) struct Q8_0<const ELEMENTS: usize> {
    blocks: Vec<lfm25_ffn::Q8_0Block>,
}

impl<const ELEMENTS: usize> Q8_0<ELEMENTS> {
    pub(crate) fn from_blocks(blocks: Vec<lfm25_ffn::Q8_0Block>) -> Result<Self, lfm25_ffn::Error> {
        if ELEMENTS % lfm25_ffn::Q8_0_BLOCK_VALUES != 0
            || blocks.len() != ELEMENTS / lfm25_ffn::Q8_0_BLOCK_VALUES
        {
            return Err(lfm25_ffn::Error::Tensor);
        }
        Ok(Self { blocks })
    }

    pub(crate) fn from_bytes(bytes: &[u8]) -> Result<Self, lfm25_ffn::Error> {
        if bytes.len() != ELEMENTS / lfm25_ffn::Q8_0_BLOCK_VALUES * lfm25_ffn::Q8_0_BLOCK_BYTES {
            return Err(lfm25_ffn::Error::Tensor);
        }
        let mut blocks = Vec::new();
        blocks
            .try_reserve_exact(ELEMENTS / lfm25_ffn::Q8_0_BLOCK_VALUES)
            .map_err(|_| lfm25_ffn::Error::BufferUnavailable)?;
        for block in bytes.chunks_exact(lfm25_ffn::Q8_0_BLOCK_BYTES) {
            blocks.push(block.try_into().map_err(|_| lfm25_ffn::Error::Tensor)?);
        }
        Self::from_blocks(blocks)
    }

    pub(crate) const fn shape(&self) -> [usize; 1] {
        [ELEMENTS]
    }

    pub(crate) fn as_blocks(&self) -> &[lfm25_ffn::Q8_0Block] {
        &self.blocks
    }

    fn into_blocks(self) -> Vec<lfm25_ffn::Q8_0Block> {
        self.blocks
    }
}

impl Q8_0<{ lfm25_ffn::FFN_INPUT_ELEMENTS }> {
    /// Produce the sealed vector used by `lumen hello` and the layer-0 proof.
    /// The returned value is an ordinary runtime tensor, not a special token.
    pub(crate) fn sealed_vector0() -> Result<Self, lfm25_ffn::Error> {
        Self::from_blocks(lfm25_ffn::sealed_layer0_activation()?)
    }
}

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

/// Real Lumen forward result: an FPGA-computed tensor and MSI evidence.
pub(crate) struct Lfm25FfnOutput {
    tensor: Q30Tensor<{ lfm25_ffn::FFN_OUTPUT_ELEMENTS }>,
    report: lfm25_ffn::ForwardReport,
}

impl Lfm25FfnOutput {
    pub(crate) fn tensor(&self) -> &Q30Tensor<{ lfm25_ffn::FFN_OUTPUT_ELEMENTS }> {
        &self.tensor
    }

    pub(crate) const fn report(&self) -> &lfm25_ffn::ForwardReport {
        &self.report
    }
}

/// Diagnostic operation retaining the exhaustive sealed layer-0 checks.
struct VerifiedLfm25Ffn0<Progress> {
    progress: Progress,
}

impl<Progress> AsyncBackend<VerifiedLfm25Ffn0<Progress>> for TruegaBackend
where
    Progress: FnMut(lfm25_ffn::Progress),
{
    type Output = lfm25_ffn::Execution;
    type Error = lfm25_ffn::Error;

    fn execute(
        &self,
        operation: VerifiedLfm25Ffn0<Progress>,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>> {
        lfm25_ffn::run_with_output(operation.progress)
    }
}

/// Lumen module representing one ahead-of-time LFM2.5 FFN function.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Lfm25Ffn {
    backend: TruegaBackend,
    layer: u8,
}

impl Lfm25Ffn {
    pub(crate) fn new(layer: u8) -> Result<Self, lfm25_ffn::Error> {
        if usize::from(layer) >= lfm25_ffn::FFN_LAYER_COUNT {
            return Err(lfm25_ffn::Error::Layer);
        }
        Ok(Self {
            backend: TruegaBackend,
            layer,
        })
    }

    pub(crate) const fn layer(&self) -> u8 {
        self.layer
    }
}

impl Default for Lfm25Ffn {
    fn default() -> Self {
        Self {
            backend: TruegaBackend,
            layer: 0,
        }
    }
}

impl AsyncModule for Lfm25Ffn {
    type Input = Q8_0<{ lfm25_ffn::FFN_INPUT_ELEMENTS }>;
    type Output = Lfm25FfnOutput;
    type Error = lfm25_ffn::Error;

    fn forward(
        &self,
        input: Self::Input,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>> {
        let backend = self.backend;
        let layer = self.layer;
        async move {
            let mut activation = [0u8; trueos_fpga_abi::builtins::lfm25_ffn::ACTIVATION_BYTES];
            for (destination, block) in activation
                .chunks_exact_mut(lfm25_ffn::Q8_0_BLOCK_BYTES)
                .zip(input.into_blocks())
            {
                destination.copy_from_slice(&block);
            }

            let before = fpga_offload::stats();
            let completion_before = fpga_offload::lfm25_stream_completion_count()?;
            type FfnOp = trueos_fpga_abi::builtins::lfm25_ffn::AotOp;
            let output = ::lumen::backend::execute(
                &backend,
                AotInvocation::<FfnOp>::new(trueos_fpga_abi::builtins::lfm25_ffn::Input {
                    layer: u16::from(layer),
                    activation,
                }),
            )
            .await?;
            let completion_after = fpga_offload::lfm25_stream_completion_count()?;
            let after = fpga_offload::stats();
            let output_q30 = Vec::from(output);
            let mut hasher = Sha256::new();
            for value in &output_q30 {
                hasher.update(value.to_le_bytes());
            }
            let report = lfm25_ffn::ForwardReport {
                layer,
                output_sha256: hasher.finalize().into(),
                fpga_calls: completion_after.wrapping_sub(completion_before) as u64,
                interrupt_delta: after.interrupts.saturating_sub(before.interrupts),
                timeout_recovery_delta: after
                    .timeout_recoveries
                    .saturating_sub(before.timeout_recoveries),
                streamed: true,
            };
            Ok(Lfm25FfnOutput {
                tensor: Q30Tensor::new(output_q30)?,
                report,
            })
        }
    }
}

pub(crate) async fn forward_layer(
    layer: u8,
    input: Q8_0<{ lfm25_ffn::FFN_INPUT_ELEMENTS }>,
) -> Result<Lfm25FfnOutput, lfm25_ffn::Error> {
    let module = Lfm25Ffn::new(layer)?;
    ::lumen::async_module::forward(&module, input).await
}

pub(crate) async fn run_sealed_lfm25_ffn0(
    progress: impl FnMut(lfm25_ffn::Progress),
) -> Result<lfm25_ffn::Report, lfm25_ffn::Error> {
    Ok(::lumen::backend::execute(&TruegaBackend, VerifiedLfm25Ffn0 { progress })
        .await?
        .report)
}

pub(crate) async fn hello() -> Result<Lfm25FfnOutput, lfm25_ffn::Error> {
    let output = forward_layer(0, Q8_0::sealed_vector0()?).await?;
    lfm25_ffn::verify_sealed_layer0_forward(output.report())?;
    Ok(output)
}

pub(crate) fn log_backend_once() {
    crate::log_info!(
        target: "boot";
        "lumen: backend={} execution=generated-aot-custom-op transport=bar0+bar2 completion=msi-worker-callback model_io=trueosfs\n",
        ::lumen::backend::default_backend_name(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_q8_tensor_enforces_exact_hidden_shape() {
        let bytes = [0u8; 32 * lfm25_ffn::Q8_0_BLOCK_BYTES];
        let input = Q8_0::<{ lfm25_ffn::FFN_INPUT_ELEMENTS }>::from_bytes(&bytes).unwrap();
        assert_eq!(input.shape(), [1_024]);
        assert_eq!(input.as_blocks().len(), 32);
        assert!(Q8_0::<{ lfm25_ffn::FFN_INPUT_ELEMENTS }>::from_bytes(&bytes[..34]).is_err());
    }

    #[test]
    fn all_generated_ffn_layers_are_addressable() {
        for layer in 0..lfm25_ffn::FFN_LAYER_COUNT as u8 {
            assert_eq!(Lfm25Ffn::new(layer).unwrap().layer(), layer);
        }
        assert!(Lfm25Ffn::new(lfm25_ffn::FFN_LAYER_COUNT as u8).is_err());
    }
}
