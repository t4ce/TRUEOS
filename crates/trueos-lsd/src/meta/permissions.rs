use super::metadata::Metadata;
use crate::color::{ColoredString, Colors, Elem};
use crate::flags::{Flags, PermissionFlag};

#[derive(Default, Debug, PartialEq, Eq, Copy, Clone)]
pub struct Permissions {
    pub user_read: bool,
    pub user_write: bool,
    pub user_execute: bool,

    pub group_read: bool,
    pub group_write: bool,
    pub group_execute: bool,

    pub other_read: bool,
    pub other_write: bool,
    pub other_execute: bool,

    pub sticky: bool,
    pub setgid: bool,
    pub setuid: bool,
}

impl From<&Metadata> for Permissions {
    fn from(_: &Metadata) -> Self {
        Self::default()
    }
}

impl Permissions {
    fn bits_to_octal(r: bool, w: bool, x: bool) -> u8 {
        (r as u8) * 4 + (w as u8) * 2 + (x as u8)
    }

    pub fn render(&self, colors: &Colors, flags: &Flags) -> ColoredString {
        let bit = |bit, chr: &'static str, elem: &Elem| {
            if bit {
                colors.colorize(chr, elem)
            } else {
                colors.colorize('-', &Elem::NoAccess)
            }
        };

        let res = match flags.permission {
            PermissionFlag::Rwx => [
                // User permissions
                bit(self.user_read, "r", &Elem::Read),
                bit(self.user_write, "w", &Elem::Write),
                match (self.user_execute, self.setuid) {
                    (false, false) => colors.colorize('-', &Elem::NoAccess),
                    (true, false) => colors.colorize('x', &Elem::Exec),
                    (false, true) => colors.colorize('S', &Elem::ExecSticky),
                    (true, true) => colors.colorize('s', &Elem::ExecSticky),
                },
                // Group permissions
                bit(self.group_read, "r", &Elem::Read),
                bit(self.group_write, "w", &Elem::Write),
                match (self.group_execute, self.setgid) {
                    (false, false) => colors.colorize('-', &Elem::NoAccess),
                    (true, false) => colors.colorize('x', &Elem::Exec),
                    (false, true) => colors.colorize('S', &Elem::ExecSticky),
                    (true, true) => colors.colorize('s', &Elem::ExecSticky),
                },
                // Other permissions
                bit(self.other_read, "r", &Elem::Read),
                bit(self.other_write, "w", &Elem::Write),
                match (self.other_execute, self.sticky) {
                    (false, false) => colors.colorize('-', &Elem::NoAccess),
                    (true, false) => colors.colorize('x', &Elem::Exec),
                    (false, true) => colors.colorize('T', &Elem::ExecSticky),
                    (true, true) => colors.colorize('t', &Elem::ExecSticky),
                },
            ]
            .into_iter()
            // From the experiment, the maximum string size is 153 bytes
            .fold(String::with_capacity(160), |mut acc, x| {
                acc.push_str(&x.to_string());
                acc
            }),
            PermissionFlag::Octal => {
                let octals = [
                    Self::bits_to_octal(self.setuid, self.setgid, self.sticky),
                    Self::bits_to_octal(self.user_read, self.user_write, self.user_execute),
                    Self::bits_to_octal(self.group_read, self.group_write, self.group_execute),
                    Self::bits_to_octal(self.other_read, self.other_write, self.other_execute),
                ]
                .into_iter()
                .fold(String::with_capacity(4), |mut acc, x| {
                    acc.push(
                        char::from_digit(x as u32, 8)
                            .expect("octal value of permission should not be greater than 7"),
                    );
                    acc
                });

                colors.colorize(octals, &Elem::Octal).to_string()
            }
            // technically this should be an error, hmm
            PermissionFlag::Attributes => colors.colorize('-', &Elem::NoAccess).to_string(),
            PermissionFlag::Disable => colors.colorize('-', &Elem::NoAccess).to_string(),
        };

        ColoredString::new(Colors::default_style(), res)
    }

    pub fn is_executable(&self) -> bool {
        self.user_execute || self.group_execute || self.other_execute
    }
}

#[cfg(unix)]
#[cfg(test)]
mod test {
    use super::Flags;
    use super::{PermissionFlag, Permissions};
    use crate::color::{Colors, ThemeOption};
    use std::fs;
    use std::fs::File;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::tempdir;

