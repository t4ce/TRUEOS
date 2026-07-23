use crate::efi::acpi::ensure_tables;
use acpi::sdt::madt::Madt;
use core::pin::Pin;

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
        let madt_ref = unsafe { madt.virtual_start.as_ref() };
        callback(madt_ref);
        callback(&"MADT Entries:");
        unsafe {
            for entry in Pin::new_unchecked(madt_ref).entries() {
                callback(&entry);
            }
        }
    }
}
