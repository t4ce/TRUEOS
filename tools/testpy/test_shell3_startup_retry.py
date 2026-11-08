#!/usr/bin/env python3
"""Exercise production Shell3 presentation and startup dispatch at UI boundaries."""

import json
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def method(source: str, signature: str) -> str:
    start = source.index(signature)
    opening = source.index("{", start)
    depth = 1
    cursor = opening + 1
    while depth:
        depth += (source[cursor] == "{") - (source[cursor] == "}")
        cursor += 1
    return source[start:cursor]


def main() -> None:
    shell = (ROOT / "src/shell3/shell3.rs").read_text()
    service = (ROOT / "src/shell3/service.rs").read_text()
    worker = service
    worker = worker[worker.index("async fn shell_worker_task("):]
    # The admitted request must reach the owner before its first presentation.
    assert worker.index("startup.configure_window(shell);") < worker.index("shell.pending_startup = startup;")
    assert worker.index("Startup::guest_account") < worker.index("shell.inherit_guest_account(")
    assert worker.index("window.as_ref().map(|window| window.access_account)") < worker.index("shell.inherit_guest_account(")
    assert worker.index("shell.inherit_guest_account(") < worker.index("shell.pending_startup = startup;")
    assert worker.index("shell.pending_startup = startup;") < worker.index("shell.present().await")
    assert "startup.launch(shell)" not in worker
    assert "shell.presentation_pending()" in worker
    assert "Timer::after(Duration::from_millis(10)).await" in worker
    presentation = method(shell, "pub async fn present(&mut self)")
    assert presentation.index("tui::bind_ui4_window(") < presentation.index("self.reconcile_local_account();")
    assert presentation.index("self.reconcile_local_account();") < presentation.index("startup.launch(self);")
    assert presentation.index("startup.launch(self);") < presentation.rindex("self.reconcile_local_account();")
    reconcile = method(shell, "fn reconcile_local_account(&mut self)")
    challenge = reconcile[reconcile.index("local_account::Action::Challenge"):]
    assert challenge.index("if self.pending_startup.is_some()") < challenge.index("admin::start_login(")
    window_spawn = method(service, "pub(crate) fn blueprint_window_spawn(")
    assert "access_account: crate::access_key_db::principal_for_vm(vm_id)" in window_spawn
    command = method(shell, "fn echo_recognized_prompt(&mut self)")
    sh3 = command[command.index('self.mode == Mode::ADM && text == "sh3"'):
                  command.index('self.mode == Mode::HV && text == "status"')]
    assert "match tui::guest_frontend_account(self.tui_frontend())" in sh3
    assert "startup::Startup::GuestShell { access_account }" in sh3
    assert "None => service::request_shell3()" in sh3
    methods = "\n".join(method(shell, signature) for signature in (
        "pub async fn present(&mut self)",
        "pub(super) fn presentation_pending(&self)",
        "pub(super) fn show_is_closed(&self)",
        "pub(super) fn inherit_guest_account(",
    ))
    source = (Path(__file__).with_name("shell3_sessions") / "startup_retry.rs").read_text()
    source = source.replace("@PRESENTATION_METHODS@", methods)
    source = source.replace("@TRUEOS_ROOT@", json.dumps(str(ROOT))[1:-1])
    for name in ("DEFAULT_COLUMNS", "DEFAULT_ROWS"):
        value = re.search(rf'pub const {name}: usize = (\d+);', shell)
        assert value, name
        source = source.replace("@" + name + "@", value.group(1))
    desktop = (ROOT / "src/ui4/link_service.rs").read_text()
    attribute = "#[trueos_executor::task(pool_size = 1)]"
    assert desktop.count(attribute) == 1
    desktop = desktop.replace(attribute, "")
    channel = re.search(r'^channel\s*=\s*"([^"]+)"',
                        (ROOT / "rust-toolchain.toml").read_text(), re.MULTILINE)
    assert channel
    with tempfile.TemporaryDirectory(prefix="trueos-startup-retry-") as directory:
        code, binary = Path(directory) / "harness.rs", Path(directory) / "tests"
        adapter = Path(directory) / "link_service.rs"
        adapter.write_text(desktop)
        source = source.replace("@LINK_SERVICE_PATH@", json.dumps(str(adapter))[1:-1])
        code.write_text(source)
        subprocess.run(["rustc", "+" + channel.group(1), "--edition=2024", "--test",
                        "--target", "x86_64-unknown-linux-gnu", str(code), "-o", str(binary)],
                       cwd=ROOT, check=True)
        subprocess.run([str(binary), "--test-threads=1"], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
