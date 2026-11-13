"""Execute the real selection/packaging adapter with a sentinel compiler module.

The fixture replaces Rust compilation and redb generation, but exercises actual
selection validation, source traversal, output replacement, AppDB arguments,
readiness and tamper checks. Full OS compilation still needs the archived tools.
"""
from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import buildins
from build_plan import catalog


FAKE_BUILDER = r'''
import hashlib, json, os, shutil, subprocess, sys, tomllib
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]
COMMIT = "af3d95584dbddcae597890340995509a7fb47a50"
IGNORED = {".git", "target", "tgt", "__pycache__", "Cargo.lock"}
NATIVE_CORE_ALLOC_BUILTINS = set(json.loads((ROOT / "buildins.json").read_text())["buildins"])
def log(value):
    with open(os.environ["TRUEOS_TEST_CALLS"], "a") as stream:
        stream.write(json.dumps(value) + "\n")
def settings(name):
    manifest = ROOT / name / "Cargo.toml"
    doc = tomllib.loads(manifest.read_text())
    source = ROOT / name / "src/main.rs"
    source.read_bytes()
    log(["settings", name])
    return manifest, source, os.environ.get("TRUEOS_TEST_STD_APP") == name, [], doc
def tool_environment(output, kernel, needs_std):
    log(["toolchain", needs_std])
    return Path("/sentinel/bin/cargo"), Path("/sentinel/bin"), dict(os.environ)
def patches(kernel):
    return {}
def input_roots(kernel):
    names = json.loads((ROOT / "buildins.json").read_text())["buildins"]
    return [ROOT / "buildins.json", ROOT / "tools", ROOT / "vendor",
            *(ROOT / name for name in names), kernel / "Cargo.toml", kernel / "tools/appdb-seed"]
def dependency_path(relative, original, staged, app_root):
    return (original.parent / relative).resolve()
def sync_tree(source, target, transformed=None):
    target.mkdir(parents=True, exist_ok=True)
    for old in target.iterdir():
        if old.name not in IGNORED and not (source / old.name).exists():
            shutil.rmtree(old) if old.is_dir() else old.unlink()
    for path in source.iterdir():
        if path.name in IGNORED:
            continue
        if path.is_dir():
            sync_tree(path, target / path.name, transformed)
        else:
            write_changed(target / path.name, (transformed or {}).get(path, path.read_bytes()))
def build_app(name, output, profile, cargo, binaries, env, overlays):
    log(["compile", name])
    if env.get("TRUEOS_TEST_FAIL_APP") == name:
        raise ValueError("sentinel compilation failed for " + name)
    payload = b"TRBP" + (ROOT / name / "src/main.rs").read_bytes()
    if name in ("edit", "hull"):
        manifest = ROOT / name / "Cargo.toml"
        relative = tomllib.loads(manifest.read_text())["dependencies"]["trueos-ide"]["path"]
        destination = dependency_path(relative, manifest, output / "sources" / name / "Cargo.toml", ROOT / name)
        log(["dependency", name, str(destination)])
        if name == "edit":
            # Simulate the actual API-reference build.rs side effect. Routing
            # to the original crate would mutate the fixture's tracked files.
            generator = Path(env["TRUEOS_API_SOURCE_ROOT"]) / "tools/build_api_prm.py"
            subprocess.run([sys.executable, str(generator), "--output", str(destination / "api-prm")], env=env, check=True)
            payload += (destination / "api-prm/reference.prm").read_bytes()
    write_changed(output / "dist" / (name + ".bp"), payload)
    imports = env.get("TRUEOS_TEST_IMPORTS", "trueos_alloc sys_yield __assert_fail atanh lrint modf").split()
    write_changed(output / "objects" / name / "module.stripped.o", ("\n".join(" U " + name for name in imports) + "\n").encode())
def verify_abi(kernel, output, names):
    log(["abi", names])
def prune_obsolete_packages(output, names):
    for path in (output / "dist").glob("*.bp"):
        if path.stem not in names:
            path.unlink()
def write_changed(path, data, *, durable=False):
    path.parent.mkdir(parents=True, exist_ok=True)
    if not path.exists() or path.read_bytes() != data:
        path.write_bytes(data)
def lz4_frame(archive):
    return b"fixture-lz4:" + archive
def checkpoint(output, name, phase):
    pass
def run(args, **kwargs):
    manifest, packages, destination, updated, bundle_manifest = args[args.index("--") + 1:]
    assert json.loads(bundle_manifest.read_text())["version"] == 1
    names = json.loads(manifest.read_text())["buildins"]
    log(["seed", str(manifest), names])
    records = {name: (packages / (name + ".bp")).read_bytes().hex() for name in names}
    destination.write_text(json.dumps(records, sort_keys=True))
'''


