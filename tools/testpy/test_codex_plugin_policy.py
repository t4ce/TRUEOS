#!/usr/bin/env python3
"""Compile actual feature policy and catalog admission for host and TRUEOS cfg.

The full Codex workspace can require unrelated platform dependencies. This harness
uses the production enums, registry, policy methods, config-map application, and
catalog scheduling gate unchanged. Fixtures replace only legacy notices/aliases
(not used here), tracing, authentication, and the scheduling boundary.
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
enum RemoteCatalogCacheRefreshMode { Force }
struct RemoteCatalogCacheRefreshRequest {
    service_config: (), auth: Option<CodexAuth>,
    scopes: BTreeSet<RemotePluginScope>,
}
fn remote_plugin_service_config(_: &PluginsConfigInput) {}
#[derive(Default)] struct PluginManager { scheduled: AtomicUsize }
impl PluginManager {
    fn remote_plugin_service_config(&self, _: &PluginsConfigInput) {}
    fn schedule_remote_catalog_cache_refresh(&self, _: RemoteCatalogCacheRefreshRequest) {
        self.scheduled.fetch_add(1, Ordering::Relaxed);
    }
"""
    + item(manager, r"^    fn maybe_start_remote_catalog_cache_refresh\b") + "}"]
    for name in [
        "plugin_features_respect_the_platform_policy",
        "plugin_platform_policy_applies_to_config_toggles",
        "plugin_platform_policy_also_filters_imported_feature_sets",
        "plugin_platform_policy_preserves_apps_and_other_features",
    ]:
        declarations += ["#[test]", item(tests, rf"^fn {name}\b")]
    declarations += ["""
#[test]
fn catalog_refresh_cannot_be_admitted_when_plugins_are_disabled() {
    let mut features = Features::with_defaults();
    features.apply_map(&BTreeMap::from([("plugins".into(), true)]));
    let manager = Arc::new(PluginManager::default());
    manager.maybe_start_remote_catalog_cache_refresh(
        &PluginsConfigInput { plugins_enabled: features.enabled(Feature::Plugins) },
        Some(CodexAuth), BTreeSet::from([RemotePluginScope::Global]),
    );
    assert_eq!(manager.scheduled.load(Ordering::Relaxed),
        if cfg!(target_os = "trueos") { 0 } else { 1 });
}
#[test]
fn plugin_defaults_respect_the_platform_policy() {
    let features = Features::with_defaults();
    for feature in [Feature::Plugins, Feature::RemotePlugin, Feature::PluginSharing] {
        assert_eq!(feature.default_enabled(), !cfg!(target_os = "trueos"));
        assert_eq!(features.enabled(feature), !cfg!(target_os = "trueos"));
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
            output = subprocess.check_output([str(binary)], text=True)
            assert "6 passed; 0 failed" in output, output
            results.append({"platform": binary.name, "passed": 6})
    print(json.dumps({"status": "PASS", "results": results}))


if __name__ == "__main__":
    main()
