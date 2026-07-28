pub(crate) mod html_shack;

use embassy_executor::{SpawnError, Spawner};

pub(crate) fn spawn_html_fetch_service(spawner: Spawner) -> Result<bool, SpawnError> {
    let mut spawned = false;
    for _ in 0..html_shack::HTML_FETCH_WORKERS {
        let token = html_shack::html_fetch_worker_task()?;
        spawner.spawn(token);
        spawned = true;
    }
    Ok(spawned)
}
