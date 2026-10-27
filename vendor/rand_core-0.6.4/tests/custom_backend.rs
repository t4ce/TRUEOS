//! Verify the custom randomness ABI used by TrueOS, including failure handling.
#![cfg(all(feature = "getrandom", getrandom_backend = "custom"))]

#[cfg(not(feature = "std"))]
use rand_core::Error;
use rand_core::{OsRng, RngCore, SeedableRng};

#[unsafe(no_mangle)]
unsafe fn __getrandom_v03_custom(dest: *mut u8, len: usize) -> Result<(), getrandom::Error> {
    match len {
        1 => Err(getrandom::Error::new_custom(1)),
        2 => Err(getrandom::Error::new_custom(42)),
        _ => {
            // SAFETY: getrandom passes a valid writable buffer of this size.
            unsafe { core::ptr::write_bytes(dest, 0xa5, len) };
            Ok(())
        }
    }
}

#[derive(Debug, PartialEq)]
struct Seed([u8; 4]);

impl SeedableRng for Seed {
    type Seed = [u8; 4];

    fn from_seed(seed: Self::Seed) -> Self {
        Seed(seed)
    }
}

#[test]
fn custom_backend_fills_seeds_and_preserves_errors() {
    let mut bytes = [0; 4];
    OsRng.try_fill_bytes(&mut bytes).unwrap();
    assert_eq!(bytes, [0xa5; 4]);
    assert_eq!(Seed::from_entropy(), Seed([0xa5; 4]));
    OsRng.try_fill_bytes(&mut []).unwrap();

    let native_error = OsRng.try_fill_bytes(&mut [0; 1]).unwrap_err();
    let opaque_error = OsRng.try_fill_bytes(&mut [0; 2]).unwrap_err();
    assert_eq!(native_error.raw_os_error(), None);
    assert_eq!(opaque_error.raw_os_error(), None);

    #[cfg(not(feature = "std"))]
    {
        assert_eq!(native_error.code().unwrap().get(), Error::CUSTOM_START + 1);
        assert_eq!(opaque_error.code().unwrap().get(), Error::INTERNAL_START + 0xffff);
    }
    #[cfg(feature = "std")]
    {
        assert_eq!(native_error.inner().downcast_ref::<getrandom::Error>(),
            Some(&getrandom::Error::new_custom(1)));
        assert_eq!(opaque_error.inner().downcast_ref::<getrandom::Error>(),
            Some(&getrandom::Error::new_custom(42)));
    }
}
