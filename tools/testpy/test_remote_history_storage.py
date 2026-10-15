#!/usr/bin/env python3
"""Check Codex history storage boundaries with the real remote SQLx service.

Extract the production checkpoint/deletion functions and SQLite existence
method. Only the surrounding store construction is a fixture. This exercises
the TRUEOS storage branch on Linux; it does not claim native scheduling or the
complete rollout-writer lifecycle.
"""
from pathlib import Path
import argparse
import os
import re
import socket
import subprocess
import tempfile
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
CODEX = ROOT.parent / "codex/codex-rs"
BLUEPRINTS = ROOT.parent / "TRUEOS-Blueprints"


def function(source, name):
    match = re.search(
        rf'^pub(?:\([^)]*\))? async fn {name}\b.*?^}}',
        source,
        re.MULTILINE | re.DOTALL,
    )
    if match is None:
        raise ValueError(f"missing production function {name}")
    return match.group(0)


def harness_source():
    sqlite = (CODEX / "state/src/sqlite.rs").read_text()
    match = re.search(
        r'^    pub async fn database_exists\b.*?^    }',
        sqlite,
        re.MULTILINE | re.DOTALL,
    )
    if match is None:
        raise ValueError("missing production SqliteConfig::database_exists")
    # Compile the real TRUEOS branch with the host remote adapter. No alternate
    # implementation of its database-existence semantics lives in this test.
    exists = match.group(0).replace(
        '#[cfg(not(target_os = "trueos"))]', '#[cfg(any())]'
    ).replace('#[cfg(target_os = "trueos")]', '#[cfg(all())]')
    history = (CODEX / "thread-store/src/local/thread_history.rs").read_text()
    production = "\n".join(function(history, name) for name in (
        "projection_state", "delete_thread",
    ))
    return '''
use sqlx::{ConnectOptions, Error, Row};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::path::{Path, PathBuf};
use tokio::sync::OnceCell;
struct SqliteConfig { path: PathBuf }
impl SqliteConfig {
    fn thread_history_db_path(&self) -> PathBuf { self.path.clone() }
''' + exists + '''
}
struct Config { sqlite: SqliteConfig }
struct LocalThreadStore {
    config: Config,
    state_db: Option<()>,
    thread_history_db: OnceCell<sqlx::SqlitePool>,
}
impl LocalThreadStore {
    fn new(path: PathBuf) -> Self {
        Self { config: Config { sqlite: SqliteConfig { path } }, state_db: Some(()), thread_history_db: OnceCell::new() }
    }
    async fn thread_history_db(&self) -> ThreadStoreResult<&sqlx::SqlitePool> {
        self.thread_history_db.get_or_try_init(|| async {
            SqlitePoolOptions::new().max_connections(2).connect_with(
                SqliteConnectOptions::new().filename(self.config.sqlite.thread_history_db_path()).create_if_missing(false)
            ).await.map_err(thread_history_error)
        }).await
    }
}
#[derive(Clone, Copy)] struct ThreadId(&'static str);
impl std::fmt::Display for ThreadId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { self.0.fmt(f) }
}
#[derive(Debug)] enum ThreadStoreError { Internal { message: String } }
impl std::fmt::Display for ThreadStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self { Self::Internal { message } => message.fmt(f) }
    }
}
impl std::error::Error for ThreadStoreError {}
type ThreadStoreResult<T> = Result<T, ThreadStoreError>;
fn thread_history_error(error: impl std::fmt::Display) -> ThreadStoreError {
    ThreadStoreError::Internal { message: error.to_string() }
}
fn thread_history_delete_error(error: impl std::fmt::Display) -> ThreadStoreError {
    thread_history_error(error)
}
struct RolloutProjectionState { next_byte_offset: u64, next_ordinal: u64 }
mod production {
use super::*;
''' + production + '''
}
async fn checkpoint(store: &LocalThreadStore, id: ThreadId) -> (u64, u64) {
    let state = production::projection_state(store, id).await.unwrap().unwrap();
    (state.next_byte_offset, state.next_ordinal)
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let home = PathBuf::from(std::env::var_os("REMOTE_HISTORY_TEST_HOME").unwrap());
    let path = home.join("logical-client-home/thread_history.sqlite");
    let store = LocalThreadStore::new(path.clone());
    let absent = LocalThreadStore::new(home.join("absent.sqlite"));
    assert!(!store.config.sqlite.database_exists(&path).await?);
    assert!(production::projection_state(&store, ThreadId("alpha")).await?.is_none());
    production::delete_thread(&absent, ThreadId("absent")).await?;
    assert!(!absent.config.sqlite.database_exists(&absent.config.sqlite.path).await?);
    assert!(absent.thread_history_db.get().is_none());
    let pool = SqlitePoolOptions::new().max_connections(2).connect_with(
        SqliteConnectOptions::new().filename(&path).create_if_missing(true)
    ).await?;
    sqlx::raw_sql("CREATE TABLE thread_history_projection_state(thread_id TEXT PRIMARY KEY,next_rollout_byte_offset INTEGER,next_rollout_ordinal INTEGER); CREATE TABLE thread_items(thread_id TEXT); CREATE TABLE thread_realtime_items(thread_id TEXT); CREATE TABLE thread_turns(thread_id TEXT);")
        .execute(&pool).await?;
    sqlx::query("INSERT INTO thread_history_projection_state VALUES ('alpha',100,2),('beta',70,1)").execute(&pool).await?;
    for table in ["thread_items", "thread_realtime_items", "thread_turns"] {
        sqlx::query(sqlx::AssertSqlSafe(format!("INSERT INTO {table} VALUES ('alpha'),('beta')"))).execute(&pool).await?;
    }
    assert!(!path.exists(), "remote storage must have no client database file");
    assert!(store.config.sqlite.database_exists(&path).await?);
    // First read must query the remote owner before the store has a pool.
    assert_eq!(checkpoint(&store, ThreadId("alpha")).await, (100, 2));
    assert!(store.thread_history_db.get().is_some());
    // Advance the durable checkpoint, then consume the next append from that
    // offset. Returning None here would replay from zero and fail apply_projection.
    sqlx::query("UPDATE thread_history_projection_state SET next_rollout_byte_offset=240,next_rollout_ordinal=5 WHERE thread_id='alpha'").execute(&pool).await?;
    assert_eq!(checkpoint(&store, ThreadId("alpha")).await, (240, 5));
    assert!(!path.exists());
    let reopened = LocalThreadStore::new(path.clone());
    assert_eq!(checkpoint(&reopened, ThreadId("alpha")).await, (240, 5));
    let delete_store = LocalThreadStore::new(path.clone());
    production::delete_thread(&delete_store, ThreadId("alpha")).await?;
    for table in ["thread_items", "thread_realtime_items", "thread_turns", "thread_history_projection_state"] {
        let row = sqlx::query(sqlx::AssertSqlSafe(format!("SELECT COUNT(*) AS remaining FROM {table} WHERE thread_id='alpha'"))).fetch_one(&pool).await?;
        assert_eq!(row.try_get::<i64, _>("remaining")?, 0);
        let row = sqlx::query(sqlx::AssertSqlSafe(format!("SELECT COUNT(*) AS remaining FROM {table} WHERE thread_id='beta'"))).fetch_one(&pool).await?;
        assert_eq!(row.try_get::<i64, _>("remaining")?, 1);
    }
    // Existing opened pools do not require another service-open just to test
    // existence. A standalone existence check still preserves transport errors.
    let previous_endpoint = std::env::var("TRUEOS_SQLITE_ENDPOINT")?;
    unsafe { std::env::set_var("TRUEOS_SQLITE_ENDPOINT", std::env::var("REMOTE_HISTORY_DEAD_ENDPOINT")?); }
    assert_eq!(checkpoint(&store, ThreadId("beta")).await, (70, 1));
    assert!(store.config.sqlite.database_exists(&path).await.is_err());
    unsafe { std::env::set_var("TRUEOS_SQLITE_ENDPOINT", previous_endpoint); }
    for store in [&store, &reopened, &delete_store] {
        store.thread_history_db.get().unwrap().close().await;
    }
    pool.close().await;
    println!("remote-history: PASS absent_no_create checkpoint_resume second_append_checkpoint reopen_checkpoint delete_preserves_peer initialized_pool_no_reopen transport_error_preserved no_client_file");
    Ok(())
}
'''


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--service", type=Path, default=BLUEPRINTS / "apps/sqlite/host/target/debug/sqlite")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="remote-history-contract-") as folder:
        folder = Path(folder)
        (folder / "src").mkdir()
        (folder / "src/main.rs").write_text(harness_source())
        (folder / "Cargo.toml").write_text(f'''[package]
name = "remote-history-contract"
version = "0.1.0"
edition = "2024"
[workspace]
[dependencies]
anyhow = "1"
sqlx = {{ version = "=0.9.0", default-features = false, features = ["sqlite", "runtime-tokio"] }}
sqlx-sqlite = {{ path = "{BLUEPRINTS / 'vendor/sqlx-sqlite-0.9.0'}", features = ["trueos-remote", "offline"] }}
tokio = {{ version = "=1.52.3", features = ["rt", "macros", "time", "sync"] }}
[patch.crates-io]
sqlx-sqlite = {{ path = "{BLUEPRINTS / 'vendor/sqlx-sqlite-0.9.0'}" }}
''')
        target = ROOT / "bld/remote-history-contract"
        subprocess.run(["cargo", "build", "--offline", "--manifest-path", str(folder / "Cargo.toml"),
                        "--target", "x86_64-unknown-linux-gnu", "--target-dir", str(target)],
                       cwd=folder, check=True)
        with socket.socket() as dead:
            dead.bind(("127.0.0.1", 0))
            dead_endpoint = f"http://127.0.0.1:{dead.getsockname()[1]}"
        with (folder / "service.log").open("w+") as log:
            service = subprocess.Popen([str(args.service), "--data-dir", str(folder / "service")],
                                       stdout=log, stderr=log)
            try:
                for _ in range(100):
                    if service.poll() is not None:
                        raise RuntimeError("synthetic SQLite service exited before readiness")
                    try:
                        urllib.request.urlopen("http://127.0.0.1:40894/healthz", timeout=.5).close()
                        break
                    except OSError:
                        time.sleep(.05)
                else:
                    raise RuntimeError("synthetic SQLite service did not become ready")
                env = os.environ | {
                    "TRUEOS_SQLITE_ENDPOINT": "http://127.0.0.1:40894",
                    "REMOTE_HISTORY_TEST_HOME": str(folder / "client"),
                    "REMOTE_HISTORY_DEAD_ENDPOINT": dead_endpoint,
                }
                subprocess.run([str(target / "x86_64-unknown-linux-gnu/debug/remote-history-contract")],
                               env=env, check=True, timeout=90)
                assert not (folder / "client").exists(), "service operations created a client file"
                images = list((folder / "service/default").glob("*.sqlite"))
                assert len(images) == 1, "absence checks created an extra remote image"
            finally:
                service.terminate()
                service.wait(timeout=10)


if __name__ == "__main__":
    main()
