use alloc::{string::String, vec::Vec};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LauncherImport {
    pub(crate) id: u32,
    pub(crate) module: String,
    pub(crate) symbol: String,
    pub(crate) iat_rva: u32,
}

pub(crate) fn patch(
    image: &mut [u8],
    imports: &[LauncherImport],
    thunks: &mut [u8],
) -> Result<(), &'static str> {
    for import in imports {
        let thunk = super::thunk32::address(import.id).ok_or("wc3 thunk address overflow")?;
        let slot = usize::try_from(import.iat_rva).map_err(|_| "wc3 IAT rva")?;
        let output = thunks
            .get_mut(
                usize::try_from(import.id)
                    .map_err(|_| "wc3 import id")?
                    .saturating_mul(super::thunk32::THUNK_BYTES)..,
            )
            .ok_or("wc3 thunk output range")?;
        super::thunk32::write(import.id, output)?;
        let iat = image
            .get_mut(slot..slot.checked_add(4).ok_or("wc3 IAT overflow")?)
            .ok_or("wc3 IAT outside image")?;
        iat.copy_from_slice(&thunk.to_le_bytes());
    }
    Ok(())
}

pub(crate) fn find_get_version(imports: &[LauncherImport]) -> bool {
    imports.iter().any(|import| {
        import.module.eq_ignore_ascii_case("KERNEL32.dll") && import.symbol == "GetVersion"
    })
}

pub(crate) fn new_table() -> Vec<LauncherImport> {
    Vec::new()
}
