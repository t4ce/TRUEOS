#![cfg(feature = "trueos")]

pub(crate) struct EmbeddedModule {
    pub(crate) path: &'static [u8],
    pub(crate) src: &'static [u8],
    pub(crate) bytecode: &'static [u8],
}

// Keep this tiny and explicit: no build pipeline, no discovery magic.
// Add new embedded modules by extending this table.
static EMBEDDED: &[EmbeddedModule] = &[
    EmbeddedModule {
        path: b"/qjs/util.mjs",
        src: include_bytes!("../../app/util.mjs"),
        bytecode: include_bytes!(concat!(env!("OUT_DIR"), "/embedded_qjs/util.qjsc")),
    },
];

#[inline]
pub(crate) fn find(path: &[u8]) -> Option<&'static EmbeddedModule> {
    // Linear scan is fine at this scale.
    for m in EMBEDDED {
        if m.path == path {
            return Some(m);
        }
    }
    None
}
