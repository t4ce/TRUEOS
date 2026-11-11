//! Unrestricted native host access-key manager; never exposed as a guest ABI.
use super::{
    MatrixTarget,
    helper::{self, Screen},
    tui::{self, Frontend},
};
use crate::{
    access_key_db as policy,
    access_key_store::{AdminChange, Catalog},
};
use alloc::{
    format,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use spin::Mutex;
use trueos_crypto::AccountId;
use trueos_fs::{RecordKey, access::GrantScope};
use trueos_time::{Duration, Timer};

#[path = "keys_model.rs"]
mod model;
use model::{Event, Input};

type Reply = Arc<Mutex<Option<Result<Catalog, String>>>>;

pub(super) fn start(frontend: Frontend) -> Result<(), String> {
    let Some((target, spawner)) = helper::admit("keys", frontend)? else {
        return Ok(());
    };
    let task = menu_task(target.clone()).map_err(|_| {
        tui::cancel_native_attach(&target);
        String::from("Keys menu task pool is full.")
    })?;
    spawner.spawn(task);
    Ok(())
}

fn request(change: Option<AdminChange>) -> Reply {
    let reply = Arc::new(Mutex::new(None));
    let result = reply.clone();
    crate::wait::spawn_local_detached(async move {
        let update = async {
            if let Some(change) = change {
                crate::access_key_workspace::save_admin_change(&change)
                    .await
                    .map_err(|e| format!("Save failed: {e:?}"))?;
            }
            crate::access_key_workspace::load_admin_catalog()
                .await
                .map_err(|e| format!("Load failed: {e:?}"))
        }
        .await;
        *result.lock() = Some(update);
    });
    reply
}

fn keys(state: &Catalog, runtime: &trueos_fs::access::PolicySnapshot) -> Vec<RecordKey> {
    let mut keys = alloc::vec![RecordKey::Ffa];
    for key in state
        .workspaces
        .iter()
        .map(|(_, r)| r.key)
        .chain(runtime.keys.iter().copied())
        .chain(state.aliases.iter().map(|(k, _)| *k))
        .chain(state.grants.iter().map(|(_, _, k)| *k))
    {
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    keys
}

fn label(state: &Catalog, key: RecordKey) -> String {
    if let Some((_, name)) = state.aliases.iter().find(|(k, _)| *k == key) {
        return name.clone();
    }
    if key == RecordKey::Ffa {
        return "public (FFA)".into();
    }
    state
        .workspaces
        .iter()
        .find(|(_, r)| r.key == key)
        .map(|(path, _)| path.clone())
        .unwrap_or_else(|| "unnamed".into())
}

fn reference(key: RecordKey) -> String {
    match key {
        RecordKey::Ffa => "FFA".into(),
        RecordKey::Key(key) => {
            use core::fmt::Write;
            let mut value = String::new();
            for byte in key.provider.as_bytes() {
                let _ = write!(value, "{byte:02x}");
            }
            value.push(':');
            for byte in key.handle.as_bytes() {
                let _ = write!(value, "{byte:02x}");
            }
            value
        }
    }
}

fn table(state: &Catalog, current: Option<AccountId>) -> (Vec<String>, Vec<RecordKey>) {
    let runtime = policy::policy_snapshot();
    let keys = keys(state, &runtime);
    let number = |key| keys.iter().position(|k| *k == key).unwrap_or(0);
    let launches = policy::launched_apps();
    let mut users = Vec::new();
    for user in current
        .into_iter()
        .chain(crate::crypt::local_shell_account())
        .chain(state.workspaces.iter().map(|(_, r)| r.owner))
        .chain(state.grants.iter().map(|(u, _, _)| *u))
        .chain(launches.iter().filter_map(|(_, u)| *u))
        .chain(
            runtime
                .containers
                .iter()
                .flat_map(|(_, _, users)| users.iter().copied()),
        )
    {
        if !users.contains(&user) {
            users.push(user);
        }
    }
    let mut lines = alloc::vec![String::from(
        "KEY  NAME / FILE ROOTS (record key inherited by files)"
    )];
    for (n, key) in keys.iter().enumerate() {
        lines.push(format!("{n:<4} {}", label(state, *key)));
        let reference = reference(*key);
        if let Some((provider, handle)) = reference.split_once(':') {
            lines.push(format!("     provider {provider}"));
            lines.push(format!("     key {handle}"));
        } else {
            lines.push(format!("     id {reference}"));
        }
        if *key == RecordKey::Ffa {
            lines.push("     -> public files; accessible to all apps/users".into());
        }
        for (root, record) in state.workspaces.iter().filter(|(_, r)| r.key == *key) {
            lines.push(format!(
                "     -> /{root}   owner user {}",
                record.owner.raw()
            ));
        }
    }
    lines.push(String::new());
    lines.push("USER ID / NAME".into());
    for user in users {
        lines.push(format!(
            "{}  {}{}",
            user.raw(),
            crate::crypt::account_username(user).unwrap_or_else(|| "(name unavailable)".into()),
            if Some(user) == current {
                " (this terminal)"
            } else {
                ""
            }
        ));
    }
    lines.push(String::new());
    lines.push("APPS LAUNCHED THIS BOOT / USER / CONTAINER".into());
    for (id, user) in &launches {
        lines.push(format!(
            "{}  user {}  container {}",
            id.app().as_str(),
            user.map(|u| u.raw().to_string())
                .unwrap_or_else(|| "anonymous; relaunch signed in".into()),
            id.id()
        ));
    }
    lines.push("Any canonical app name can receive a grant before its first launch.".into());
    lines.push(String::new());
    lines.push("ASSIGNMENTS: USER -> APP [SCOPE] -> KEY".into());
    for (user, scope, key) in &runtime.grants {
        let (app, scope) = match scope {
            GrantScope::App(app) => (app.as_str(), String::from("all admitted containers")),
            GrantScope::Container(id) => (id.app().as_str(), format!("container {}", id.id())),
        };
        let saved = matches!(scope.as_str(), "container 0")
            && state
                .grants
                .iter()
                .any(|(u, a, k)| u == user && a == app && k == key);
        lines.push(format!(
            "{} -> {} [{}] -> {} ({}){}",
            user.raw(),
            app,
            scope,
            number(*key),
            label(state, *key),
            if saved { " [saved]" } else { " [native]" }
        ));
    }
    lines.push(String::new());
    lines.push("MEMBERSHIP: USER -> WORKSPACE (required for keyed access)".into());
    for (id, _, members) in &runtime.containers {
        for user in members {
            lines.push(format!("{} -> /{}", user.raw(), id.workspace_root()));
        }
    }
    (lines, keys)
}

#[trueos_executor::task(pool_size = 2)]
async fn menu_task(target: MatrixTarget) {
    let mut screen = Screen::default();
    let mut input = Input::default();
    let mut state = Catalog::default();
    let mut pending = Some(request(None));
    let mut status = String::from("Loading access-key catalog...");
    let mut scroll = 0usize;
    let mut visible = false;
    while let Some((bytes, _)) = tui::native_read(&target) {
        if !tui::native_visible(&target) {
            visible = false;
            input = Input::default();
            Timer::after(Duration::from_millis(20)).await;
            continue;
        }
        let Some(frontend) = tui::native_frontend(&target) else {
            break;
        };
        if !visible {
            screen.invalidate();
            visible = true;
            if pending.is_none() {
                pending = Some(request(None));
            }
        }
        if let Some(reply) = &pending {
            let result = reply.lock().take();
            if let Some(result) = result {
                match result {
                    Ok(next) => {
                        state = next;
                        status = "Catalog loaded; saved grants are active.".into();
                    }
                    Err(error) => {
                        status = error;
                    }
                }
                pending = None;
            }
        }
        let account = tui::frontend_access_account(frontend);
        let (body, key_ids) = table(&state, account);
        let selectors: Vec<_> = key_ids
            .iter()
            .map(|k| (*k, label(&state, *k)))
            .chain(
                state
                    .workspaces
                    .iter()
                    .map(|(path, record)| (record.key, path.clone())),
            )
            .chain(state.aliases.iter().cloned())
            .collect();
        let now = crate::chronos::monotonic_nanos();
        for event in input
            .feed(&bytes, now)
            .into_iter()
            .chain(input.timeout(now))
        {
            match event {
                Event::Quit => {
                    tui::native_write(&target, b"\x1b[?1000l\x1b[?1006l\x1b[?25h\x1b[0m");
                    tui::native_return(&target);
                    visible = false;
                }
                Event::Scroll(delta) => scroll = scroll.saturating_add_signed(delta),
                Event::Submit(command) => {
                    if pending.is_some() {
                        status = "Wait for the current save/load to complete.".into();
                        continue;
                    }
                    match model::command(&command, &key_ids, &selectors, account) {
                        Ok(change) => {
                            pending = Some(request(change));
                            status = "Working...".into();
                        }
                        Err(error) => status = error.into(),
                    }
                }
            }
        }
        if !visible {
            continue;
        }
        let height = frontend.rows.saturating_sub(7);
        scroll = scroll.min(body.len().saturating_sub(height));
        let mut lines = alloc::vec![
            "KEYS  Filesystem access | unrestricted host management".into(),
            "grant KEY APP [USER] | revoke KEY APP [USER] | alias KEY NAME".into(),
            "Example: grant apps/codex termdir  |  refresh  |  Esc return".into(),
            "KEY = row, alias or workspace; grants apply to default container 0.".into(),
        ];
        lines.extend(body.iter().skip(scroll).take(height).cloned());
        while lines.len() < frontend.rows.saturating_sub(3) {
            lines.push(String::new());
        }
        lines.push(format!("{}  [arrows/PageUp/PageDown scroll]", status));
        lines.push(format!("> {}", input.text()));
        lines.push(format!(
            "Rows {}-{}/{}",
            scroll + 1,
            (scroll + height).min(body.len()),
            body.len()
        ));
        screen.paint(&target, &lines, frontend.cols, frontend.rows);
        Timer::after(Duration::from_millis(20)).await;
    }
}
