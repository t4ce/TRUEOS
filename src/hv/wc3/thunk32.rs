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
    TlsSetValue,
    GetCurrentThreadId,
    GetStartupInfoA,
    GetStdHandle,
    GetFileType,
    SetHandleCount,
    GetCommandLineA,
    GetEnvironmentStringsW,
    GetEnvironmentStringsA,
    FreeEnvironmentStringsA,
    GetACP,
    GetCPInfo,
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
