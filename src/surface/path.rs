use alloc::{
    borrow::{Cow, ToOwned},
    string::String,
};
use core::borrow::Borrow;
use core::ops::Deref;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Component<'a> {
    RootDir,
    CurDir,
    ParentDir,
    Normal(&'a str),
}

#[derive(Debug, Clone)]
pub struct Components<'a> {
    path: &'a str,
    pos: usize,
    yielded_root: bool,
}

#[repr(transparent)]
pub struct Path {
    inner: str,
}

pub struct PathBuf {
    inner: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StripPrefixError;

impl Path {
    pub fn new<S: AsRef<str> + ?Sized>(s: &S) -> &Path {
        unsafe { &*(s.as_ref() as *const str as *const Path) }
    }

    pub fn as_str(&self) -> &str {
        &self.inner
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.as_str().as_bytes()
    }

    pub fn is_absolute(&self) -> bool {
        self.as_str().starts_with('/')
    }

    pub fn is_relative(&self) -> bool {
        !self.is_absolute()
    }

    pub fn parent(&self) -> Option<&Path> {
        let s = self.trim_trailing_slash();
        if s.is_empty() {
            return None;
        }
        let idx = s.rfind('/')?;
        if idx == 0 {
            return Some(Path::new("/"));
        }
        Some(Path::new(&s[..idx]))
    }

    pub fn file_name(&self) -> Option<&str> {
        let s = self.trim_trailing_slash();
        let name = s.rsplit_once('/').map(|(_, tail)| tail).unwrap_or(s);
        if name.is_empty() { None } else { Some(name) }
    }

    pub fn file_stem(&self) -> Option<&str> {
        let name = self.file_name()?;
        if let Some((stem, _)) = name.rsplit_once('.') {
            if stem.is_empty() { None } else { Some(stem) }
        } else {
            Some(name)
        }
    }

    pub fn extension(&self) -> Option<&str> {
        let name = self.file_name()?;
        let (_stem, ext) = name.rsplit_once('.')?;
        if ext.is_empty() { None } else { Some(ext) }
    }

    pub fn with_extension(&self, ext: &str) -> PathBuf {
        let mut base = match self.file_name() {
            Some(name) => {
                let s = self.as_str();
                let cutoff = s.len() - name.len();
                String::from(&s[..cutoff]) + name.split('.').next().unwrap_or("")
            }
            None => self.as_str().to_owned(),
        };
        if !ext.is_empty() {
            base.push('.');
            base.push_str(ext);
        }
        PathBuf { inner: base }
    }

    pub fn join<P: AsRef<Path>>(&self, path: P) -> PathBuf {
        let p = path.as_ref();
        if p.is_absolute() {
            return PathBuf::from(p.as_str());
        }
        let mut buf = PathBuf::from(self.as_str());
        buf.push(p);
        buf
    }

    pub fn starts_with<P: AsRef<Path>>(&self, base: P) -> bool {
        let b = base.as_ref().as_str();
        let s = self.as_str();
        if b == "/" {
            return self.is_absolute();
        }
        if s == b {
            return true;
        }
        if let Some(rest) = s.strip_prefix(b) {
            return rest.starts_with('/') || rest.is_empty();
        }
        false
    }

    pub fn strip_prefix<P: AsRef<Path>>(&self, base: P) -> Result<&Path, StripPrefixError> {
        let b = base.as_ref().as_str();
        let s = self.as_str();
        if b == "/" {
            return if self.is_absolute() {
                Ok(self)
            } else {
                Err(StripPrefixError)
            };
        }
        if let Some(rest) = s.strip_prefix(b) {
            if rest.is_empty() {
                return Ok(Path::new(""));
            }
            if rest.starts_with('/') {
                return Ok(Path::new(&rest[1..]));
            }
        }
        Err(StripPrefixError)
    }

    pub fn components(&self) -> Components<'_> {
        Components {
            path: self.as_str(),
            pos: 0,
            yielded_root: false,
        }
    }

    fn trim_trailing_slash(&self) -> &str {
        let s = self.as_str();
        if s.len() > 1 {
            s.trim_end_matches('/')
        } else {
            s
        }
    }
}

impl PathBuf {
    pub fn new() -> Self {
        Self {
            inner: String::new(),
        }
    }

    pub fn as_str(&self) -> &str {
        self.inner.as_str()
    }

