import hashlib
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

import package_blueprint as packaging


class ContractCommentTests(unittest.TestCase):
    def test_relocated_comments_pass_but_abi_changes_fail(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            blueprints = root / "blueprints"
            asset = blueprints / "apps/shadertoy/assets/fixture"
            asset.mkdir(parents=True)
            kernel = root / "crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp"
            kernel.mkdir(parents=True)
            (root / "src/intel/gpgpu/artifacts").mkdir(parents=True)
            source = b"kernel source"
            contract = b"// source: /checked-in/kernel.clcpp\n// normalized commands: old\nconst ABI: u32 = 1;\n"
            (kernel / "shadertoy_fixture.contract.rs").write_bytes(contract)
            for suffix, data in [("bin", b"binary"), ("spv", b"spirv"), ("clcpp", source), ("contract.rs", contract)]:
                (asset / f"kernel.{suffix}").write_bytes(data)
            (asset / "input.glsl").write_bytes(b"glsl")
            (asset / "kernel.manifest.json").write_text(json.dumps({"source": {
                "path": "kernel.clcpp", "inputs": [{"path": "kernel.clcpp", "sha256": hashlib.sha256(source).hexdigest()}]
            }}))
            with mock.patch.object(packaging, "ROOT", root), mock.patch.object(packaging, "PROGRAMS", ((1, "fixture"),)):
                def run(mode):
                    with mock.patch.object(sys, "argv", ["package_blueprint.py", "--blueprints-root", str(blueprints), mode]):
                        packaging.main()
                run("--update-trust")
                moved = b"// source: /runner/kernel.clcpp\n// normalized commands: new\nconst ABI: u32 = 1;\n"
                (asset / "kernel.contract.rs").write_bytes(moved)
                # Reproduce the release ordering: regenerate the Blueprint
                # package, restore kernel comments, then verify it again.
                run("--update-trust")
                run("--check")
                (asset / "kernel.contract.rs").write_bytes(moved.replace(b"= 1", b"= 2"))
                with self.assertRaisesRegex(SystemExit, "Blueprint contract differs"):
                    run("--check")


if __name__ == "__main__":
    unittest.main()
