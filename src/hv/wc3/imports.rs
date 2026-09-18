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
        let kind = if is_get_version(import) {
            super::thunk32::Kind::Return
        } else if is_heap_create(import) {
            super::thunk32::Kind::HeapCreate
        } else {
            super::thunk32::Kind::Stop
        };
        super::thunk32::write(import.id, kind, output)?;
        let iat = image
            .get_mut(slot..slot.checked_add(4).ok_or("wc3 IAT overflow")?)
            .ok_or("wc3 IAT outside image")?;
        iat.copy_from_slice(&thunk.to_le_bytes());
    }
    Ok(())
}

pub(crate) fn find_get_version(imports: &[LauncherImport]) -> bool {
    imports
        .iter()
        .filter(|import| is_get_version(import))
        .count()
        == 1
}

pub(crate) fn is_get_version(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("KERNEL32.dll") && import.symbol == "GetVersion"
}

pub(crate) fn find_heap_create(imports: &[LauncherImport]) -> bool {
    imports
        .iter()
        .filter(|import| is_heap_create(import))
        .count()
        == 1
}

pub(crate) fn is_heap_create(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("KERNEL32.dll") && import.symbol == "HeapCreate"
}

pub(crate) fn new_table() -> Vec<LauncherImport> {
    Vec::new()
}
