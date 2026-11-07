//! Owned native environment byte pairs; no C string or environ representation.
use alloc::vec::Vec;

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

pub(crate) struct Environment {
    initialized: bool,
    entries: Vec<Entry>,
}

impl Entry {
    fn pair(&self) -> (&[u8], &[u8]) {
        self.bytes.split_at(self.key_len)
    }
}

impl Environment {
    pub(crate) const fn new() -> Self {
        Self {
            initialized: false,
            entries: Vec::new(),
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
    pub(crate) fn set(&mut self, key: &[u8], value: &[u8], overwrite: bool) -> Result<(), i32> {
        self.set_impl(key, value, overwrite, true)
    }
    #[cfg(feature = "legacy_vm_checkpoint")]
    pub(crate) fn refresh_reserved(&mut self, key: &[u8], value: &[u8]) -> Result<(), i32> {
        if !key.starts_with(b"TRUEOS_") {
            return Err(EPERM);
        }
        self.set_impl(key, value, true, false)
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
        let new_len = key.len().saturating_add(value.len());
        let retained: usize = self
            .entries
            .iter()
            .enumerate()
            .filter(|(i, _)| Some(*i) != index)
            .map(|(_, entry)| entry.bytes.len() + 2)
            .sum();
        if key.len() > MAX_STRING
            || value.len() > MAX_STRING
            || retained.saturating_add(new_len).saturating_add(2) > MAX_BYTES
            || (index.is_none() && self.entries.len() >= MAX_ENTRIES)
        {
            return Err(ENOMEM);
        }
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(new_len).map_err(|_| ENOMEM)?;
        bytes.extend_from_slice(key);
        bytes.extend_from_slice(value);
        let entry = Entry {
            key_len: key.len(),
            bytes,
        };
        if index.is_none() {
            self.entries.try_reserve(1).map_err(|_| ENOMEM)?;
        }
        if let Some(index) = index {
            self.entries[index] = entry;
        } else {
            self.entries.push(entry);
        }
        Ok(())
    }
    pub(crate) fn unset(&mut self, key: &[u8]) -> Result<(), i32> {
        Self::writable(key)?;
        if let Some(index) = self.index(key) {
            self.entries.remove(index);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_environment_owns_only_key_value_bytes() {
        let mut env = Environment::new();
        env.initialize([(b"HOME".as_slice(), b"/native".as_slice())].into_iter())
            .unwrap();
        assert_eq!(env.get(b"HOME"), Some(b"/native".as_slice()));
        assert_eq!(
            env.pairs().collect::<Vec<_>>(),
            [(b"HOME".as_slice(), b"/native".as_slice())]
        );
        assert_eq!(env.entries[0].bytes, b"HOME/native");
        env.set(b"HOME", b"/changed", true).unwrap();
        env.set(b"OTHER", b"new", true).unwrap();
        env.unset(b"OTHER").unwrap();
    }
    #[test]
    fn mutation_preserves_overwrite_and_reserved_key_rules() {
        let mut env = Environment::new();
        env.initialize(
            [
                (b"HOME".as_slice(), b"/old".as_slice()),
                (b"TRUEOS_FS_SCOPE".as_slice(), b"private".as_slice()),
            ]
            .into_iter(),
        )
        .unwrap();
        env.set(b"HOME", b"/new", false).unwrap();
        assert_eq!(env.get(b"HOME"), Some(b"/old".as_slice()));
        env.set(b"HOME", b"/new", true).unwrap();
        assert_eq!(env.get(b"HOME"), Some(b"/new".as_slice()));
        env.unset(b"HOME").unwrap();
        assert_eq!(env.get(b"HOME"), None);
        assert_eq!(env.get(b"TRUEOS_FS_SCOPE"), Some(b"private".as_slice()));
    }
    #[test]
    fn errors_leave_existing_values_intact() {
        let mut env = Environment::new();
        env.initialize([(b"TZ".as_slice(), b"UTC0".as_slice())].into_iter())
            .unwrap();
        assert_eq!(env.set(b"", b"x", true), Err(EINVAL));
        assert_eq!(env.set(b"a=b", b"x", true), Err(EINVAL));
        assert_eq!(env.unset(b"TRUEOS_FS_SCOPE"), Err(EPERM));
        assert_eq!(env.set(b"TRUEOS_APP_SHA256", b"fake", true), Err(EPERM));
        assert_eq!(
            env.set(b"TZ", &alloc::vec![b'x';MAX_STRING+1], true),
            Err(ENOMEM)
        );
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
    }
}
