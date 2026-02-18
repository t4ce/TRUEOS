use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn run(cmd: &mut Command) {
    let status = cmd
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .unwrap_or_else(|e| panic!("spawn {:?} failed: {e}", cmd));
    if !status.success() {
        panic!("command {:?} failed with status {status}", cmd);
    }
}

fn read_to_string_if_exists(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

fn write_string(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dir");
    }
    fs::write(path, contents).expect("write file");
}

fn remove_dir_if_exists(path: &Path) {
    if path.exists() {
        fs::remove_dir_all(path)
            .unwrap_or_else(|e| panic!("remove {} failed: {e}", path.display()));
    }
}

fn is_file(path: &Path) -> bool {
    path.metadata().map(|m| m.is_file()).unwrap_or(false)
}

fn require_tool(tool: &str, hint: &str) {
    let ok = Command::new("sh")
        .arg("-lc")
        .arg(format!("command -v {} >/dev/null 2>&1", tool))
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        panic!("Missing required tool '{tool}'. Hint: {hint}");
    }
}

pub struct LiminePaths {
    pub src_dir: PathBuf,
    pub build_dir: PathBuf,
    pub prefix_dir: PathBuf,
}

impl LiminePaths {
    pub fn share_dir(&self) -> PathBuf {
        self.prefix_dir.join("share").join("limine")
    }

    pub fn stamp(&self) -> PathBuf {
        self.build_dir.join(".installed")
    }
}

pub fn default_paths(repo_root: &Path) -> LiminePaths {
    LiminePaths {
        src_dir: repo_root.join("bld").join("limine-src"),
        build_dir: repo_root.join("bld").join("limine-build"),
        prefix_dir: repo_root.join("bld").join("limine-prefix"),
    }
}

fn default_config_args(prefix: &Path) -> String {
    format!(
        "--prefix={} --enable-uefi-x86-64 --enable-uefi-cd",
        prefix.display()
    )
}

fn should_build(paths: &LiminePaths) -> bool {
    let share = paths.share_dir();
    // ISO build needs these.
    let need_uefi =
        is_file(&share.join("BOOTX64.EFI")) && is_file(&share.join("limine-uefi-cd.bin"));

    !need_uefi || !is_file(&paths.stamp())
}

pub fn ensure_limine(repo_root: &Path) {
    // Avoid surprising rebuilds when nothing is missing.
    let paths = default_paths(repo_root);
    if !should_build(&paths) {
        return;
    }

    // Tools we need.
    require_tool("git", "Install git");
    require_tool("make", "Install build-essential / make");
    require_tool("gcc", "Install a C compiler toolchain");

    // Limine generates configure via ./bootstrap (autoreconf).
    // Match the existing Makefile behavior: only require autoreconf when configure is missing.

    let repo = std::env::var("TRUEOS_LIMINE_REPO")
        .unwrap_or_else(|_| "https://github.com/limine-bootloader/limine.git".to_string());
    let reference = std::env::var("TRUEOS_LIMINE_REF").unwrap_or_else(|_| "v10.x".to_string());

    // If Cargo is in offline mode, don't try to hit the network.
    if std::env::var_os("CARGO_NET_OFFLINE").is_some() {
        if !paths.src_dir.exists() {
            panic!(
                "Limine sources not found at {} and CARGO_NET_OFFLINE is set. \
Set TRUEOS_LIMINE_REF/TRUEOS_LIMINE_REPO and build with network access once.",
                paths.src_dir.display()
            );
        }
    }

    if !paths.src_dir.join(".git").exists() {
        // Fresh clone into bld/ so it stays out of version control.
        remove_dir_if_exists(&paths.src_dir);
        fs::create_dir_all(paths.src_dir.parent().unwrap()).expect("create bld/");

        let mut cmd = Command::new("git");
        cmd.arg("clone")
            .arg("--depth")
            .arg("1")
            .arg("--branch")
            .arg(&reference)
            .arg(&repo)
            .arg(&paths.src_dir);
        run(&mut cmd);
    }

    // Reconfigure/rebuild if config args changed.
    let config_args = std::env::var("TRUEOS_LIMINE_CONFIG_ARGS")
        .unwrap_or_else(|_| default_config_args(&paths.prefix_dir));

    let config_path = paths.build_dir.join(".config_args");
    let prior = read_to_string_if_exists(&config_path);
    if prior.as_deref() != Some(config_args.trim()) {
        remove_dir_if_exists(&paths.build_dir);
        remove_dir_if_exists(&paths.prefix_dir);
    }

    fs::create_dir_all(&paths.build_dir).expect("create build dir");
    fs::create_dir_all(&paths.prefix_dir).expect("create prefix dir");
    write_string(&config_path, config_args.trim());

    // Bootstrap if needed.
    if !paths.src_dir.join("configure").is_file() {
        require_tool(
            "autoreconf",
            "Install autoconf + automake (and likely libtool)",
        );
        let mut cmd = Command::new("sh");
        cmd.current_dir(&paths.src_dir)
            .arg("-lc")
            .arg("./bootstrap");
        run(&mut cmd);
    }

    // Configure into build dir.
    let configure = paths.src_dir.join("configure");
    if !configure.is_file() {
        panic!("Limine configure script missing at {}", configure.display());
    }

    let mut cmd = Command::new(&configure);
    cmd.current_dir(&paths.build_dir)
        .env("CC_FOR_TARGET", "gcc")
        .env("LD_FOR_TARGET", "ld")
        .env("OBJCOPY_FOR_TARGET", "objcopy")
        .env("OBJDUMP_FOR_TARGET", "objdump")
        .env("READELF_FOR_TARGET", "readelf");

    for part in config_args.split_whitespace() {
        cmd.arg(part);
    }
    run(&mut cmd);

    let mut make_cmd = Command::new("make");
    make_cmd.current_dir(&paths.build_dir);
    run(&mut make_cmd);

    let mut install_cmd = Command::new("make");
    install_cmd.current_dir(&paths.build_dir).arg("install");
    run(&mut install_cmd);

    write_string(&paths.stamp(), "ok\n");

    // Tell Cargo when to rerun this build step.
    // We intentionally do NOT rerun on every Limine file change (it lives in bld/). The
    // presence/absence of output files and the config args handle rebuild decisions.
    println!("cargo:rerun-if-env-changed=TRUEOS_LIMINE_REPO");
    println!("cargo:rerun-if-env-changed=TRUEOS_LIMINE_REF");
    println!("cargo:rerun-if-env-changed=TRUEOS_LIMINE_CONFIG_ARGS");
    println!("cargo:rerun-if-env-changed=CARGO_NET_OFFLINE");
}

// Keep public surface small; this is only used from build scripts.
pub fn ensure_limine_from_manifest_dir(manifest_dir: &Path) {
    ensure_limine(manifest_dir);
}

fn _assert_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<LiminePaths>();
}
