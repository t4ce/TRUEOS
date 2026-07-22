use core::future::{Future, ready};
use core::pin::pin;
use core::task::{Context, Poll, Waker};

use lumen::async_module::{AsyncModule, forward};
use lumen::backend::{
    AotCodecError, AotInvocation, AotLane, AotTransportKind, AsyncBackend, LumenBackend,
    TruegaAotBackend, TruegaCustomOp, default_backend, execute,
};
use trueos_fpga_abi::builtins;

struct AddOne;

impl AsyncBackend<u32> for AddOne {
    type Output = u32;
    type Error = ();

    fn execute(&self, operation: u32) -> impl Future<Output = Result<Self::Output, Self::Error>> {
        ready(Ok(operation + 1))
    }
}

struct AddOneModule;

struct CodecLoopback;

impl TruegaAotBackend for CodecLoopback {
    type Error = AotCodecError;

    fn execute_aot<Op>(
        &self,
        operation: AotInvocation<Op>,
    ) -> impl Future<Output = Result<Op::Output, Self::Error>>
    where
        Op: TruegaCustomOp,
    {
        let mut bytes = [0u8; 8192];
        let result = Op::encode(&operation.input, &mut bytes)
            .and_then(|length| Op::decode(&bytes[..length]));
        ready(result)
    }
}

impl AsyncModule for AddOneModule {
    type Input = u32;
    type Output = u32;
    type Error = ();

    fn forward(
        &self,
        input: Self::Input,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>> {
        execute(&AddOne, input)
    }
}

#[test]
fn truega_feature_selects_async_truega_backend() {
    assert_eq!(default_backend(), LumenBackend::Truega);
    assert!(default_backend().is_truega());

    let mut future = pin!(execute(&AddOne, 41));
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    assert_eq!(future.as_mut().poll(&mut context), Poll::Ready(Ok(42)));
}

#[test]
fn async_module_forwards_through_the_typed_backend() {
    let mut future = pin!(forward(&AddOneModule, 41));
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    assert_eq!(future.as_mut().poll(&mut context), Poll::Ready(Ok(42)));
}

#[test]
fn generated_aot_operation_uses_blanket_async_dispatch() {
    type Add = builtins::add_u32::AotOp;

    let invocation = AotInvocation::<Add>::new([41, 1]);
    assert_eq!(invocation.descriptor().transport, AotTransportKind::InlineBar0WorkPackage);
    assert_eq!(invocation.descriptor().lane, AotLane::Bar0WorkPackage);

    // The loopback intentionally decodes the first encoded u32. Its purpose is to prove
    // that the blanket backend preserves the generated operation's concrete `u32` output.
    let mut future = pin!(execute(&CodecLoopback, invocation));
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    assert_eq!(future.as_mut().poll(&mut context), Poll::Ready(Ok(41)));
}

#[test]
fn generated_lfm25_ffn_contract_is_all_layers_and_fixed_shape() {
    let descriptor = builtins::lfm25_ffn::AOT_DESCRIPTOR;
    assert_eq!(descriptor.name, "lfm25.ffn");
    assert_eq!(descriptor.transport, AotTransportKind::FixedBar2RowStream);
    assert_eq!(descriptor.inputs[0].name, "layer");
    assert_eq!(descriptor.inputs[1].shape.dimensions[0], 1024);
    assert_eq!(descriptor.outputs[0].shape.dimensions[0], 1024);
    assert_eq!(builtins::lfm25_ffn::MODEL_LAYERS, 16);
    assert_ne!(descriptor.contract_sha256, [0; 32]);
}
