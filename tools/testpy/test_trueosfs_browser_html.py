#!/usr/bin/env python3
"""Exercise the production HTTP tree renderer without booting the kernel."""
from pathlib import Path
import subprocess
import tempfile
from test_clip_position3_uv_texture import ROOT, item


def main():
    source = "src/r/fs/fs_html.rs"
    harness = "extern crate alloc;\nextern crate self as trueos_math;\n"
    harness += "use alloc::{string::String, vec::Vec};\n"
    for module in ("ascii_tree", "html_tree", "tree"):
        harness += f'#[path = "{ROOT}/crates/trueos-math/src/{module}.rs"] mod {module};\n'
    harness += "pub use tree::{Tree, NodeId};\n"
    for name in ("FsKind", "FsEntry", "escaped", "browser_tree", "browser_tree_tests"):
        harness += item(source, name)
    with tempfile.TemporaryDirectory(prefix="trueos-fs-html-") as directory:
        root = Path(directory)
        (root / "test.rs").write_text(harness)
        subprocess.run(["rustc", "--edition=2024", "--test", "-A", "dead_code",
                        str(root / "test.rs"), "-o", str(root / "test")], check=True)
        subprocess.run([str(root / "test"), "browser_tree_tests"], check=True)


if __name__ == "__main__":
    main()
