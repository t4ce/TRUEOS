#!/usr/bin/env python3
"""Compile actual plugin availability and catalog admission for host and TRUEOS.

The full Codex workspace can require unrelated platform dependencies. This harness
uses the production enums, registry, policy methods, config-map application, and
catalog scheduling gate unchanged. Fixtures replace only legacy notices/aliases
(not used here), tracing, authentication, and the scheduling boundary.

The startup source guard complements the app-server integration test
app_server_startup_downloads_installed_plugin_bundles_without_discovery; it does
not substitute for executing installed plugins on TRUEOS.
"""

import json
import os
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
CODEX = Path(os.environ.get("TRUEOS_CODEX_ROOT", ROOT.parent / "codex"))


def item(source, pattern):
    match = re.search(pattern, source, re.M)
    assert match, pattern
    brace = source.index("{", match.end())
    # Select at the item's indentation, preserving nested Rust blocks verbatim.
    indent = re.match(r" *", source[match.start():]).group()
    end = re.search(r"^" + indent + r"}\s*\n", source[brace:], re.M)
    assert end, pattern
    return source[match.start():brace + end.end()]


def main():
    source = (CODEX / "codex-rs/features/src/lib.rs").read_text()
    tests = (CODEX / "codex-rs/features/src/tests.rs").read_text()
    manager = (CODEX / "codex-rs/core-plugins/src/manager.rs").read_text()
    startup = item(manager, r"^    pub fn maybe_start_plugin_startup_tasks_for_config\b")
    # Startup retains runtime refreshes but must not pull the discovery catalog.
    # Listing explicitly invokes discovery and uses the same admission gate below.
    for forbidden in [
        "maybe_start_remote_catalog_cache_refresh",
        "maybe_start_remote_plugin_caches_refresh",
        "recommended_plugins_mode_for_config",
        "featured_plugin_ids_for_config",
        "fetch_and_cache_remote_plugin_catalog",
    ]:
        assert forbidden not in startup, f"startup eagerly calls {forbidden}"
    for required in [
        "maybe_start_remote_installed_plugins_cache_refresh_with_notify",
        "maybe_start_remote_installed_plugin_bundle_sync",
    ]:
        assert required in startup, f"startup lost installed-plugin refresh {required}"
    registry_start = source.index("pub const FEATURES:")
    registry_end = source.index("\n];", registry_start) + 3
    declarations = [
        "#![allow(dead_code, unexpected_cfgs)]",
        "use std::collections::{BTreeMap, BTreeSet};",
        "use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};",
        "extern crate self as tracing;",
        '#[macro_export] macro_rules! warn { ($($tokens:tt)*) => {{}}; }',
    ]
    for name, kind, derives in [
        ("Stage", "enum", "Debug, Clone, Copy, PartialEq, Eq"),
        ("Feature", "enum", "Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash"),
        ("FeatureSpec", "struct", "Debug, Clone, Copy"),
        ("LegacyFeatureUsage", "struct", "Debug, Clone, PartialEq, Eq, PartialOrd, Ord"),
        ("Features", "struct", "Debug, Clone, Default, PartialEq"),
    ]:
        declarations += [f"#[derive({derives})]", item(source, rf"^pub {kind} {name}\b")]
    declarations += [source[registry_start:registry_end], item(source, r"^impl Feature\b")]
    methods = [
        item(source, rf"^    pub fn {name}\b") for name in [
            "with_defaults", "enabled", "enable", "disable", "set_enabled",
            "plugin_recommendations_enabled", "apply_map",
        ]
    ]
    methods += [
        "fn record_legacy_usage(&mut self, _: &str, _: Feature) {}",
        "fn record_legacy_usage_force(&mut self, _: &str, _: Feature) {}",
    ]
    declarations += ["impl Features {" + "\n".join(methods) + "}"]
    declarations += [item(source, r"^pub fn feature_for_key\b"), """
mod legacy { pub fn feature_for_key(_: &str) -> Option<super::Feature> { None } }
struct PluginsConfigInput { plugins_enabled: bool }
struct CodexAuth;
#[derive(PartialEq, Eq, PartialOrd, Ord)] enum RemotePluginScope { Global }
struct RemoteCatalogCacheRefreshRequest {
    service_config: (), auth: Option<CodexAuth>,
    scopes: BTreeSet<RemotePluginScope>,
}
#[derive(Default)] struct PluginManager { scheduled: AtomicUsize }
impl PluginManager {
    fn remote_plugin_service_config(&self, _: &PluginsConfigInput) {}
    fn schedule_remote_catalog_cache_refresh(&self, _: RemoteCatalogCacheRefreshRequest) {
        self.scheduled.fetch_add(1, Ordering::Relaxed);
    }
"""
    + item(manager, r"^    fn maybe_start_remote_catalog_cache_refresh\b") + "}"]
    for name in [
        "plugin_features_support_explicit_toggles_on_all_targets",
        "plugin_features_support_config_toggles_on_all_targets",
        "plugin_features_preserve_imported_feature_sets",
        "plugin_recommendations_keep_their_feature_dependencies",
    ]:
        declarations += ["#[test]", item(tests, rf"^fn {name}\b")]
    declarations += ["""
#[test]
fn catalog_refresh_cannot_be_admitted_when_plugins_are_disabled() {
    let manager = Arc::new(PluginManager::default());
    manager.maybe_start_remote_catalog_cache_refresh(
        &PluginsConfigInput { plugins_enabled: false },
        Some(CodexAuth), BTreeSet::from([RemotePluginScope::Global]),
    );
    assert_eq!(manager.scheduled.load(Ordering::Relaxed), 0);
}
#[test]
fn explicit_catalog_refresh_is_available_on_all_targets() {
    let features = Features::with_defaults();
    let manager = Arc::new(PluginManager::default());
    manager.maybe_start_remote_catalog_cache_refresh(
        &PluginsConfigInput { plugins_enabled: features.enabled(Feature::Plugins) },
        Some(CodexAuth), BTreeSet::from([RemotePluginScope::Global]),
    );
    assert_eq!(manager.scheduled.load(Ordering::Relaxed), 1);
}
#[test]
fn catalog_refresh_requires_explicit_scopes() {
    let manager = Arc::new(PluginManager::default());
    manager.maybe_start_remote_catalog_cache_refresh(
        &PluginsConfigInput { plugins_enabled: true }, Some(CodexAuth), BTreeSet::new(),
    );
    assert_eq!(manager.scheduled.load(Ordering::Relaxed), 0);
}
#[test]
fn plugin_defaults_match_upstream_on_all_targets() {
    let features = Features::with_defaults();
    for feature in [Feature::Plugins, Feature::RemotePlugin, Feature::PluginSharing] {
        assert!(feature.default_enabled());
        assert!(features.enabled(feature));
    }
}
"""
    ]
    results = []
    with tempfile.TemporaryDirectory(prefix="trueos-codex-plugin-policy-") as temp:
        path = Path(temp) / "policy.rs"
        path.write_text("\n".join(declarations))
        for trueos in (False, True):
            binary = Path(temp) / ("trueos" if trueos else "host")
            command = ["rustc", "--edition=2024", "--test", str(path), "-o", str(binary)]
            if trueos:
                command += ["--cfg", 'target_os="trueos"', "-Aexplicit_builtin_cfgs_in_flags"]
            subprocess.run(command, check=True)
            result = subprocess.run([str(binary)], check=False, capture_output=True, text=True)
            assert result.returncode == 0, result.stdout + result.stderr
            assert "8 passed; 0 failed" in result.stdout, result.stdout
            results.append({"platform": binary.name, "passed": 8})
    print(json.dumps({"status": "PASS", "startup_discovery_source_guard": "PASS", "results": results}))


if __name__ == "__main__":
    main()
