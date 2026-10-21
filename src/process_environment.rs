//! Owned POSIX environment bytes. The caller chooses the process allocation
//! domain and serializes reads/mutations; no returned pointer aliases a source
//! launch map or a temporary allocation.
use alloc::vec::Vec;
use core::ffi::c_char;
use core::ptr;

pub(crate) const EINVAL: i32 = 22;
pub(crate) const EPERM: i32 = 1;
pub(crate) const ENOMEM: i32 = 12;
pub(crate) const MAX_STRING: usize = 64 * 1024;
const MAX_BYTES: usize = 1024 * 1024;
const MAX_ENTRIES: usize = 1024;

struct Entry {
    key_len: usize,
    text: Vec<u8>, // key=value\0, owning both getenv and environ payloads
}

pub(crate) struct Environment {
    initialized: bool,
    entries: Vec<Entry>,
    envp: Vec<*mut c_char>,
    // Importers bind the ADDRESS of this cell, which does not move with envp.
    pub(crate) environ: *mut *mut c_char,
}
unsafe impl Send for Environment {}

impl Environment {
    pub(crate) const fn new() -> Self {
        Self {
            initialized: false,
            entries: Vec::new(),
            envp: Vec::new(),
            environ: ptr::null_mut(),
        }
    }
    pub(crate) fn initialized(&self) -> bool {
        self.initialized
    }
    pub(crate) fn initialize<'a>(
        &mut self,
        initial: impl Iterator<Item = (&'a [u8], &'a [u8])>,
    ) -> Result<(), i32> {
        if self.initialized {
            return Ok(());
        }
        let mut next = Self::new();
        for (key, value) in initial {
            next.set_impl(key, value, true, false)?;
        }
        next.rebuild()?;
        next.initialized = true;
        *self = next;
        Ok(())
    }
    fn index(&self, key: &[u8]) -> Option<usize> {
        self.entries
            .iter()
            .position(|entry| &entry.text[..entry.key_len] == key)
    }
    pub(crate) fn get(&self, key: &[u8]) -> Option<&[u8]> {
        let entry = &self.entries[self.index(key)?];
        Some(&entry.text[entry.key_len + 1..entry.text.len() - 1])
    }
    pub(crate) fn get_ptr(&self, key: &[u8]) -> *mut c_char {
        self.index(key)
            .map(|i| unsafe {
                self.entries[i]
                    .text
                    .as_ptr()
                    .add(self.entries[i].key_len + 1)
                    .cast_mut()
                    .cast()
            })
            .unwrap_or(ptr::null_mut())
    }
    pub(crate) fn pairs(&self) -> impl Iterator<Item = (&[u8], &[u8])> {
        self.entries.iter().map(|entry| {
            (&entry.text[..entry.key_len], &entry.text[entry.key_len + 1..entry.text.len() - 1])
        })
    }
    fn validate(key: &[u8]) -> Result<(), i32> {
        if key.is_empty() || key.contains(&b'=') || key.contains(&0) {
            Err(EINVAL)
        } else {
            Ok(())
        }
    }
    fn writable(key: &[u8]) -> Result<(), i32> {
        Self::validate(key)?;
        if key.starts_with(b"TRUEOS_") {
            Err(EPERM)
        } else {
            Ok(())
        }
    }
    fn reserve_view(&mut self, len: usize) -> Result<(), i32> {
        self.envp
            .try_reserve(len.saturating_sub(self.envp.len()))
            .map_err(|_| ENOMEM)
    }
    fn rebuild(&mut self) -> Result<(), i32> {
        self.reserve_view(self.entries.len() + 1)?;
        self.envp.clear();
        self.envp.extend(
            self.entries
                .iter_mut()
                .map(|entry| entry.text.as_mut_ptr().cast()),
        );
        self.envp.push(ptr::null_mut());
        self.environ = self.envp.as_mut_ptr();
        Ok(())
    }
    pub(crate) fn set(&mut self, key: &[u8], value: &[u8], overwrite: bool) -> Result<(), i32> {
        self.set_impl(key, value, overwrite, true)
    }
    fn set_impl(
        &mut self,
        key: &[u8],
        value: &[u8],
        overwrite: bool,
        protect: bool,
    ) -> Result<(), i32> {
        if protect {
            Self::writable(key)?;
        } else {
            Self::validate(key)?;
        }
        if value.contains(&0) {
            return Err(EINVAL);
        }
        let index = self.index(key);
        if index.is_some() && !overwrite {
            return Ok(());
        }
        let new_len = key.len().saturating_add(value.len()).saturating_add(2);
        let retained: usize = self
            .entries
            .iter()
            .enumerate()
            .filter(|(i, _)| Some(*i) != index)
            .map(|(_, e)| e.text.len())
            .sum();
        if key.len() > MAX_STRING
            || value.len() > MAX_STRING
            || retained.saturating_add(new_len) > MAX_BYTES
            || (index.is_none() && self.entries.len() >= MAX_ENTRIES)
        {
            return Err(ENOMEM);
        }
        let mut text = Vec::new();
        text.try_reserve_exact(new_len).map_err(|_| ENOMEM)?;
        text.extend_from_slice(key);
        text.push(b'=');
        text.extend_from_slice(value);
        text.push(0);
        // Reserve every fallible allocation before changing namespace entries.
        if index.is_none() {
            self.entries.try_reserve(1).map_err(|_| ENOMEM)?;
        }
        self.reserve_view(self.entries.len() + 2)?;
        let entry = Entry {
            key_len: key.len(),
            text,
        };
        if let Some(index) = index {
            self.entries[index] = entry;
        } else {
            self.entries.push(entry);
        }
        self.rebuild()
    }
    pub(crate) fn unset(&mut self, key: &[u8]) -> Result<(), i32> {
        Self::writable(key)?;
        if let Some(index) = self.index(key) {
            self.entries.remove(index);
            self.rebuild()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    unsafe fn snapshot(env: &Environment) -> Vec<Vec<u8>> {
        let mut ptr = env.environ;
        let mut out = Vec::new();
        while !unsafe { *ptr }.is_null() {
            out.push(
                unsafe { core::ffi::CStr::from_ptr(*ptr) }
                    .to_bytes()
                    .to_vec(),
            );
            ptr = unsafe { ptr.add(1) };
        }
        out
    }
    #[test]
    fn mutation_getter_and_environ_are_one_owned_view() {
        let mut env = Environment::new();
        env.initialize(
            [
                (b"HOME".as_slice(), b"/old".as_slice()),
                (b"TRUEOS_FS_SCOPE".as_slice(), b"private".as_slice()),
            ]
            .into_iter(),
        )
        .unwrap();
        let cell = &env.environ as *const _;
        env.set(b"HOME", b"/new", false).unwrap();
        assert_eq!(env.get(b"HOME"), Some(b"/old".as_slice()));
        env.set(b"HOME", b"/new", true).unwrap();
        assert_eq!(unsafe { core::ffi::CStr::from_ptr(env.get_ptr(b"HOME")) }.to_bytes(), b"/new");
        assert_eq!(
            unsafe { snapshot(&env) },
            [b"HOME=/new".to_vec(), b"TRUEOS_FS_SCOPE=private".to_vec()]
        );
        assert_eq!(cell, &env.environ as *const _);
        env.unset(b"HOME").unwrap();
        assert!(env.get_ptr(b"HOME").is_null());
        assert_eq!(unsafe { snapshot(&env) }, [b"TRUEOS_FS_SCOPE=private".to_vec()]);
    }
    #[test]
    fn errors_leave_existing_values_and_view_intact() {
        let mut env = Environment::new();
        env.initialize([(b"TZ".as_slice(), b"UTC0".as_slice())].into_iter())
            .unwrap();
        let ptr = env.environ;
        assert_eq!(env.set(b"", b"x", true), Err(EINVAL));
        assert_eq!(env.set(b"a=b", b"x", true), Err(EINVAL));
        assert_eq!(env.unset(b"TRUEOS_FS_SCOPE"), Err(EPERM));
        assert_eq!(env.set(b"TRUEOS_APP_SHA256", b"fake", true), Err(EPERM));
        assert_eq!(env.set(b"TZ", &alloc::vec![b'x';MAX_STRING+1], true), Err(ENOMEM));
        assert_eq!(env.environ, ptr);
        assert_eq!(env.get(b"TZ"), Some(b"UTC0".as_slice()));
    }
    #[test]
    fn independent_processes_and_non_utf8_values_are_isolated() {
        let mut a = Environment::new();
        let mut b = Environment::new();
        a.initialize(core::iter::empty()).unwrap();
        b.initialize(core::iter::empty()).unwrap();
        a.set(b"\xff", b"\xfe", true).unwrap();
        assert_eq!(a.get(b"\xff"), Some(b"\xfe".as_slice()));
        assert_eq!(b.get(b"\xff"), None);
        assert_ne!(a.environ, b.environ);
    }
}
