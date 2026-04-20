use anyhow::Result;
#[cfg(not(feature = "trueos-net"))]
use anyhow::Context;
#[cfg(feature = "trueos-net")]
use anyhow::anyhow;
use std::path::Path;

#[cfg(not(feature = "trueos-net"))]
pub fn read_to_string(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))
}

#[cfg(feature = "trueos-net")]
pub fn read_to_string(path: &Path) -> Result<String> {
    let path_str = path
        .to_str()
        .ok_or_else(|| anyhow!("path is not valid UTF-8: {}", path.display()))?;
    v::vio::kfs::read_file_utf8(path_str)
        .map_err(|rc| anyhow!("failed to read {} rc={}", path.display(), rc))
}

#[cfg(not(feature = "trueos-net"))]
pub fn write(path: &Path, contents: impl AsRef<[u8]>) -> Result<()> {
    std::fs::write(path, contents)
        .with_context(|| format!("failed to write {}", path.display()))
}

#[cfg(feature = "trueos-net")]
pub fn write(path: &Path, contents: impl AsRef<[u8]>) -> Result<()> {
    let path_str = path
        .to_str()
        .ok_or_else(|| anyhow!("path is not valid UTF-8: {}", path.display()))?;
    v::vio::kfs::write_file(path_str, contents.as_ref())
        .map_err(|rc| anyhow!("failed to write {} rc={}", path.display(), rc))
}

#[cfg(not(feature = "trueos-net"))]
pub fn create_dir_all(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path)
        .with_context(|| format!("failed to create directory {}", path.display()))
}

#[cfg(feature = "trueos-net")]
pub fn create_dir_all(path: &Path) -> Result<()> {
    let path_str = path
        .to_str()
        .ok_or_else(|| anyhow!("path is not valid UTF-8: {}", path.display()))?;
    v::vio::kfs::create_dir_all(path_str)
        .map_err(|rc| anyhow!("failed to create directory {} rc={}", path.display(), rc))
}

#[cfg(not(feature = "trueos-net"))]
pub fn is_file(path: &Path) -> bool {
    path.is_file()
}

#[cfg(feature = "trueos-net")]
pub fn is_file(path: &Path) -> bool {
    read_to_string(path).is_ok()
}

#[cfg(not(feature = "trueos-net"))]
pub fn exists(path: &Path) -> bool {
    path.exists()
}

#[cfg(feature = "trueos-net")]
pub fn exists(path: &Path) -> bool {
    let Some(path_str) = path.to_str() else {
        return false;
    };
    v::vio::kfs::exists(path_str).unwrap_or(false)
}
