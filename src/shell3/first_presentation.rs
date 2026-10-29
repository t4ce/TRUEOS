//! Keep command sessions responsive while this same AP waits for first paint.

pub(super) async fn wait<E, Sleep: core::future::Future<Output = ()>>(
    mut ready: impl FnMut() -> Result<bool, E>,
    mut progress_commands: impl FnMut(),
    mut sleep: impl FnMut() -> Sleep,
) -> bool {
    for _ in 0..10_000 {
        progress_commands();
        match ready() {
            Ok(true) => return true,
            Ok(false) => sleep().await,
            Err(_) => return false,
        }
    }
    false
}
