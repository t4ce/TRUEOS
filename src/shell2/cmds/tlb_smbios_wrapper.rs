#[path = "tlb_smbios.rs"]
mod decoder;

pub(crate) fn append_dump(out: &mut alloc::string::String) {
    decoder::append_dump(out);
    super::tlb_platform::append_dump(out);
}
