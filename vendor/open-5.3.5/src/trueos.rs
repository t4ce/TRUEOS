use std::{
    ffi::OsStr,
    process::Command,
};

pub fn commands<T: AsRef<OsStr>>(path: T) -> Vec<Command> {
    vec![with_command(path, "trueos-open")]
}

pub fn with_command<T: AsRef<OsStr>>(path: T, app: impl Into<String>) -> Command {
    let mut cmd = Command::new(app.into());
    cmd.arg(path.as_ref());
    cmd
}
