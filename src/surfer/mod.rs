pub(crate) mod html_shack;

use embassy_executor::SpawnError;

pub(crate) fn spawn_html_fetch_service() -> Result<bool, SpawnError> {
    let mut spawned = false;
    for _ in 0..html_shack::HTML_FETCH_WORKERS {
        let Some((_slot, _kind, spawner)) = crate::workers::pick_eff_background_spawner_with_slot()
            .or_else(crate::workers::pick_background_spawner_with_slot)
        else {
            break;
        };
        let token = html_shack::html_fetch_worker_task()?;
        spawner.spawn(token);
        spawned = true;
    }
    Ok(spawned)
}
