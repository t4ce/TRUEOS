#!/usr/bin/env python3
"""Run authored avatar locomotion/action tests through the native pose bridge."""
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
VOXY = ROOT.parent / "voxy"

def main():
    with tempfile.TemporaryDirectory(prefix="voxy-animation-") as directory:
        project = Path(directory)
        (project / "src").mkdir()
        (project / "assets").symlink_to(ROOT.parent / "voxy-assets", target_is_directory=True)
        fixture = (ROOT / "tools/testpy/fixtures/voxy_world_animation.rs").read_text()
        (project / "src/lib.rs").write_text(fixture.replace("@VOXY@", str(VOXY)))
        (project / "Cargo.toml").write_text(f'''[package]
name="voxy-animation-contract-tests"
version="0.1.0"
edition="2024"
[workspace]
[dependencies]
anim={{package="veloren-voxygen-anim",path="{VOXY / 'anim'}"}}
common={{package="veloren-common",path="{VOXY / 'common'}"}}
vek={{version="0.17",features=["serde","mint"]}}
ron={{version="0.12",default-features=false,features=["std"]}}
[patch.crates-io]
shred={{path="{VOXY / 'vendor/shred'}"}}
specs={{path="{VOXY / 'vendor/specs'}"}}
hibitset={{path="{VOXY / 'vendor/hibitset'}"}}
''')
        env = dict(os.environ, VELOREN_ASSETS=str(project / "assets"),
                   CARGO_WORKSPACE_DIR=str(VOXY))
        subprocess.run(["cargo", "+nightly-2026-07-10", "test", "--manifest-path", str(project / "Cargo.toml"),
                        "--target", "x86_64-unknown-linux-gnu", "--target-dir",
                        str(ROOT / "bld/voxy-animation-host-tests")],
                       cwd=project, env=env, check=True)

if __name__ == "__main__":
    main()
