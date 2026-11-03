#!/usr/bin/env python3
"""Run the real Codex IDE client's transport tests without its full workspace.

Only the test crate manifest is substituted. The client and shared wire protocol
are compiled from their authored source files, including HTML open validation.
"""
import json
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    client = ROOT.parent / "codex/codex-rs/core/src/trueos_ide.rs"
    protocol = ROOT.parent / "TRUEOS-Blueprints/crates/trueos-ide"
    with tempfile.TemporaryDirectory(prefix="trueos-codex-ide-client-") as temporary:
        directory = Path(temporary)
        (directory / "src").mkdir()
        (directory / "Cargo.toml").write_text(
            '[package]\nname="trueos-codex-ide-client-tests"\nversion="0.0.0"\nedition="2024"\n'
            '[dependencies]\nserde={version="1",features=["derive"]}\nserde_json="1"\n'
            'tokio={version="=1.54.1",features=["rt","time","macros","sync"]}\n'
            f'trueos-ide={{path={json.dumps(str(protocol))},default-features=false}}\n[workspace]\n'
        )
        (directory / "src/lib.rs").write_text(
            '#![allow(dead_code,unexpected_cfgs)]\n'
            f'#[path={json.dumps(str(client))}]\npub mod actual;\n'
        )
        env = os.environ.copy()
        # This is a host test, independent of the kernel Cargo configuration.
        for name in ["CARGO_BUILD_TARGET", "CARGO_ENCODED_RUSTFLAGS", "RUSTFLAGS"]:
            env.pop(name, None)
        subprocess.run(["cargo", "test", "--offline"], cwd=directory, env=env, check=True)


if __name__ == "__main__":
    main()
