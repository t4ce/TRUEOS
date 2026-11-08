#!/usr/bin/env python3
"""Stage pinned fs-err and exercise ordinary-host filesystem/error behavior.

Native opaque-file/metadata signatures are checked by the real XERJ native
build, not simulated with host descriptor aliases in this fixture.
"""
import argparse
import json
from pathlib import Path
import subprocess
import tempfile

from test_native_mio import ROOT, TOOLCHAIN, environment, info, manifest
from test_native_rama_tcp import STAGE_DRIVER

SDK = ROOT.parent / "TRUEOS-Blueprints"
TESTS = r'''
#[cfg(test)] mod tests {
 use std::{io::{Read,Seek,SeekFrom},path::PathBuf};
 fn root(label:&str)->PathBuf {let p=std::env::temp_dir().join(format!("fs-err-native-provider-{}-{label}",std::process::id()));let _=std::fs::remove_dir_all(&p);std::fs::create_dir(&p).unwrap();p}
 #[test] fn synchronous_context_keeps_kind_path_and_real_bytes(){let p=root("sync");let path=p.join("native-source");fs_err::write(&path,b"real filesystem").unwrap();assert_eq!(fs_err::read(&path).unwrap(),b"real filesystem");let absent=p.join("missing-source");let e=fs_err::read(&absent).unwrap_err();assert_eq!(e.kind(),std::io::ErrorKind::NotFound);assert!(e.to_string().contains("missing-source"));std::fs::remove_dir_all(p).unwrap();}
 #[test] fn asynchronous_context_metadata_and_directory_entries_remain_real(){let p=root("async");tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {let path=p.join("native-source");fs_err::tokio::write(&path,b"async bytes").await.unwrap();assert_eq!(fs_err::tokio::read(&path).await.unwrap(),b"async bytes");assert_eq!(fs_err::tokio::metadata(&path).await.unwrap().len(),11);let mut entries=fs_err::tokio::read_dir(&p).await.unwrap();let entry=entries.next_entry().await.unwrap().unwrap();assert_eq!(entry.path(),path);assert!(entry.file_type().await.unwrap().is_file());assert_eq!(entry.metadata().await.unwrap().len(),11);assert!(entries.next_entry().await.unwrap().is_none());let e=fs_err::tokio::read(p.join("missing-source")).await.unwrap_err();assert_eq!(e.kind(),std::io::ErrorKind::NotFound);assert!(e.to_string().contains("missing-source"));});std::fs::remove_dir_all(p).unwrap();}
 #[test] fn host_std_conversions_retain_owned_file_and_shared_cursor(){let p=root("conversion");let path=p.join("native-source");std::fs::write(&path,b"owned handle").unwrap();tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {let f=fs_err::File::open(&path).unwrap();let mut native=fs_err::tokio::File::from_std(f).into_std().await;let mut bytes=Vec::new();native.read_to_end(&mut bytes).unwrap();assert_eq!(bytes,b"owned handle");native.seek(SeekFrom::Start(6)).unwrap();let async_again:fs_err::tokio::File=native.into();let mut native=async_again.try_into_std().unwrap();let mut bytes=Vec::new();native.read_to_end(&mut bytes).unwrap();assert_eq!(bytes,b"handle");let mut std_options=std::fs::OpenOptions::new();std_options.read(true);let options:fs_err::tokio::OpenOptions=std_options.into();assert_eq!(options.open(&path).await.unwrap().metadata().await.unwrap().len(),12);});std::fs::remove_dir_all(p).unwrap();}
}
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    out = parser.parse_args().output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    checks = []
    with tempfile.TemporaryDirectory(prefix="xerj-fs-err-") as temporary:
        work = Path(temporary)

        def run(label, command, env=None):
            log = out / (label + ".log")
            with log.open("w") as stream:
                result = subprocess.run(command, cwd=work, env=env, stdout=stream,
                                        stderr=subprocess.STDOUT, timeout=240)
            checks.append({"label": label, "exit": result.returncode, "log": str(log)})
            if result.returncode:
                raise RuntimeError(log.read_text()[-12000:])
            return log.read_text()

        module = SDK / "src/xerj_fs_err_overlay.rs"
        (work / "stage.rs").write_text(STAGE_DRIVER.replace("@MODULE@", json.dumps(str(module))))
        run("compile", ["rustup", "run", TOOLCHAIN, "rustc", "--edition=2024",
                        str(work / "stage.rs"), "-o", str(work / "stage")])
        source, = (Path.home() / ".cargo/registry/src").glob("*/fs-err-3.3.1")
        staged = Path(run("stage", [str(work / "stage"), str(work), str(source)]).strip())
        path = staged / "Cargo.toml"
        text = []
        skip = False
        for line in path.read_text().splitlines():
            if line.startswith("["):
                # The optional debug formatter is outside this I/O proof and
                # not cached locally. Trim it only in the temporary fixture.
                skip = ("dev-dependencies" in line or line == "[features]"
                        or line == "[dependencies.path_facts]")
            if not skip:
                text.append(line)
        path.write_text("\n".join(text) + "\n")
        manifest(work, f'''[package]
name="xerj-fs-err-provider-proof"
version="0.0.0"
edition="2024"
[workspace]
[dependencies]
fs-err={{path={json.dumps(str(staged))},features=["tokio"]}}
tokio={{path={json.dumps(str(ROOT.parent / 'TRUEOS-Buildins/vendor/tokio-1.54.1'))},features=["rt","fs","io-util"]}}
[patch.crates-io]
tokio={{path={json.dumps(str(ROOT.parent / 'TRUEOS-Buildins/vendor/tokio-1.54.1'))}}}
mio={{path={json.dumps(str(ROOT / 'vendor/mio-1.2.0'))}}}
[lib]
path="lib.rs"
''')
        (work / "lib.rs").write_text(TESTS)
        for label, flags in [("debug", []), ("release", ["--release"])]:
            result = run(label, ["rustup", "run", TOOLCHAIN, "cargo", "test", "--offline",
                                 *flags, "--target", "x86_64-unknown-linux-gnu", "--lib"],
                         environment(out / "target"))
            assert "3 passed" in result
    (out / "receipt.json").write_text(json.dumps({"scope": __doc__, "checks": checks,
                                                "source": info(module)}, indent=2) + "\n")
    print(out / "receipt.json")


if __name__ == "__main__":
    main()