    #[test]
    fn permission_rwx() {
        let tmp_dir = tempdir().expect("failed to create temp dir");

        // Create the file;
        let file_path = tmp_dir.path().join("file.txt");
        File::create(&file_path).expect("failed to create file");
        fs::set_permissions(&file_path, fs::Permissions::from_mode(0o655))
            .expect("unable to set permissions to file");
        let meta = file_path.metadata().expect("failed to get meta");

        let colors = Colors::new(ThemeOption::NoColor);
        let perms = Permissions::from(&meta);

        assert_eq!("rw-r-xr-x", perms.render(&colors, &Flags::default()).content());
    }

    #[test]
    fn permission_rwx2() {
        let tmp_dir = tempdir().expect("failed to create temp dir");

        // Create the file;
        let file_path = tmp_dir.path().join("file.txt");
        File::create(&file_path).expect("failed to create file");
        fs::set_permissions(&file_path, fs::Permissions::from_mode(0o777))
            .expect("unable to set permissions to file");
        let meta = file_path.metadata().expect("failed to get meta");

        let colors = Colors::new(ThemeOption::NoColor);
        let perms = Permissions::from(&meta);

        assert_eq!("rwxrwxrwx", perms.render(&colors, &Flags::default()).content());
    }

    #[test]
    fn permission_rwx_sticky() {
        let tmp_dir = tempdir().expect("failed to create temp dir");

        // Create the file;
        let file_path = tmp_dir.path().join("file.txt");
        File::create(&file_path).expect("failed to create file");
        fs::set_permissions(&file_path, fs::Permissions::from_mode(0o1777))
            .expect("unable to set permissions to file");

        let meta = file_path.metadata().expect("failed to get meta");

        let colors = Colors::new(ThemeOption::NoColor);
        let flags = Flags {
            permission: PermissionFlag::Rwx,
            ..Default::default()
        };
        let perms = Permissions::from(&meta);

        assert_eq!("rwxrwxrwt", perms.render(&colors, &flags).content());
    }

    #[test]
    fn permission_octal() {
        let tmp_dir = tempdir().expect("failed to create temp dir");

        // Create the file;
        let file_path = tmp_dir.path().join("file.txt");
        File::create(&file_path).expect("failed to create file");
        fs::set_permissions(&file_path, fs::Permissions::from_mode(0o655))
            .expect("unable to set permissions to file");
        let meta = file_path.metadata().expect("failed to get meta");

        let colors = Colors::new(ThemeOption::NoColor);
        let flags = Flags {
            permission: PermissionFlag::Octal,
            ..Default::default()
        };
        let perms = Permissions::from(&meta);

        assert_eq!("0655", perms.render(&colors, &flags).content());
    }

    #[test]
    fn permission_octal2() {
        let tmp_dir = tempdir().expect("failed to create temp dir");

        // Create the file;
        let file_path = tmp_dir.path().join("file.txt");
        File::create(&file_path).expect("failed to create file");
        fs::set_permissions(&file_path, fs::Permissions::from_mode(0o777))
            .expect("unable to set permissions to file");
        let meta = file_path.metadata().expect("failed to get meta");

        let colors = Colors::new(ThemeOption::NoColor);
        let flags = Flags {
            permission: PermissionFlag::Octal,
            ..Default::default()
        };
        let perms = Permissions::from(&meta);

        assert_eq!("0777", perms.render(&colors, &flags).content());
    }

    #[test]
    fn permission_octal_sticky() {
        let tmp_dir = tempdir().expect("failed to create temp dir");

        // Create the file;
        let file_path = tmp_dir.path().join("file.txt");
        File::create(&file_path).expect("failed to create file");
        fs::set_permissions(&file_path, fs::Permissions::from_mode(0o1777))
            .expect("unable to set permissions to file");

        let meta = file_path.metadata().expect("failed to get meta");

        let colors = Colors::new(ThemeOption::NoColor);
        let flags = Flags {
            permission: PermissionFlag::Octal,
            ..Default::default()
        };
        let perms = Permissions::from(&meta);

        assert_eq!("1777", perms.render(&colors, &flags).content());
    }

    #[test]
    fn permission_disable() {
        let tmp_dir = tempdir().expect("failed to create temp dir");

        // Create the file;
        let file_path = tmp_dir.path().join("file.txt");
        File::create(&file_path).expect("failed to create file");
        fs::set_permissions(&file_path, fs::Permissions::from_mode(0o655))
            .expect("unable to set permissions to file");
        let meta = file_path.metadata().expect("failed to get meta");

        let colors = Colors::new(ThemeOption::NoColor);
        let flags = Flags {
            permission: PermissionFlag::Disable,
            ..Default::default()
        };
        let perms = Permissions::from(&meta);

        assert_eq!("-", perms.render(&colors, &flags).content());
    }
}
