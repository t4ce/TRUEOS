//! Version-one launch payload, with an optional destination after the script.
//! Legacy callers send `app NUL script`; routed callers append `NUL destination`.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Destination { CurrentShell, NewShell3, Headless }

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Request<'a> {
    pub app: &'a str,
    pub script: &'a str,
    pub destination: Destination,
}

pub(super) fn parse(payload: &[u8]) -> Result<Request<'_>, i32> {
    let mut fields = payload.split(|byte| *byte == 0);
    let app = core::str::from_utf8(fields.next().ok_or(-1)?).map_err(|_| -1)?;
    let script = core::str::from_utf8(fields.next().ok_or(-1)?).map_err(|_| -1)?;
    let destination = match fields.next() {
        None => Destination::CurrentShell,
        Some(b"sh3") => Destination::NewShell3,
        Some(b"headless") => Destination::Headless,
        _ => return Err(-1),
    };
    if app.is_empty() || fields.next().is_some() { return Err(-1); }
    Ok(Request { app, script, destination })
}
