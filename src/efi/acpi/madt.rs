use crate::efi::acpi::ensure_tables;
use acpi::sdt::madt::Madt;

pub fn walk_subtables<F>(mut callback: F)
where
    F: FnMut(&dyn core::fmt::Debug),
{
    let Some(tables) = ensure_tables() else {
        return;
    };

    // Try to find the MADT table using the acpi crate's mechanism
    if let Some(madt) = tables.find_table::<Madt>() {
        callback(&"MADT Header Found:");
        unsafe {
            callback(madt.virtual_start.as_ref());
        }
    }
}
