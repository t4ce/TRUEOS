pub(crate) const THUNK_BASE: u32 = 0x0030_0000;
pub(crate) const THUNK_BYTES: usize = 12;

#[derive(Copy, Clone, Eq, PartialEq)]
pub(crate) enum Kind {
    Stop,
    Return,
    HeapCreate,
    GetVersionExA,
    InitializeCriticalSection,
    EnterCriticalSection,
    LeaveCriticalSection,
    TlsAlloc,
    HeapAlloc,
    HeapFree,
    CreateEventA,
    GetLastError,
    CloseHandle,
    GetTickCount,
    TlsSetValue,
    GetCurrentThreadId,
    GetStartupInfoA,
    GetModuleFileNameA,
    GetModuleHandleA,
    RegisterClassA,
    GetDesktopWindow,
    GetClientRect,
    CreateWindowExA,
    GetStdHandle,
    GetFileType,
    SetHandleCount,
    GetCommandLineA,
    GetEnvironmentStringsW,
    GetEnvironmentStringsA,
    FreeEnvironmentStringsA,
    GetACP,
    GetCPInfo,
    GetStringTypeW,
    LCMapStringW,
    MultiByteToWideChar,
    WideCharToMultiByte,
}

pub(crate) fn write(import_id: u32, kind: Kind, output: &mut [u8]) -> Result<(), &'static str> {
    if output.len() < THUNK_BYTES {
        return Err("wc3 thunk buffer too small");
    }
    output[..THUNK_BYTES].fill(0x90);
    output[..8].copy_from_slice(&[
        0xB8,
        import_id as u8,
        (import_id >> 8) as u8,
        (import_id >> 16) as u8,
        (import_id >> 24) as u8,
        0x0F,
        0x01,
        0xC1,
    ]);
    match kind {
        Kind::Return => output[8] = 0xC3,
        Kind::HeapCreate => {
            output[8] = 0xC2;
            output[9] = 0x0C;
            output[10] = 0x00;
        }
        Kind::GetVersionExA => {
            output[8] = 0xC2;
            output[9] = 0x04;
            output[10] = 0x00;
        }
        Kind::InitializeCriticalSection => {
            output[8] = 0xC2;
            output[9] = 0x04;
            output[10] = 0x00;
        }
        Kind::EnterCriticalSection | Kind::LeaveCriticalSection => {
            output[8] = 0xC2;
            output[9] = 0x04;
            output[10] = 0x00;
        }
        Kind::TlsAlloc => output[8] = 0xC3,
        Kind::HeapAlloc => {
            output[8] = 0xC2;
            output[9] = 0x0C;
            output[10] = 0x00;
        }
        Kind::HeapFree => {
            output[8] = 0xC2;
            output[9] = 0x0C;
            output[10] = 0x00;
        }
        Kind::CreateEventA => {
            output[8] = 0xC2;
            output[9] = 0x10;
            output[10] = 0x00;
        }
        Kind::GetLastError | Kind::GetTickCount => output[8] = 0xC3,
        Kind::CloseHandle => {
            output[8] = 0xC2;
            output[9] = 0x04;
            output[10] = 0x00;
        }
        Kind::TlsSetValue => {
            output[8] = 0xC2;
            output[9] = 0x08;
            output[10] = 0x00;
        }
        Kind::GetCurrentThreadId => output[8] = 0xC3,
        Kind::GetStartupInfoA => {
            output[8] = 0xC2;
            output[9] = 0x04;
            output[10] = 0x00;
        }
        Kind::GetModuleFileNameA => {
            output[8] = 0xC2;
            output[9] = 0x0C;
            output[10] = 0x00;
        }
        Kind::GetModuleHandleA => {
            output[8] = 0xC2;
            output[9] = 0x04;
            output[10] = 0x00;
        }
        Kind::RegisterClassA => {
            output[8] = 0xC2;
            output[9] = 0x04;
            output[10] = 0x00;
        }
        Kind::GetDesktopWindow => output[8] = 0xC3,
        Kind::GetClientRect => {
            output[8] = 0xC2;
            output[9] = 0x08;
            output[10] = 0x00;
        }
        Kind::CreateWindowExA => {
            output[8] = 0xC2;
            output[9] = 0x30;
            output[10] = 0x00;
        }
        Kind::GetStdHandle | Kind::GetFileType | Kind::SetHandleCount => {
            output[8] = 0xC2;
            output[9] = 0x04;
            output[10] = 0x00;
        }
        Kind::GetCommandLineA | Kind::GetEnvironmentStringsW | Kind::GetEnvironmentStringsA => {
            output[8] = 0xC3;
        }
        Kind::FreeEnvironmentStringsA => {
            output[8] = 0xC2;
            output[9] = 0x04;
            output[10] = 0x00;
        }
        Kind::GetACP => output[8] = 0xC3,
        Kind::GetCPInfo => {
            output[8] = 0xC2;
            output[9] = 0x08;
            output[10] = 0x00;
        }
        Kind::GetStringTypeW => {
            output[8] = 0xC2;
            output[9] = 0x10;
            output[10] = 0x00;
        }
        Kind::MultiByteToWideChar => {
            output[8] = 0xC2;
            output[9] = 0x18;
            output[10] = 0x00;
        }
        Kind::LCMapStringW => {
            output[8] = 0xC2;
            output[9] = 0x18;
            output[10] = 0x00;
        }
        Kind::WideCharToMultiByte => {
            output[8] = 0xC2;
            output[9] = 0x20;
            output[10] = 0x00;
        }
        Kind::Stop => {
            output[8] = 0x0F;
            output[9] = 0x0B;
        }
    }
    Ok(())
}

pub(crate) const fn address(import_id: u32) -> Option<u32> {
    match import_id.checked_mul(THUNK_BYTES as u32) {
        Some(offset) => THUNK_BASE.checked_add(offset),
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{Kind, THUNK_BYTES, write};

    fn cleanup_bytes(kind: Kind) -> [u8; 3] {
        let mut thunk = [0; THUNK_BYTES];
        write(1, kind, &mut thunk).unwrap();
        [thunk[8], thunk[9], thunk[10]]
    }

    #[test]
    fn four_argument_thunks_clean_0x10() {
        assert_eq!(cleanup_bytes(Kind::GetStringTypeW), [0xC2, 0x10, 0x00]);
    }

    #[test]
    fn locale_thunks_clean_their_stdcall_argument_widths() {
        assert_eq!(cleanup_bytes(Kind::MultiByteToWideChar), [0xC2, 0x18, 0x00]);
        assert_eq!(cleanup_bytes(Kind::LCMapStringW), [0xC2, 0x18, 0x00]);
        assert_eq!(cleanup_bytes(Kind::WideCharToMultiByte), [0xC2, 0x20, 0x00]);
    }

    #[test]
    fn post_crt_thunks_clean_their_abi_widths() {
        assert_eq!(cleanup_bytes(Kind::CreateEventA), [0xC2, 0x10, 0x00]);
        assert_eq!(cleanup_bytes(Kind::CloseHandle), [0xC2, 0x04, 0x00]);
        assert_eq!(cleanup_bytes(Kind::GetLastError), [0xC3, 0x00, 0x00]);
        assert_eq!(cleanup_bytes(Kind::GetTickCount), [0xC3, 0x00, 0x00]);
    }
}
