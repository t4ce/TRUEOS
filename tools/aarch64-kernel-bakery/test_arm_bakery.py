#!/usr/bin/env python3

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from artifact_contract import (
    BACKEND,
    ContractError,
    ELF_MACHINE_AARCH64,
    analyze_object,
    verify_manifest,
)


TOOL_DIR = Path(__file__).resolve().parent
REPO_ROOT = TOOL_DIR.parent.parent
SOURCE = REPO_ROOT / "crates/trueos-shader/cpu/kernels/copy_rect_rgba8.cpp"
PROFILE = TOOL_DIR / "profiles/aarch64-none-elf.json"
ENTRY = "trueos_arm_copy_rect_rgba8"


def _find_tool(environment_name: str, default: str) -> str | None:
    requested = os.environ.get(environment_name)
    if requested:
        if Path(requested).parent != Path("."):
            return requested if Path(requested).is_file() else None
        return shutil.which(requested)
    return shutil.which(default)


class Aarch64BakeryTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.clang = _find_tool("ARM_CLANG", "clang")
        if cls.clang is None:
            raise unittest.SkipTest("Clang is required for AArch64 bakery tests")
        cls.temporary = tempfile.TemporaryDirectory()
        cls.root = Path(cls.temporary.name)
        cls.artifact_dir = cls.root / "published"
        command = [
            sys.executable,
            "-B",
            str(TOOL_DIR / "bake.py"),
            "--source",
            str(SOURCE),
            "--artifact-name",
            "copy_rect_rgba8",
            "--profile",
            str(PROFILE),
            "--expect-entry",
            ENTRY,
            "--build-root",
            str(cls.root / "build"),
            "--publish-dir",
            str(cls.artifact_dir),
            "--clang",
            cls.clang,
            "--repro-check",
        ]
        process = subprocess.run(
            command,
            cwd=REPO_ROOT,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            check=False,
        )
        if process.returncode != 0:
            raise AssertionError(process.stdout)

    @classmethod
    def tearDownClass(cls) -> None:
        if hasattr(cls, "temporary"):
            cls.temporary.cleanup()

    def test_emits_reproducible_freestanding_aarch64_object(self) -> None:
        object_path = self.artifact_dir / "copy_rect_rgba8.o"
        analysis = analyze_object(
            object_path,
            expected_machine=ELF_MACHINE_AARCH64,
            expected_entries=[ENTRY],
        )
        self.assertEqual(analysis["elf_machine"], ELF_MACHINE_AARCH64)
        self.assertEqual(analysis["undefined_symbols"], [])
        self.assertEqual(
            [entry["name"] for entry in analysis["entries"]],
            [ENTRY],
        )
        self.assertGreater(analysis["entries"][0]["entry_size"], 0)

    def test_compiler_free_manifest_verification(self) -> None:
        manifest = verify_manifest(
            self.artifact_dir / "copy_rect_rgba8.manifest.json",
            self.artifact_dir / "copy_rect_rgba8.o",
            repo_root=REPO_ROOT,
        )
        self.assertEqual(manifest["backend"], BACKEND)
        self.assertTrue(manifest["reproducibility"]["object_identical"])
        self.assertEqual(
            [record["path"] for record in manifest["inputs"]],
            [
                "crates/trueos-shader/cpu/kernels/copy_rect_rgba8.cpp",
                "crates/trueos-shader/cpu/kernels/include/trueos_arm_kernels.h",
            ],
        )

    def test_rejects_non_aarch64_elf_machine(self) -> None:
        original = self.artifact_dir / "copy_rect_rgba8.o"
        changed = self.root / "wrong-machine.o"
        data = bytearray(original.read_bytes())
        data[18:20] = (62).to_bytes(2, "little")
        changed.write_bytes(data)
        with self.assertRaisesRegex(ContractError, "expected ELF machine 183"):
            analyze_object(
                changed,
                expected_machine=ELF_MACHINE_AARCH64,
                expected_entries=[ENTRY],
            )

    def test_manifest_detects_object_change(self) -> None:
        manifest_path = self.artifact_dir / "copy_rect_rgba8.manifest.json"
        object_path = self.artifact_dir / "copy_rect_rgba8.o"
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        manifest["artifact"]["sha256"] = "0" * 64
        changed_manifest = self.root / "changed.manifest.json"
        changed_manifest.write_text(json.dumps(manifest), encoding="utf-8")
        with self.assertRaisesRegex(ContractError, "object hash changed"):
            verify_manifest(changed_manifest, object_path, repo_root=REPO_ROOT)


class CopyRectBehaviorTests(unittest.TestCase):
    def test_odd_width_origins_pitches_and_guards(self) -> None:
        clangxx = _find_tool("HOST_CLANGXX", "clang++")
        if clangxx is None:
            self.skipTest("host clang++ is required for the semantic test")
        with tempfile.TemporaryDirectory() as directory:
            executable = Path(directory) / "copy-rect-test"
            command = [
                clangxx,
                "-std=c++20",
                "-O2",
                "-Wall",
                "-Wextra",
                "-Werror",
                str(SOURCE),
                str(TOOL_DIR / "test_copy_rect_native.cpp"),
                "-o",
                str(executable),
            ]
            subprocess.run(command, cwd=REPO_ROOT, check=True)
            process = subprocess.run([str(executable)], check=False)
            self.assertEqual(process.returncode, 0)


if __name__ == "__main__":
    unittest.main()
