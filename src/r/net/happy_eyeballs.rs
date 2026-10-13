//! Poll both address-family attempts; a failure never wins the race.
extern crate alloc;
use alloc::{format, string::String};
use core::{future::Future, task::Poll};

pub(super) async fn first_success<T>(
    ipv6: impl Future<Output = Result<T, String>>,
    ipv4: impl Future<Output = Result<T, String>>,
) -> Result<(usize, T), String> {
    let mut ipv6 = core::pin::pin!(ipv6);
    let mut ipv4 = core::pin::pin!(ipv4);
    let mut errors: [Option<String>; 2] = [None, None];
    core::future::poll_fn(|cx| {
        if errors[0].is_none() {
            match ipv6.as_mut().poll(cx) {
                Poll::Ready(Ok(value)) => return Poll::Ready(Ok((0, value))),
                Poll::Ready(Err(err)) => errors[0] = Some(err),
                Poll::Pending => {}
            }
        }
        if errors[1].is_none() {
            match ipv4.as_mut().poll(cx) {
                Poll::Ready(Ok(value)) => return Poll::Ready(Ok((1, value))),
                Poll::Ready(Err(err)) => errors[1] = Some(err),
                Poll::Pending => {}
            }
        }
        if let [Some(v6), Some(v4)] = &errors {
            Poll::Ready(Err(format!("IPv6: {}; IPv4: {}", v6, v4)))
        } else {
            Poll::Pending
        }
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::{
        future::{pending, ready},
        task::{Context, Waker},
    };
    fn poll<T>(future: impl Future<Output = T>) -> Poll<T> {
        let mut future = core::pin::pin!(future);
        future
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
    }
    #[test]
    fn ipv6_wins_without_waiting_for_ipv4() {
        assert_eq!(poll(first_success(ready(Ok(6)), pending())), Poll::Ready(Ok((0, 6))));
    }
    #[test]
    fn broken_or_stalled_ipv6_does_not_block_ipv4() {
        assert_eq!(poll(first_success(pending(), ready(Ok(4)))), Poll::Ready(Ok((1, 4))));
        assert_eq!(
            poll(first_success(ready(Err("broken".into())), ready(Ok(4)))),
            Poll::Ready(Ok((1, 4)))
        );
    }
    #[test]
    fn waits_for_remaining_attempt_after_one_failure() {
        assert_eq!(
            poll(first_success::<u8>(ready(Err("broken".into())), pending())),
            Poll::Pending
        );
    }
    #[test]
    fn reports_both_failures() {
        assert_eq!(
            poll(first_success::<u8>(ready(Err("six".into())), ready(Err("four".into())))),
            Poll::Ready(Err("IPv6: six; IPv4: four".into()))
        );
    }
}
