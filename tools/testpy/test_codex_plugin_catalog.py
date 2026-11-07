#!/usr/bin/env python3
"""Run the production Codex catalog cache and its unit tests in isolation.

The complete workspace currently has an unrelated reqwest/hyper feature conflict.
Only account identity, HTTP configuration and the plugin item boundary are fixtures;
catalog serialization, buffered I/O, atomic persistence and cache tests are unchanged.
This is a host regression harness, not a TRUEOS filesystem or live plugin test.
It does not measure allocations inside the target filesystem backend.
"""

import json
import os
from pathlib import Path
import subprocess
import tempfile
import tomllib


ROOT = Path(__file__).resolve().parents[2]
CODEX = Path(os.environ.get("TRUEOS_CODEX_ROOT", ROOT.parent / "codex"))


def main():
    module = CODEX / "codex-rs/core-plugins/src/remote/catalog_cache.rs"
    packages = tomllib.loads((CODEX / "codex-rs/Cargo.lock").read_text())["package"]
    versions = {
        package["name"]: package["version"]
        for package in packages
        if package["name"] != "http" or package["version"].startswith("1.")
    }
    with tempfile.TemporaryDirectory(prefix="trueos-codex-plugin-catalog-") as workspace:
        workspace = Path(workspace)
        (workspace / "src").mkdir()
        (workspace / "Cargo.toml").write_text(f'''[package]
name = "trueos-codex-plugin-catalog-regression"
version = "0.0.0"
edition = "2024"

[dependencies]
chrono = {{ version = "={versions["chrono"]}", features = ["serde"] }}
http = "={versions["http"]}"
pretty_assertions = "={versions["pretty_assertions"]}"
serde = {{ version = "={versions["serde"]}", features = ["derive"] }}
serde_json = "={versions["serde_json"]}"
tempfile = "={versions["tempfile"]}"
tracing = "={versions["tracing"]}"
''')
        (workspace / "src/lib.rs").write_text('''#![allow(dead_code)]
extern crate self as codex_login;
extern crate self as codex_utils_paths;

pub struct AuthHeaders;
impl AuthHeaders {
    pub fn new(_: http::HeaderMap) -> Self { Self }
}
pub enum CodexAuth { Headers(AuthHeaders) }
impl CodexAuth {
    pub fn get_account_id(&self) -> Option<String> { None }
    pub fn get_chatgpt_user_id(&self) -> Option<String> { None }
    pub fn is_workspace_account(&self) -> bool { false }
}

pub mod path {
    pub fn write_atomically(path: &std::path::Path, contents: &str) -> std::io::Result<()> {
        use std::io::Write;
        let parent = path.parent().expect("cache parent");
        std::fs::create_dir_all(parent)?;
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        file.write_all(contents.as_bytes())?;
        file.persist(path)?;
        Ok(())
    }
}

pub mod test_support {
    pub fn test_http_client_factory() {}
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RemotePluginCatalogCacheMode { PreferCache, PreferFreshCache, ForceRefetch }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
enum RemotePluginScope { Global, User, Workspace }
#[derive(serde::Serialize, serde::Deserialize)]
struct RemotePluginDirectoryItem { id: String, name: String }
struct RemotePluginServiceConfig { chatgpt_base_url: String }
impl RemotePluginServiceConfig {
    fn new(chatgpt_base_url: String, _: (), _: Option<String>) -> Self {
        Self { chatgpt_base_url }
    }
}
''' + f"\n#[path = {json.dumps(str(module))}]\nmod catalog_cache;\n")
        env = os.environ.copy()
        env.setdefault("CARGO_TARGET_DIR", str(ROOT / "bld/test-codex-plugin-catalog"))
        subprocess.run(
            ["cargo", "test", "--offline", "--manifest-path", str(workspace / "Cargo.toml")],
            cwd=workspace,
            env=env,
            check=True,
        )


if __name__ == "__main__":
    main()