    pub fn from<S: AsRef<str>>(s: S) -> Self {
        Self {
            inner: s.as_ref().to_owned(),
        }
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, core::str::Utf8Error> {
        core::str::from_utf8(bytes).map(Self::from)
    }

    pub fn from_bytes_lossy(bytes: &[u8]) -> Self {
        match String::from_utf8_lossy(bytes) {
            Cow::Borrowed(s) => PathBuf::from(s),
            Cow::Owned(s) => PathBuf::from(s),
        }
    }

    pub fn as_path(&self) -> &Path {
        Path::new(self.inner.as_str())
    }

    pub fn push<P: AsRef<Path>>(&mut self, path: P) {
        let p = path.as_ref();
        if p.is_absolute() {
            self.inner.clear();
            self.inner.push_str(p.as_str());
            return;
        }
        if !self.inner.is_empty() && !self.inner.ends_with('/') {
            self.inner.push('/');
        }
        self.inner.push_str(p.as_str());
    }

    pub fn pop(&mut self) -> bool {
        let s = Path::new(self.as_str());
        if let Some(parent) = s.parent() {
            self.inner.truncate(parent.as_str().len());
            true
        } else {
            if self.inner.is_empty() {
                false
            } else {
                self.inner.clear();
                true
            }
        }
    }
}

impl AsRef<Path> for PathBuf {
    fn as_ref(&self) -> &Path {
        self.as_path()
    }
}

impl AsRef<Path> for Path {
    fn as_ref(&self) -> &Path {
        self
    }
}

impl AsRef<Path> for str {
    fn as_ref(&self) -> &Path {
        Path::new(self)
    }
}

impl AsRef<Path> for String {
    fn as_ref(&self) -> &Path {
        Path::new(self.as_str())
    }
}

impl Deref for PathBuf {
    type Target = Path;
    fn deref(&self) -> &Self::Target {
        self.as_path()
    }
}

impl Borrow<Path> for PathBuf {
    fn borrow(&self) -> &Path {
        self.as_path()
    }
}

impl From<String> for PathBuf {
    fn from(value: String) -> Self {
        Self { inner: value }
    }
}

impl From<&str> for PathBuf {
    fn from(value: &str) -> Self {
        Self {
            inner: value.to_owned(),
        }
    }
}

impl ToOwned for Path {
    type Owned = PathBuf;
    fn to_owned(&self) -> Self::Owned {
        PathBuf::from(self.as_str())
    }
}

impl<'a> Iterator for Components<'a> {
    type Item = Component<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if !self.yielded_root && self.path.starts_with('/') {
            self.yielded_root = true;
            // Skip duplicate leading slashes.
            while self.pos < self.path.len() && self.path.as_bytes()[self.pos] == b'/' {
                self.pos += 1;
            }
            return Some(Component::RootDir);
        }

        while self.pos < self.path.len() {
            // Skip consecutive separators.
            while self.pos < self.path.len() && self.path.as_bytes()[self.pos] == b'/' {
                self.pos += 1;
            }
            if self.pos >= self.path.len() {
                break;
            }
            let start = self.pos;
            while self.pos < self.path.len() && self.path.as_bytes()[self.pos] != b'/' {
                self.pos += 1;
            }
            let seg = &self.path[start..self.pos];
            if seg.is_empty() {
                continue;
            }
            return Some(match seg {
                "." => Component::CurDir,
                ".." => Component::ParentDir,
                _ => Component::Normal(seg),
            });
        }

        None
    }
}

/// Normalize a user-provided POSIX-style path into a FAT-friendly *relative* path.
///
/// Properties:
/// - Collapses duplicate separators.
/// - Ignores a leading `/` (treats absolute as rooted-at-volume).
/// - Drops `.` segments.
/// - Rejects any `..` segment (returns `None`).
/// - Returns an empty string for inputs like `""`, `"/"`, or `"./"`.
///
/// Intended as a small, shared policy layer between shell/QJS and the FAT backend.
pub fn normalize_rel_no_parent(input: &str) -> Option<String> {
    let s = input.trim();
    if s.is_empty() {
        return Some(String::new());
    }
    if s.as_bytes().iter().any(|&b| b == 0) {
        return None;
    }

    let p = Path::new(s);
    let mut out = String::new();
    for c in p.components() {
        match c {
            Component::RootDir => {
                // Keep output relative; absolute input just means “from volume root”.
            }
            Component::CurDir => {}
            Component::ParentDir => return None,
            Component::Normal(seg) => {
                if seg.is_empty() {
                    continue;
                }
                if !out.is_empty() {
                    out.push('/');
                }
                out.push_str(seg);
            }
        }
    }

    Some(out)
}
