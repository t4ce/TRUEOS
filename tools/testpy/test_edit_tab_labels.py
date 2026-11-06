#!/usr/bin/env python3
"""Exercise the real terminal-to-Shell3 selection label handoff on the host."""
from pathlib import Path
import subprocess
import tempfile

from test_edit_life_native import function

ROOT = Path(__file__).resolve().parents[2]

FIXTURE = r'''
#![allow(dead_code)]
extern crate alloc;
use std::sync::Mutex;
use trueos_terminal::{Terminal, TerminalColor};
#[derive(Clone, Copy, Debug, PartialEq)]
enum RgbaColor {
    BlackTransparent,
    Terminal { foreground: [u8;4], background: [u8;4], underline: bool },
}
impl RgbaColor {
    fn rgba(self) -> [u8;4] { match self {
        Self::BlackTransparent => [0;4], Self::Terminal { foreground, .. } => foreground,
    }}
}
mod update {
    pub const CONTROL_BACKGROUND: [u8;4] = [16,16,16,255];
    pub const MATRIX_BACKGROUND: [u8;4] = [24,24,24,255];
}
type RenderedLine = Vec<(char, Option<RgbaColor>)>;
struct SpinMutex<T>(Mutex<T>);
impl<T> SpinMutex<T> {
    const fn new(value:T) -> Self { Self(Mutex::new(value)) }
    fn lock(&self) -> std::sync::MutexGuard<'_,T> { self.0.lock().unwrap() }
}
mod snapshot {
    use super::*;
    struct Lease(&'static str);
    impl Lease { fn name(&self) -> &str { self.0 } }
    struct Native { painted: bool }
    struct Route {
        frontend: Option<u64>, selected: bool, lease: Lease, owner: Option<()>,
        native: Option<Native>, screen: Terminal,
    }
    impl Route { fn active(&self) -> bool { true } }
    struct Registry { routes: Vec<Route>, remote_frontends: Vec<(u64,())> }
    static ROUTES: SpinMutex<Registry> = SpinMutex::new(Registry {
        routes: Vec::new(), remote_frontends: Vec::new(),
    });
    @SNAPSHOT@
    @RENDERED_CELL@
    @COLOR@

    #[test]
    fn metadata_paints_labels_without_changing_text_or_nonblank_cells() {
        let mut screen = Terminal::new(40, 1);
        screen.feed(b"Hello\x1b[48;2;44;44;44m               Hello\x1b[?25l");
        screen.feed(b"\x1b]777;edit-tabs;1;0;5;15;4;5;20\x07");
        ROUTES.lock().routes.push(Route { frontend: Some(1), selected: true,
            lease: Lease("edit"), owner: None, native: None, screen });
        let (lines, _) = snapshot_with_effect(1, Some("edit")).unwrap();
        assert_eq!(lines[0][0].0, 'H');
        for column in 5..20 {
            let cell = trueos_terminal::tab_label_cell::TabLabelCell::from_character(lines[0][column].0).unwrap();
            assert_eq!((cell.count, cell.columns, cell.column), (4,15,(column-5) as u32));
            assert_eq!(ROUTES.lock().routes[0].screen.cells()[column].glyph, ' ');
            assert_eq!(lines[0][column].1.unwrap().rgba(), [255;4]);
        }
        ROUTES.lock().routes[0].screen.feed(b"\x1b[1;10HX");
        let (lines, _) = snapshot_with_effect(1, Some("edit")).unwrap();
        assert_eq!(lines[0][9].0, 'X');
        ROUTES.lock().routes[0].screen.feed(b"\x1b[?2026h\x1b]777;edit-tabs;1;clear\x07");
        let (pending, _) = snapshot_with_effect(1, Some("edit")).unwrap();
        assert_eq!(pending, lines);
        ROUTES.lock().routes[0].screen.feed(b"\x1b[?2026l");
        let (lines, _) = snapshot_with_effect(1, Some("edit")).unwrap();
        assert_eq!(lines[0][5].0, ' ');
        assert_eq!(lines[0][9].0, 'X');
    }
}
'''


def main() -> None:
    source = (ROOT / "src/shell3/tui.rs").read_text()
    fixture = FIXTURE.replace("@SNAPSHOT@", function(source, "snapshot_with_effect"))
    fixture = fixture.replace("@RENDERED_CELL@", function(source, "rendered_cell"))
    fixture = fixture.replace("@COLOR@", function(source, "color"))
    with tempfile.TemporaryDirectory(prefix="trueos-selection-tabs-") as temporary:
        directory = Path(temporary)
        rust = ["rustc", "--edition=2024", "--target=x86_64-unknown-linux-gnu",
                "-C", f"linker={ROOT}/tools/rust-host-ld"]
        terminal = directory / "libtrueos_terminal.rlib"
        subprocess.run(rust + ["--crate-type=rlib", "--crate-name=trueos_terminal",
                       str(ROOT / "crates/trueos-terminal/src/lib.rs"), "-o", str(terminal)], check=True)
        test = directory / "snapshot.rs"
        test.write_text(fixture)
        binary = directory / "tests"
        subprocess.run(rust + ["--test", str(test), "--extern", f"trueos_terminal={terminal}",
                              "-o", str(binary)], check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
