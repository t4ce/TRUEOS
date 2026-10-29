#[path = "@TRUEOS_ROOT@/src/shell3/first_presentation.rs"]
mod first_presentation;

fn run_wait(future: impl core::future::Future<Output = bool>) -> bool {
    use core::task::{Context, Poll, Waker};
    let mut future = std::pin::pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(result) => return result,
            Poll::Pending => {}
        }
    }
}

#[test]
fn first_presentation_wait_services_new_capture_and_termination_before_paint() {
    let mut test = SessionTest::new();
    let mut handles = Vec::new();
    let mut checks = 0;
    let mut sleeps = 0;
    let read_exit = |handle| {
        let mut bytes = [0; 4096];
        assert!(sessions::read(1, handle, &mut bytes) >= 8);
        assert_eq!(u32::from_le_bytes(bytes[..4].try_into().unwrap()), 1);
        i32::from_le_bytes(bytes[4..8].try_into().unwrap())
    };
    let presented = run_wait(first_presentation::wait(
        || -> Result<bool, ()> {
            match checks {
                0 => {
                    // Both requests arrive after the worker has begun waiting.
                    for (script, stdin_open) in [(&b"ram"[..], false), (&b""[..], true)] {
                        let handle = sessions::open(1, script, 0, 0, stdin_open);
                        assert!(handle > 0);
                        handles.push(handle as u64);
                    }
                }
                1 => {
                    assert_eq!(read_exit(handles[0]), 0);
                    assert_eq!(sessions::control(1, handles[1], 4, 0, 0), 0);
                }
                2 => {
                    assert_eq!(read_exit(handles[1]), 137);
                    assert_eq!(world().released, vec![AP, AP]);
                }
                _ => return Ok(true),
            }
            checks += 1;
            Ok(false)
        },
        || test.worker.poll(),
        || {
            sleeps += 1;
            let mut yielded = false;
            core::future::poll_fn(move |context| {
                if yielded {
                    core::task::Poll::Ready(())
                } else {
                    yielded = true;
                    context.waker().wake_by_ref();
                    core::task::Poll::Pending
                }
            })
        },
    ));
    test.handles.extend(handles);
    assert!(presented);
    assert_eq!(sleeps, 3);
    assert_eq!(world().tty_created, 0);
}

#[test]
fn first_presentation_wait_keeps_existing_error_and_timeout_bounds() {
    let mut progress = 0;
    assert!(!run_wait(first_presentation::wait(
        || Err::<bool, ()>(()),
        || progress += 1,
        || async {},
    )));
    assert_eq!(progress, 1);
    let mut sleeps = 0;
    progress = 0;
    assert!(!run_wait(first_presentation::wait(
        || Ok::<bool, ()>(false),
        || progress += 1,
        || { sleeps += 1; async {} },
    )));
    assert_eq!(progress, 10_000);
    assert_eq!(sleeps, 10_000);
}
