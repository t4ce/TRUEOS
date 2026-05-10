use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const DEFAULT_LIMINE_REPO: &str = "https://github.com/limine-bootloader/limine.git";
const DEFAULT_LIMINE_REF: &str = "v10.x";
const LIMINE_SUBMODULE_PATH: &str = "vendor/limine";

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

fn copy_dir_recursive(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap_or_else(|e| panic!("create {} failed: {e}", dst.display()));

    for entry in fs::read_dir(src).unwrap_or_else(|e| panic!("read {} failed: {e}", src.display()))
    {
        let entry = entry.unwrap_or_else(|e| panic!("read entry in {} failed: {e}", src.display()));
        let name = entry.file_name();
        if name == ".git" {
            continue;
        }

        let src_path = entry.path();
        let dst_path = dst.join(&name);
        let ty = entry
            .file_type()
            .unwrap_or_else(|e| panic!("stat {} failed: {e}", src_path.display()));

        if ty.is_dir() {
            copy_dir_recursive(&src_path, &dst_path);
        } else if ty.is_file() {
            fs::copy(&src_path, &dst_path).unwrap_or_else(|e| {
                panic!("copy {} to {} failed: {e}", src_path.display(), dst_path.display())
            });
        } else if ty.is_symlink() {
            #[cfg(unix)]
            {
                let target = fs::read_link(&src_path)
                    .unwrap_or_else(|e| panic!("readlink {} failed: {e}", src_path.display()));
                std::os::unix::fs::symlink(&target, &dst_path)
                    .unwrap_or_else(|e| panic!("symlink {} failed: {e}", dst_path.display()));
            }
        }
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

fn tool_search_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(path_var) = env::var_os("PATH") {
        roots.extend(env::split_paths(&path_var));
    }
    roots.push(PathBuf::from("/opt/homebrew/opt/llvm/bin"));
    roots.push(PathBuf::from("/usr/local/opt/llvm/bin"));
    roots.push(PathBuf::from("/opt/homebrew/opt/binutils/bin"));
    roots.push(PathBuf::from("/usr/local/opt/binutils/bin"));
    roots
}

fn find_tool(tool_names: &[&str]) -> Option<PathBuf> {
    let roots = tool_search_roots();
    for root in roots {
        for tool_name in tool_names {
            let candidate = root.join(tool_name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn require_any_tool(tool_names: &[&str], hint: &str) -> PathBuf {
    find_tool(tool_names).unwrap_or_else(|| {
        panic!("Missing required tool '{}'. Hint: {hint}", tool_names.join(" or "))
    })
}

fn tool_display_name(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

struct LimineToolchain {
    cc: PathBuf,
    ld: PathBuf,
    objcopy: PathBuf,
    objdump: PathBuf,
    readelf: PathBuf,
}

impl LimineToolchain {
    fn detect() -> Self {
        Self {
            cc: require_any_tool(&["gcc", "clang", "cc"], "Install gcc or clang"),
            ld: require_any_tool(&["ld.lld", "gld", "ld"], "Install LLVM lld or GNU ld"),
            objcopy: require_any_tool(
                &["llvm-objcopy", "gobjcopy", "objcopy"],
                "Install Homebrew llvm or binutils",
            ),
            objdump: require_any_tool(
                &["llvm-objdump", "gobjdump", "objdump"],
                "Install Homebrew llvm or binutils",
            ),
            readelf: require_any_tool(
                &["llvm-readelf", "greadelf", "readelf"],
                "Install Homebrew llvm or binutils",
            ),
        }
    }

    fn stamp_contents(&self) -> String {
        format!(
            "CC_FOR_TARGET={}\nLD_FOR_TARGET={}\nOBJCOPY_FOR_TARGET={}\nOBJDUMP_FOR_TARGET={}\nREADELF_FOR_TARGET={}\n",
            self.cc.display(),
            self.ld.display(),
            self.objcopy.display(),
            self.objdump.display(),
            self.readelf.display()
        )
    }
}

fn clone_limine_repo(paths: &LiminePaths, repo: &str, reference: &str) {
    remove_dir_if_exists(&paths.src_dir);
    fs::create_dir_all(paths.src_dir.parent().unwrap()).expect("create bld/");

    let mut cmd = Command::new("git");
    cmd.arg("clone")
        .arg("--depth")
        .arg("1")
        .arg("--branch")
        .arg(reference)
        .arg(repo)
        .arg(&paths.src_dir);
    run(&mut cmd);
}

fn git_output(args: &[&str], cwd: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn source_stamp(paths: &LiminePaths) -> Option<String> {
    let head = git_output(&["rev-parse", "HEAD"], &paths.submodule_dir)?;
    Some(format!("submodule:{head}"))
}

fn ensure_limine_submodule(repo_root: &Path, paths: &LiminePaths) {
    if paths.submodule_dir.join(".git").exists() || paths.submodule_dir.join("bootstrap").is_file()
    {
        return;
    }

    let mut cmd = Command::new("git");
    cmd.current_dir(repo_root)
        .arg("submodule")
        .arg("update")
        .arg("--init")
        .arg(LIMINE_SUBMODULE_PATH);
    run(&mut cmd);
}

fn prepare_limine_source(repo_root: &Path, paths: &LiminePaths, repo: &str, reference: &str) {
    let using_default_source = repo == DEFAULT_LIMINE_REPO && reference == DEFAULT_LIMINE_REF;
    if !using_default_source {
        if !paths.src_dir.join(".git").exists() {
            clone_limine_repo(paths, repo, reference);
        }
        return;
    }

    ensure_limine_submodule(repo_root, paths);
    let stamp = source_stamp(paths).unwrap_or_else(|| {
        panic!("Limine submodule at {} is not initialized", paths.submodule_dir.display())
    });
    let stamp_path = paths.src_dir.join(".trueos_source_stamp");
    let prior = read_to_string_if_exists(&stamp_path);
    if prior.as_deref() == Some(stamp.as_str()) && paths.src_dir.join("bootstrap").is_file() {
        return;
    }

    remove_dir_if_exists(&paths.src_dir);
    copy_dir_recursive(&paths.submodule_dir, &paths.src_dir);
    write_string(&stamp_path, &stamp);
}

pub struct LiminePaths {
    pub submodule_dir: PathBuf,
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

    pub fn toolchain_stamp(&self) -> PathBuf {
        self.build_dir.join(".toolchain_args")
    }
}

pub fn default_paths(repo_root: &Path) -> LiminePaths {
    LiminePaths {
        submodule_dir: repo_root.join(LIMINE_SUBMODULE_PATH),
        src_dir: repo_root.join("bld").join("limine-src"),
        build_dir: repo_root.join("bld").join("limine-build"),
        prefix_dir: repo_root.join("bld").join("limine-prefix"),
    }
}

fn default_config_args(prefix: &Path) -> String {
    format!("--prefix={} --enable-uefi-x86-64 --enable-uefi-cd", prefix.display())
}

fn source_stamp_changed(paths: &LiminePaths) -> bool {
    if !paths.submodule_dir.join("bootstrap").is_file() {
        return false;
    }

    let Some(stamp) = source_stamp(paths) else {
        return false;
    };
    read_to_string_if_exists(&paths.src_dir.join(".trueos_source_stamp")).as_deref()
        != Some(stamp.as_str())
}

fn should_build(paths: &LiminePaths) -> bool {
    let share = paths.share_dir();
    // ISO build needs these.
    let need_uefi =
        is_file(&share.join("BOOTX64.EFI")) && is_file(&share.join("limine-uefi-cd.bin"));

    !need_uefi || !is_file(&paths.stamp()) || source_stamp_changed(paths)
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
    let toolchain = LimineToolchain::detect();

    // Limine generates configure via ./bootstrap (autoreconf).
    // Match the existing Makefile behavior: only require autoreconf when configure is missing.

    let repo =
        std::env::var("TRUEOS_LIMINE_REPO").unwrap_or_else(|_| DEFAULT_LIMINE_REPO.to_string());
    let reference =
        std::env::var("TRUEOS_LIMINE_REF").unwrap_or_else(|_| DEFAULT_LIMINE_REF.to_string());

    // If Cargo is in offline mode, don't try to hit the network.
    if std::env::var_os("CARGO_NET_OFFLINE").is_some() {
        if !paths.src_dir.exists() && !paths.submodule_dir.exists() {
            panic!(
                "Limine sources not found at {} or {} and CARGO_NET_OFFLINE is set. \
Run git submodule update --init {} with network access once.",
                paths.src_dir.display(),
                paths.submodule_dir.display(),
                LIMINE_SUBMODULE_PATH
            );
        }
    }

    prepare_limine_source(repo_root, &paths, &repo, &reference);

    // Reconfigure/rebuild if config args changed.
    let config_args = std::env::var("TRUEOS_LIMINE_CONFIG_ARGS")
        .unwrap_or_else(|_| default_config_args(&paths.prefix_dir));

    let config_path = paths.build_dir.join(".config_args");
    let prior = read_to_string_if_exists(&config_path);
    let toolchain_path = paths.toolchain_stamp();
    let prior_toolchain = read_to_string_if_exists(&toolchain_path);
    let toolchain_stamp = toolchain.stamp_contents();
    if prior.as_deref() != Some(config_args.trim())
        || prior_toolchain.as_deref() != Some(toolchain_stamp.trim())
    {
        remove_dir_if_exists(&paths.build_dir);
        remove_dir_if_exists(&paths.prefix_dir);
    }

    fs::create_dir_all(&paths.build_dir).expect("create build dir");
    fs::create_dir_all(&paths.prefix_dir).expect("create prefix dir");
    write_string(&config_path, config_args.trim());
    write_string(&toolchain_path, toolchain_stamp.trim());

    // Bootstrap if needed.
    if !paths.src_dir.join("configure").is_file() {
        if !paths.src_dir.join("bootstrap").is_file() {
            // Self-heal broken/incomplete source trees instead of panicking on ./bootstrap.
            prepare_limine_source(repo_root, &paths, &repo, &reference);
        }
        require_tool("autoreconf", "Install autoconf + automake (and likely libtool)");
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
        .env("CC_FOR_TARGET", tool_display_name(&toolchain.cc))
        .env("LD_FOR_TARGET", tool_display_name(&toolchain.ld))
        .env("OBJCOPY_FOR_TARGET", tool_display_name(&toolchain.objcopy))
        .env("OBJDUMP_FOR_TARGET", tool_display_name(&toolchain.objdump))
        .env("READELF_FOR_TARGET", tool_display_name(&toolchain.readelf));

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

pub fn ensure_limine_from_manifest_dir(manifest_dir: &Path) {
    ensure_limine(manifest_dir);
}
