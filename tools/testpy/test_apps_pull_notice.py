#!/usr/bin/env python3
"""Exercise the real Apps completion observer without VM/display hardware."""
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    service = (ROOT / "src/shell3/service.rs").read_text()
    start = service.index("struct AppsLaunch;")
    end = service.index("// OS update completion", start)
    observer = service[start:end]
    source = r'''
extern crate alloc;
use std::cell::RefCell;
#[derive(Default)]
struct State { notices: Vec<String>, discarded: usize }
thread_local! { static STATE: RefCell<State> = RefCell::default(); }
mod hv {
    pub mod launcher {
        pub trait LaunchObserver {
            fn finished(&self, target: &crate::shell3::MatrixTarget, vm: Option<u8>, error: Option<&str>, reason: Option<&str>);
        }
    }
    pub mod matrix {
        pub fn discard(_: &crate::shell3::MatrixTarget) {
            crate::STATE.with_borrow_mut(|s| s.discarded += 1);
        }
    }
}
mod shell3 {
    pub struct MatrixTarget;
    pub struct MatrixSlots;
    impl MatrixSlots {
        pub fn echo(slot: Option<&str>, lifetime: Option<u64>, message: String) {
            assert_eq!((slot,lifetime),(None,None), "notice must survive in the default slot");
            crate::STATE.with_borrow_mut(|s| s.notices.push(message));
        }
    }
    mod service {
        @OBSERVER@
        #[cfg(test)]
        mod tests {
            use super::*;
            use crate::hv::launcher::LaunchObserver;
            #[test]
            fn info_survives_in_default_slot_and_helper_slot_is_retired() {
                AppsLaunch.finished(&super::super::MatrixTarget,Some(4),None,Some("apps:info:CDN maintenance"));
                crate::STATE.with_borrow(|s| {
                    assert_eq!(s.notices,["CDN maintenance"]);
                    assert_eq!(s.discarded,1);
                });
            }
            #[test]
            fn normal_completion_and_unrelated_exit_reasons_are_quiet() {
                for reason in [None,Some("apps pullbot drained"),Some("os:quit")] {
                    AppsLaunch.finished(&super::super::MatrixTarget,Some(4),None,reason);
                }
                crate::STATE.with_borrow(|s| { assert!(s.notices.is_empty()); assert_eq!(s.discarded,3); });
            }
            #[test]
            fn admission_failure_uses_existing_error_flow_and_still_cleans_up() {
                AppsLaunch.finished(&super::super::MatrixTarget,None,Some("VM unavailable"),Some("apps:info:stale"));
                crate::STATE.with_borrow(|s| { assert!(s.notices.is_empty()); assert_eq!(s.discarded,1); });
            }
        }
    }
}
'''.replace("@OBSERVER@", observer)
    channel = re.search(r'^channel\s*=\s*"([^"]+)"', (ROOT / "rust-toolchain.toml").read_text(), re.MULTILINE).group(1)
    with tempfile.TemporaryDirectory(prefix="trueos-apps-notice-") as directory:
        code, binary = Path(directory) / "test.rs", Path(directory) / "tests"
        code.write_text(source)
        subprocess.run(["rustc", "+" + channel, "--edition=2024", "--test", "--target", "x86_64-unknown-linux-gnu", str(code), "-o", str(binary)], check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
