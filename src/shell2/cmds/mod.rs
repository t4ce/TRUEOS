use alloc::string::String as AllocString;

pub(crate) mod set;
pub(crate) mod update;

pub(crate) fn command_registry_json() -> AllocString {
    AllocString::from(
        "{\"version\":1,\"commands\":[{\"name\":\"update\",\"mode\":\"cmd\",\"summary\":\"Download the latest TrueOS archive and install it onto the mounted TRUEOSFS root disk.\"}]}",
    )
}
