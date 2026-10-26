//! The native loader's retained transcript, available after firmware exits.

#[allow(dead_code)]
#[path = "../tools/trueos-boot/src/bootlog.rs"]
mod wire;

pub(crate) fn text() -> Option<&'static str> {
    wire::text_from_blob(crate::limine::module_bytes_by_string(b"trueos.boot.log")?)
}

pub(crate) fn replay() {
    if let Some(text) = text() {
        crate::log_important!(target: "boot";
            "native-loader-log: begin\n{}native-loader-log: end\n", text);
    }
}
