//! Native owned environment bytes with a lazy C compatibility view.
//! Native reads and mutations keep byte pairs without C terminators, pointer
//! arrays, or an environ namespace. Only an actual getenv/environ consumer
//! materializes that view, which stays synchronized with subsequent mutations.
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
    bytes: Vec<u8>, // key followed immediately by value; no C encoding
}

struct CView {
    texts: Vec<Vec<u8>>,
    pointers: Vec<*mut c_char>,
}

impl CView {
    fn new<'a>(pairs: impl Iterator<Item = (&'a [u8], &'a [u8])>) -> Result<Self, i32> {
        let mut texts = Vec::new();
        for (key, value) in pairs {
            let mut text = Vec::new();
            text.try_reserve_exact(key.len() + value.len() + 2).map_err(|_| ENOMEM)?;
            text.extend_from_slice(key);
            text.push(b'=');
            text.extend_from_slice(value);
            text.push(0);
            texts.try_reserve(1).map_err(|_| ENOMEM)?;
            texts.push(text);
        }
        let mut pointers = Vec::new();
        pointers.try_reserve_exact(texts.len() + 1).map_err(|_| ENOMEM)?;
        pointers.extend(texts.iter_mut().map(|text| text.as_mut_ptr().cast()));
        pointers.push(ptr::null_mut());
        Ok(Self { texts, pointers })
    }
}

pub(crate) struct Environment {
    initialized: bool,
    entries: Vec<Entry>,
    view: Option<CView>,
    // Legacy importers bind this stable cell only after requesting c_environ.
    pub(crate) environ: *mut *mut c_char,
}
unsafe impl Send for Environment {}

impl Entry {
    fn pair(&self) -> (&[u8], &[u8]) { self.bytes.split_at(self.key_len) }
}

impl Environment {
    pub(crate) const fn new() -> Self {
        Self { initialized: false, entries: Vec::new(), view: None, environ: ptr::null_mut() }
    }
    pub(crate) fn initialized(&self) -> bool { self.initialized }
    pub(crate) fn initialize<'a>(&mut self, initial: impl Iterator<Item = (&'a [u8], &'a [u8])>) -> Result<(), i32> {
        if self.initialized { return Ok(()); }
        let mut next = Self::new();
        for (key, value) in initial { next.set_impl(key, value, true, false)?; }
        next.initialized = true;
        *self = next;
        Ok(())
    }
    fn index(&self, key: &[u8]) -> Option<usize> {
        self.entries.iter().position(|entry| entry.pair().0 == key)
    }
    pub(crate) fn get(&self, key: &[u8]) -> Option<&[u8]> {
        Some(self.entries[self.index(key)?].pair().1)
    }
    pub(crate) fn pairs(&self) -> impl Iterator<Item = (&[u8], &[u8])> {
        self.entries.iter().map(Entry::pair)
    }
    fn replace_view(&mut self, view: CView) {
        self.view = Some(view);
        self.environ = self.view.as_mut().unwrap().pointers.as_mut_ptr();
    }
    pub(crate) fn c_environ(&mut self) -> Result<*mut *mut c_char, i32> {
        if self.view.is_none() { self.replace_view(CView::new(self.pairs())?); }
        Ok(self.environ)
    }
    pub(crate) fn get_ptr(&mut self, key: &[u8]) -> *mut c_char {
        let Some(index) = self.index(key) else { return ptr::null_mut(); };
        if self.c_environ().is_err() { return ptr::null_mut(); }
        unsafe { self.view.as_mut().unwrap().texts[index].as_mut_ptr().add(self.entries[index].key_len + 1).cast() }
    }
    fn validate(key: &[u8]) -> Result<(), i32> {
        if key.is_empty() || key.contains(&b'=') || key.contains(&0) { Err(EINVAL) } else { Ok(()) }
    }
    fn writable(key: &[u8]) -> Result<(), i32> {
        Self::validate(key)?;
        if key.starts_with(b"TRUEOS_") { Err(EPERM) } else { Ok(()) }
    }
    pub(crate) fn set(&mut self, key: &[u8], value: &[u8], overwrite: bool) -> Result<(), i32> {
        self.set_impl(key, value, overwrite, true)
    }
    #[cfg(feature = "legacy_vm_checkpoint")]
    pub(crate) fn refresh_reserved(&mut self, key: &[u8], value: &[u8]) -> Result<(), i32> {
        if !key.starts_with(b"TRUEOS_") { return Err(EPERM); }
        self.set_impl(key, value, true, false)
    }
    fn set_impl(&mut self, key: &[u8], value: &[u8], overwrite: bool, protect: bool) -> Result<(), i32> {
        if protect { Self::writable(key)?; } else { Self::validate(key)?; }
        if value.contains(&0) { return Err(EINVAL); }
        let index = self.index(key);
        if index.is_some() && !overwrite { return Ok(()); }
        let new_len = key.len().saturating_add(value.len());
        let retained: usize = self.entries.iter().enumerate().filter(|(i, _)| Some(*i) != index)
            .map(|(_, entry)| entry.bytes.len() + 2).sum();
        if key.len() > MAX_STRING || value.len() > MAX_STRING
            || retained.saturating_add(new_len).saturating_add(2) > MAX_BYTES
            || (index.is_none() && self.entries.len() >= MAX_ENTRIES)
        { return Err(ENOMEM); }
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(new_len).map_err(|_| ENOMEM)?;
        bytes.extend_from_slice(key);
        bytes.extend_from_slice(value);
        let entry = Entry { key_len: key.len(), bytes };
        if index.is_none() { self.entries.try_reserve(1).map_err(|_| ENOMEM)?; }
        // Prepare every fallible C-view allocation before committing either
        // namespace. A native process pays none of this encoding cost.
        let view = if self.view.is_some() {
            let pairs = self.entries.iter().enumerate().map(|(i, old)|
                if Some(i) == index { entry.pair() } else { old.pair() })
                .chain(index.is_none().then_some(entry.pair()).into_iter());
            Some(CView::new(pairs)?)
        } else { None };
        if let Some(index) = index { self.entries[index] = entry; } else { self.entries.push(entry); }
        if let Some(view) = view { self.replace_view(view); }
        Ok(())
    }
    pub(crate) fn unset(&mut self, key: &[u8]) -> Result<(), i32> {
        Self::writable(key)?;
        if let Some(index) = self.index(key) {
            let view = if self.view.is_some() {
                Some(CView::new(self.entries.iter().enumerate().filter(|(i, _)| *i != index).map(|(_, entry)| entry.pair()))?)
            } else { None };
            self.entries.remove(index);
            if let Some(view) = view { self.replace_view(view); }
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
    fn native_environment_never_materializes_c_storage() {
        let mut env = Environment::new();
        env.initialize([(b"HOME".as_slice(), b"/native".as_slice())].into_iter()).unwrap();
        assert_eq!(env.get(b"HOME"), Some(b"/native".as_slice()));
        assert_eq!(env.pairs().collect::<Vec<_>>(), [(b"HOME".as_slice(), b"/native".as_slice())]);
        assert_eq!(env.entries[0].bytes, b"HOME/native");
        env.set(b"HOME", b"/changed", true).unwrap();
        env.set(b"OTHER", b"new", true).unwrap();
        env.unset(b"OTHER").unwrap();
        assert!(env.view.is_none());
        assert!(env.environ.is_null());
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
        env.c_environ().unwrap();
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
        a.c_environ().unwrap();
        b.c_environ().unwrap();
        assert_ne!(a.environ, b.environ);
    }
}
