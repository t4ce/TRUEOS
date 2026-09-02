/// Revalidate and expose the exact captured HII package-list export to explicit
/// diagnostic consumers. The raw configuration section is deliberately not
/// returned by this function.
pub(crate) fn with_raw_hii<R>(f: impl FnOnce(&[u8]) -> R) -> Result<R, String> {
    let captured = locate_captured_sections()?;
    Ok(f(captured.hii))
}
