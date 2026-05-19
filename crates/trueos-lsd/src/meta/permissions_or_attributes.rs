use crate::{
    color::{ColoredString, Colors},
    flags::Flags,
};

use super::Permissions;

#[derive(Clone, Debug)]
pub enum PermissionsOrAttributes {
    Permissions(Permissions),
}

impl PermissionsOrAttributes {
    pub fn render(&self, colors: &Colors, flags: &Flags) -> ColoredString {
        match self {
            PermissionsOrAttributes::Permissions(permissions) => permissions.render(colors, flags),
        }
    }
}
