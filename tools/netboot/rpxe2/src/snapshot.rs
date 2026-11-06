use anyhow::{Context, Result, ensure};
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::Duration,
};
use tempfile::{Builder, TempDir};

pub const FILES: [&str; 3] = [
    "EFI/BOOT/BOOTX64.EFI",
    "TRUEOS.elf",
    "EFI/BOOT/tgl_guc_70.bin",
];

#[derive(Debug, PartialEq, Eq)]
struct Signature {
    source: PathBuf,
    // Device, inode, size, and nanosecond modification time.
    files: Vec<(u64, u64, u64, i64, i64)>,
}

impl Signature {
    fn read(source: &Path) -> Result<Self> {
        let source = fs::canonicalize(source)
            .with_context(|| format!("Cannot resolve artifacts: {}", source.display()))?;
        let files = FILES
            .iter()
            .map(|name| {
                let file = source.join(name);
                let stat = fs::metadata(&file)
                    .with_context(|| format!("Cannot read {}", file.display()))?;
                ensure!(stat.is_file(), "Not a file: {}", file.display());
                Ok((
                    stat.dev(),
                    stat.ino(),
                    stat.len(),
                    stat.mtime(),
                    stat.mtime_nsec(),
                ))
            })
            .collect::<Result<_>>()?;
        Ok(Self { source, files })
    }
}

pub struct Snapshot {
    root: TempDir,
    source: PathBuf,
    signature: Option<Signature>,
}

impl Snapshot {
    pub fn new(source: PathBuf) -> Result<Self> {
        let root = Builder::new().prefix("trueos-rpxe2-").tempdir()?;
        readable_dirs(root.path())?;
        for name in FILES {
            let target = if name.contains('/') {
                PathBuf::from("../../active").join(name)
            } else {
                PathBuf::from("active").join(name)
            };
            symlink(target, root.path().join(name))?;
        }
        let mut snapshot = Self {
            root,
            source,
            signature: None,
        };
        ensure!(
            snapshot.refresh()?,
            "Artifacts changed during initial snapshot; retry startup"
        );
        Ok(snapshot)
    }

    pub fn root(&self) -> &Path {
        self.root.path()
    }

    pub fn refresh(&mut self) -> Result<bool> {
        let before = Signature::read(&self.source)?;
        if self.signature.as_ref() == Some(&before) {
            return Ok(false);
        }
        // The guard removes incomplete generations on any copy or validation error.
        let generation = Builder::new()
            .prefix("generation-")
            .tempdir_in(self.root())?;
        readable_dirs(generation.path())?;
        for name in FILES {
            let target = generation.path().join(name);
            fs::copy(before.source.join(name), &target)?;
            fs::set_permissions(target, fs::Permissions::from_mode(0o644))?;
        }
        if Signature::read(&self.source)? != before {
            return Ok(false);
        }
        let next = self.root().join("next");
        symlink(generation.path().file_name().unwrap(), &next)?;
        if let Err(error) = fs::rename(&next, self.root().join("active")) {
            let _ = fs::remove_file(next);
            return Err(error.into());
        }
        // Retain old generations for transfers already using their files.
        let _ = generation.keep();
        println!("PXE files loaded: {}", before.source.display());
        self.signature = Some(before);
        Ok(true)
    }

    pub fn watch(mut self) -> Watcher {
        let (stop, receive) = mpsc::channel();
        let thread = thread::spawn(move || {
            while receive.recv_timeout(Duration::from_millis(100))
                == Err(mpsc::RecvTimeoutError::Timeout)
            {
                if let Err(error) = self.refresh() {
                    eprintln!("rpxe2 reload: {error:#}");
                }
            }
            // Drop the root only after the supervisor has stopped dnsmasq.
        });
        Watcher {
            stop,
            thread: Some(thread),
        }
    }
}

fn readable_dirs(root: &Path) -> Result<()> {
    fs::create_dir_all(root.join("EFI/BOOT"))?;
    for dir in [root.to_path_buf(), root.join("EFI"), root.join("EFI/BOOT")] {
        fs::set_permissions(dir, fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

pub struct Watcher {
    stop: mpsc::Sender<()>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Drop for Watcher {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn populate(dir: &Path, bytes: &[u8]) {
        fs::create_dir_all(dir.join("EFI/BOOT")).unwrap();
        for name in FILES {
            fs::write(dir.join(name), bytes).unwrap();
        }
        fs::write(dir.join("secret"), b"private").unwrap();
    }

    #[test]
    fn publication_is_atomic_and_retains_old_generation() {
        let tmp = tempfile::tempdir().unwrap();
        let first = tmp.path().join("first");
        let second = tmp.path().join("second");
        populate(&first, b"first");
        populate(&second, b"second");
        let source = tmp.path().join("current");
        symlink(&first, &source).unwrap();
        let mut snapshot = Snapshot::new(source.clone()).unwrap();
        let old = fs::canonicalize(snapshot.root().join("active")).unwrap();
        assert!(!snapshot.root().join("secret").exists());
        assert!(!snapshot.refresh().unwrap());
        symlink(&second, tmp.path().join("next")).unwrap();
        fs::rename(tmp.path().join("next"), &source).unwrap();
        assert!(snapshot.refresh().unwrap());
        for name in FILES {
            assert_eq!(fs::read(snapshot.root().join(name)).unwrap(), b"second");
            assert_eq!(fs::read(old.join(name)).unwrap(), b"first");
            assert_eq!(
                fs::metadata(snapshot.root().join(name)).unwrap().mode() & 0o777,
                0o644
            );
        }
        let root = snapshot.root().to_path_buf();
        drop(snapshot);
        assert!(!root.exists());
    }

    #[test]
    fn failed_reload_keeps_last_complete_snapshot() {
        let tmp = tempfile::tempdir().unwrap();
        populate(tmp.path(), b"complete");
        let mut snapshot = Snapshot::new(tmp.path().to_path_buf()).unwrap();
        fs::write(tmp.path().join(FILES[0]), b"incomplete").unwrap();
        fs::remove_file(tmp.path().join(FILES[1])).unwrap();
        assert!(snapshot.refresh().is_err());
        for name in FILES {
            assert_eq!(fs::read(snapshot.root().join(name)).unwrap(), b"complete");
        }
        populate(tmp.path(), b"recovered");
        assert!(snapshot.refresh().unwrap());
    }
}
