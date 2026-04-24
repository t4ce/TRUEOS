#[cfg(not(feature = "trueos-net"))]
use anyhow::Context;
use anyhow::{Result, anyhow};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

#[cfg(feature = "trueos-net")]
const TRUEOS_FALLBACK_HOME: &str = "/home/t4ce";

#[cfg(not(feature = "trueos-net"))]
pub fn args() -> impl Iterator<Item = String> {
    std::env::args()
}

#[cfg(feature = "trueos-net")]
pub fn args() -> impl Iterator<Item = String> {
    v::env::args()
}

#[cfg(not(feature = "trueos-net"))]
pub fn current_dir() -> Result<PathBuf> {
    std::env::current_dir().context("failed to resolve current directory")
}

#[cfg(feature = "trueos-net")]
pub fn current_dir() -> Result<PathBuf> {
    if let Some(path) = var_os("PWD") {
        return Ok(PathBuf::from(path));
    }
    Ok(PathBuf::from("/lc"))
}

pub fn home_dir() -> Result<PathBuf> {
    let home = var_os("HOME")
        .or_else(fallback_home_os)
        .ok_or_else(|| anyhow!("$HOME is not set"))?;
    Ok(PathBuf::from(home))
}

pub fn home_dir_opt() -> Option<PathBuf> {
    var_os("HOME").or_else(fallback_home_os).map(PathBuf::from)
}

#[cfg(not(feature = "trueos-net"))]
pub fn var_os(key: &str) -> Option<OsString> {
    std::env::var_os(key)
}

#[cfg(feature = "trueos-net")]
pub fn var_os(key: &str) -> Option<OsString> {
    match v::env::var(key) {
        Ok(value) => Some(OsString::from(value)),
        Err(_) => None,
    }
}

#[cfg(feature = "trueos-net")]
fn fallback_home_os() -> Option<OsString> {
    Some(OsString::from(TRUEOS_FALLBACK_HOME))
}

#[cfg(not(feature = "trueos-net"))]
fn fallback_home_os() -> Option<OsString> {
    None
}

pub fn path_from_home(relative: &Path) -> Result<PathBuf> {
    Ok(home_dir()?.join(relative))
}