FAKE_REFERENCE = r'''
import argparse, hashlib, json, os, sys
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser()
parser.add_argument("--output", type=Path, required=True)
parser.add_argument("--check", action="store_true")
args = parser.parse_args()
sources = {name: hashlib.sha256((ROOT / name).read_bytes()).hexdigest()
           for name in ("api-input.rs", "tools/build_api_prm.py", "Cargo.toml")}
payload = b"Generated API: " + (ROOT / "api-input.rs").read_bytes()
manifest = {"source": {"files": sources}, "files": {"reference.prm": {
    "sha256": hashlib.sha256(payload).hexdigest(), "bytes": len(payload)}}}
try:
    current = (json.loads((args.output / "manifest.json").read_text()) == manifest
               and (args.output / "reference.prm").read_bytes() == payload)
except (OSError, ValueError):
    current = False
if current:
    sys.exit(0)
if args.check:
    sys.exit("API reference is stale")
args.output.mkdir(parents=True, exist_ok=True)
(args.output / "reference.prm").write_bytes(payload)
(args.output / "manifest.json").write_text(json.dumps(manifest, sort_keys=True))
with open(os.environ["TRUEOS_TEST_CALLS"], "a") as stream:
    stream.write(json.dumps(["api-reference", str(args.output)]) + "\n")
'''


class SelectedBuildinsTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="trueos-selected-buildins-")
        self.addCleanup(temporary.cleanup)
        self.directory = Path(temporary.name)
        self.source = self.directory / "TRUEOS-Buildins"
        self.kernel = self.directory / "TRUEOS"
        self.output = self.kernel / "tgt/buildins/release"
        self.names = sorted(item["id"] for item in catalog()["buildins"])
        (self.source / "tools").mkdir(parents=True)
        self.builder = self.source / "tools/build.py"
        self.builder.write_text(FAKE_BUILDER)
        (self.source / "buildins.json").write_text(json.dumps({"buildins": self.names}))
        self.add_apps(self.names)
        helper = self.kernel / "tools/appdb-seed"
        helper.mkdir(parents=True)
        manifest = '[dependencies]\nredb = { version = "=4.4.0" }\n'
        (helper / "Cargo.toml").write_text(manifest)
        (helper / "seed.rs").write_text("fixture helper input")
        (self.kernel / "Cargo.toml").write_text(manifest)
        self.ide = self.directory / "TRUEOS-Blueprints/crates/trueos-ide"
        (self.ide / "src").mkdir(parents=True)
        (self.ide / "api-prm").mkdir()
        (self.ide / "Cargo.toml").write_text('[package]\nname="trueos-ide"\nversion="0.1.0"\n'
                                            '[dependencies]\ntrueos-redb={path="../trueos-redb",optional=true}\n[workspace]\n')
        (self.ide / "src/lib.rs").write_text("fixture IDE source")
        (self.ide / "api-prm/reference.prm").write_text("tracked old reference must stay unchanged")
        (self.ide / "api-prm/manifest.json").write_text('{"tracked":"old reference"}')
        (self.directory / "TRUEOS-Blueprints/appbundle.json").write_text('{"version":1,"bundles":{}}')
        bundle_crate = self.ide.parent / "trueos-appbundle"
        bundle_crate.mkdir()
        (bundle_crate / "lib.rs").write_text("fixture bundle metadata parser")
        redb = self.ide.parent / "trueos-redb"
        redb.mkdir()
        (redb / "Cargo.toml").write_text('[package]\nname="trueos-redb"\nversion="0.1.0"\n')
        for name in ("edit", "hull"):
            with (self.source / name / "Cargo.toml").open("a") as manifest:
                manifest.write('[dependencies]\ntrueos-ide={path="../../TRUEOS-Blueprints/crates/trueos-ide"}\n')
        (self.kernel / "tools/build_api_prm.py").write_text(FAKE_REFERENCE)
        (self.kernel / "api-input.rs").write_text("fixture original API")
        (self.kernel / "src").mkdir()
        (self.kernel / "src/unix_compat.rs").write_text(
            'pub(crate) fn is_unix_import(name: &str) -> bool { matches!(name, "malloc" | "close" | "memchr") }\n'
            'pub(crate) fn resolve_import(name: &str) -> Option<usize> { None }\n')
        bindir = self.directory / "bin"
        bindir.mkdir()
        nm = bindir / "nm"
        nm.write_text("#!/usr/bin/env python3\nimport pathlib, sys\n"
                      "assert sys.argv[1] == '-u'\nprint(pathlib.Path(sys.argv[2]).read_text())\n")
        nm.chmod(0o755)
        self.calls = self.directory / "calls.jsonl"
        self.environment = dict(os.environ, TRUEOS_TEST_CALLS=str(self.calls),
                                PATH=str(bindir) + os.pathsep + os.environ["PATH"])
        for key in ("TRUEOS_TEST_FAIL_APP", "TRUEOS_TEST_STD_APP", "TRUEOS_TEST_IMPORTS"):
            self.environment.pop(key, None)

    def add_apps(self, names):
        for name in names:
            app = self.source / name
            (app / "src").mkdir(parents=True, exist_ok=True)
            (app / "Cargo.toml").write_text(f'[package]\nname="{name}"\nversion="0.1.0"\n')
            (app / "src/main.rs").write_text(f"// source of {name}\nfn main() {{}}\n")

    def run_adapter(self, selection, *, check=False, fail=None, native_only=False):
        environment = self.environment.copy()
        if fail is not None:
            environment["TRUEOS_TEST_FAIL_APP"] = fail
        return subprocess.run(
            [sys.executable, buildins.__file__, "--source", str(self.source), "--kernel", str(self.kernel),
             "--output", str(self.output), "--profile", "release", "--selection", selection,
             *(["--check"] if check else []), *(["--native-only"] if native_only else [])],
            env=environment, capture_output=True, text=True, timeout=15,
        )

    def successful_build(self, selection):
        result = self.run_adapter(selection)
        self.assertEqual(result.returncode, 0, result.stderr)
        return result

    def recorded_calls(self):
        return [json.loads(line) for line in self.calls.read_text().splitlines()] if self.calls.exists() else []

    def test_only_selected_apps_are_compiled_checked_and_seeded(self):
        # The unselected editor is not even a valid crate. Reading its manifest
        # for compatibility or external dependencies would fail this build.
        (self.source / "edit/Cargo.toml").write_text("deliberately invalid TOML {{")
        self.successful_build("commander,apps")
        calls = self.recorded_calls()
        self.assertEqual([call[1] for call in calls if call[0] == "compile"], ["apps", "commander"])
        self.assertEqual([call[1] for call in calls if call[0] == "abi"], [["apps", "commander"]])
        seeds = [call for call in calls if call[0] == "seed"]
        self.assertEqual(seeds, [["seed", str(self.output / "buildins.json"), ["apps", "commander"]]])
        self.assertEqual(set(json.loads((self.output / "app.db").read_text())), {"apps", "commander"})
        self.assertEqual({path.stem for path in (self.output / "dist").glob("*.bp")}, {"apps", "commander"})
        result = self.run_adapter("apps,commander", check=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn(str(self.source / "edit"), result.stdout)
        self.assertIn(str(self.source / "commander"), result.stdout)
        self.assertIn(str(Path(buildins.__file__).resolve()), result.stdout)

    def test_omitted_source_changes_do_not_invalidate_selection_but_selected_changes_do(self):
        self.successful_build("commander")
        (self.source / "edit/Cargo.toml").write_text("invalid unselected manifest")
        result = self.run_adapter("commander", check=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        (self.source / "commander/src/main.rs").write_text("changed selected source")
        result = self.run_adapter("commander", check=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("inputs are stale", result.stderr)

    def test_changing_all_to_subset_replaces_database_and_prunes_stale_packages(self):
        self.successful_build("all")
        old_database = (self.output / "app.db").read_bytes()
        self.successful_build("apps")
        self.assertNotEqual((self.output / "app.db").read_bytes(), old_database)
        self.assertEqual(json.loads((self.output / "buildins.json").read_text()), {"buildins": ["apps"]})
        self.assertEqual(set(json.loads((self.output / "app.db").read_text())), {"apps"})
        self.assertEqual({path.stem for path in (self.output / "dist").glob("*.bp")}, {"apps"})
        self.assertNotEqual(self.run_adapter("all", check=True).returncode, 0)
        self.successful_build("all")
        self.assertEqual(set(json.loads((self.output / "app.db").read_text())), set(self.names))

    def test_failed_compile_never_marks_an_old_payload_ready(self):
        self.successful_build("all")
        result = self.run_adapter("commander", fail="commander")
        self.assertNotEqual(result.returncode, 0)
        self.assertTrue((self.output / ".building").exists())
        for selection in ("all", "commander"):
            self.assertNotEqual(self.run_adapter(selection, check=True).returncode, 0)
        self.successful_build("commander")
        self.assertFalse((self.output / ".building").exists())
        self.assertEqual(self.run_adapter("commander", check=True).returncode, 0)

    def test_preflight_source_failure_cannot_reuse_the_previous_selected_payload(self):
        self.successful_build("commander")
        # settings fails before .building is created, so source freshness must
        # independently reject the old successful readiness record.
        (self.source / "commander/src/main.rs").unlink()
        result = self.run_adapter("commander")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.output / ".building").exists())
        self.assertTrue((self.output / "ready.json").exists())
        result = self.run_adapter("commander", check=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("inputs are stale", result.stderr)

    def test_check_rejects_schema_selection_manifest_package_database_and_extra_payload_drift(self):
        self.successful_build("apps,commander")
        files = ["ready.json", "buildins.json", "dist/apps.bp", "app.db", "app.db.lz4"]
        originals = {name: (self.output / name).read_bytes() for name in files}
        ready = json.loads(originals["ready.json"])
        mutations = {
            "old schema": ("ready.json", json.dumps({**ready, "schema": "old"}).encode()),
            "wrong selection": ("ready.json", json.dumps({**ready, "buildins": ["apps"]}).encode()),
            "extra ready package": ("ready.json", json.dumps({**ready, "packages": {**ready["packages"], "edit": "0" * 64}}).encode()),
            "wrong manifest": ("buildins.json", b'{"buildins":["apps"]}'),
            "changed package": ("dist/apps.bp", b"changed package"),
            "changed raw database": ("app.db", b"changed raw database"),
            "changed compressed database": ("app.db.lz4", b"changed compressed database"),
        }
        for label, (name, value) in mutations.items():
            with self.subTest(label=label):
                for original, data in originals.items():
                    (self.output / original).write_bytes(data)
                (self.output / name).write_bytes(value)
                self.assertNotEqual(self.run_adapter("apps,commander", check=True).returncode, 0)
        for original, data in originals.items():
            (self.output / original).write_bytes(data)
        (self.output / "dist/edit.bp").write_bytes(b"excluded stale package")
        self.assertNotEqual(self.run_adapter("apps,commander", check=True).returncode, 0)

    def test_selected_external_dependency_is_tracked_recursively(self):
        external = self.directory / "external"
        nested = self.directory / "nested"
        external.mkdir()
        nested.mkdir()
        (self.source / "commander/Cargo.toml").write_text('[dependencies]\nexternal={path="../../external"}\n')
        (external / "Cargo.toml").write_text('[dependencies]\nnested={path="../nested"}\n')
        (nested / "Cargo.toml").write_text('[package]\nname="nested"\n')
        (nested / "lib.rs").write_text("initial shared code")
        self.successful_build("commander")
        result = self.run_adapter("commander", check=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(str(external), result.stdout)
        self.assertIn(str(nested), result.stdout)
        (nested / "lib.rs").write_text("changed shared code")
        self.assertNotEqual(self.run_adapter("commander", check=True).returncode, 0)

    def test_external_kernel_vendor_dependency_is_not_skipped(self):
        sunset = self.kernel / "vendor/sunset"
        sunset.mkdir(parents=True)
        (sunset / "Cargo.toml").write_text('[package]\nname="sunset"\nversion="0.1.0"\n')
        (sunset / "lib.rs").write_text("initial shared SSH implementation")
        with (self.source / "ssh/Cargo.toml").open("a") as stream:
            stream.write('[dependencies]\nsunset={path="../../TRUEOS/vendor/sunset"}\n')
        self.successful_build("ssh")
        result = self.run_adapter("ssh", check=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(str(sunset), result.stdout)
        (sunset / "lib.rs").write_text("changed shared SSH implementation")
        self.assertNotEqual(self.run_adapter("ssh", check=True).returncode, 0)

    def test_native_validation_preserves_exact_selection_and_records_its_mode(self):
        result = self.run_adapter("ssh,commander", native_only=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        ready = json.loads((self.output / "ready.json").read_text())
        self.assertTrue(ready["native_only"])
        self.assertEqual(ready["buildins"], ["commander", "ssh"])
        self.assertEqual(set(json.loads((self.output / "app.db").read_text())), {"commander", "ssh"})
        self.assertEqual(self.run_adapter("commander,ssh", check=True, native_only=True).returncode, 0)
        result = self.run_adapter("commander,ssh", check=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("native-only validation mode", result.stderr)
        self.assertIsNotNone(buildins.preparation_record(self.output, "release", ["commander", "ssh"]))
        self.successful_build("commander,ssh")
        self.assertNotEqual(self.run_adapter("commander,ssh", check=True, native_only=True).returncode, 0)

    def test_native_validation_rejects_unaudited_or_std_apps_without_filtering(self):
        self.builder.write_text(FAKE_BUILDER + '\nNATIVE_CORE_ALLOC_BUILTINS.remove("ssh")\n')
        result = self.run_adapter("commander,ssh", native_only=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("ssh: selected buildin is not in the audited native set", result.stderr)
        self.assertFalse(any(call[0] == "compile" for call in self.recorded_calls()))
        self.assertFalse((self.output / "buildins.json").exists())
        self.builder.write_text(FAKE_BUILDER)
        self.environment["TRUEOS_TEST_STD_APP"] = "ssh"
        result = self.run_adapter("commander,ssh", native_only=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("ssh: native-only selection now requires std", result.stderr)
        self.assertFalse(any(call[0] == "compile" for call in self.recorded_calls()))

    def test_native_import_audit_prevents_database_and_ready_publication(self):
        # memchr remains a freestanding resolver but is still in upstream's
        # strict native-only classifier; this must reproduce that audit too.
        for forbidden in ("malloc", "close", "memchr"):
            with self.subTest(forbidden=forbidden):
                self.environment["TRUEOS_TEST_IMPORTS"] = "trueos_alloc " + forbidden
                result = self.run_adapter("ssh", native_only=True)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("native-only image imports Unix compatibility", result.stderr)
                self.assertFalse((self.output / "ready.json").exists())
                self.assertFalse((self.output / "app.db").exists())
                self.assertTrue((self.output / ".building").exists())
        self.assertFalse(any(call[0] == "seed" for call in self.recorded_calls()))

    def test_native_classifier_change_fails_closed(self):
        (self.kernel / "src/unix_compat.rs").write_text("renamed classifier")
        result = self.run_adapter("ssh", native_only=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("native Unix import classifier changed", result.stderr)
        self.assertFalse((self.output / "ready.json").exists())

    def test_source_catalog_or_builder_contract_changes_fail_before_compilation(self):
        for selection in ("", "none", "apps,apps", "../escape", "invented"):
            with self.subTest(selection=selection):
                result = self.run_adapter(selection)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(self.recorded_calls(), [])
        (self.source / "buildins.json").write_text(json.dumps({"buildins": [*self.names, "invented"]}))
        result = self.run_adapter("apps")
        self.assertIn("differs from the selector catalog", result.stderr)
        self.assertEqual(self.recorded_calls(), [])
        (self.source / "buildins.json").write_text(json.dumps({"buildins": self.names}))
        self.builder.write_text(FAKE_BUILDER.replace("def verify_abi(kernel, output, names):", "def verify_abi(kernel, output, names, new_input):"))
        result = self.run_adapter("apps")
        self.assertIn("incompatible TRUEOS-Buildins builder API", result.stderr)
        self.assertEqual(self.recorded_calls(), [])

    def test_unchanged_database_reuses_cache_but_changed_recipe_regenerates_it(self):
        self.successful_build("commander")
        self.successful_build("commander")
        self.assertEqual(len([call for call in self.recorded_calls() if call[0] == "seed"]), 1)
        self.builder.write_text(self.builder.read_text() + "\n# packaging recipe changed\n")
        self.assertNotEqual(self.run_adapter("commander", check=True).returncode, 0)
        self.successful_build("commander")
        self.assertEqual(len([call for call in self.recorded_calls() if call[0] == "seed"]), 2)

    def test_edit_generates_private_references_and_preserves_original_dependencies(self):
        originals = {path.relative_to(self.ide): path.read_bytes() for path in self.ide.rglob("*") if path.is_file()}
        self.environment["TRUEOS_API_SOURCE_ROOT"] = "/deliberately-wrong-inherited-root"
        self.successful_build("edit,hull")
        staged = self.output / "dependencies/trueos-ide"
        current = {path.relative_to(self.ide): path.read_bytes() for path in self.ide.rglob("*") if path.is_file()}
        self.assertEqual(current, originals)
        self.assertEqual([call[2] for call in self.recorded_calls() if call[0] == "dependency"], [str(staged), str(staged)])
        import tomllib
        staged_manifest = tomllib.loads((staged / "Cargo.toml").read_text())
        self.assertEqual(staged_manifest["dependencies"]["trueos-redb"]["path"], str(self.ide.parent / "trueos-redb"))
        ready = json.loads((self.output / "ready.json").read_text())
        self.assertEqual(ready["generated"], buildins.generated_state(self.output, ["edit", "hull"]))
        record = buildins.preparation_record(self.output, "release", ["edit", "hull"])
        self.assertEqual(record["generated"], ready["generated"])
        result = self.run_adapter("edit,hull", check=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(str(self.ide), result.stdout)
        self.assertIn(str(self.kernel / "api-input.rs"), result.stdout)
        self.assertIn(str(staged), result.stdout)

    def test_edit_reference_cache_is_preserved_and_api_changes_require_regeneration(self):
        self.successful_build("edit")
        reference = self.output / "dependencies/trueos-ide/api-prm/reference.prm"
        timestamp = reference.stat().st_mtime_ns
        before = json.loads((self.output / "ready.json").read_text())["generated"]
        self.successful_build("edit")
        self.assertEqual(reference.stat().st_mtime_ns, timestamp)
        self.assertEqual(len([call for call in self.recorded_calls() if call[0] == "api-reference"]), 1)
        (self.kernel / "api-input.rs").write_text("changed API source")
        self.assertNotEqual(self.run_adapter("edit", check=True).returncode, 0)
        self.successful_build("edit")
        self.assertNotEqual(json.loads((self.output / "ready.json").read_text())["generated"], before)
        self.assertIn(b"changed API source", reference.read_bytes())
        self.assertEqual((self.ide / "api-prm/reference.prm").read_text(), "tracked old reference must stay unchanged")
        reference.write_bytes(b"tampered generated reference")
        self.assertNotEqual(self.run_adapter("edit", check=True).returncode, 0)

    def test_hull_without_edit_never_stages_or_generates_api(self):
        (self.kernel / "tools/build_api_prm.py").unlink()
        self.successful_build("hull")
        self.assertFalse((self.output / "dependencies").exists())
        self.assertEqual([call[2] for call in self.recorded_calls() if call[0] == "dependency"], [str(self.ide)])
        self.assertFalse(any(call[0] == "api-reference" for call in self.recorded_calls()))
        self.assertEqual(buildins.preparation_record(self.output, "release", ["hull"])["generated"], {})


if __name__ == "__main__":
    unittest.main()
