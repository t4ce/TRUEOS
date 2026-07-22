use core::future::{Future, ready};
use core::pin::pin;
use core::task::{Context, Poll, Waker};

use lumen::async_module::{AsyncModule, forward};
use lumen::backend::{AsyncBackend, LumenBackend, default_backend, execute};

struct AddOne;

impl AsyncBackend<u32> for AddOne {
    type Output = u32;
    type Error = ();

    fn execute(&self, operation: u32) -> impl Future<Output = Result<Self::Output, Self::Error>> {
        ready(Ok(operation + 1))
    }
}

struct AddOneModule;

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
