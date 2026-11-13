//! Image viewer admission for Blueprint callers, independent of terminals.
use alloc::{string::String, vec::Vec};

fn sources(payload: &[u8]) -> Result<Vec<String>, i32> {
    if payload.is_empty() || payload.len() > trueos_vm::vmcall::PAYLOAD_CAP {
        return Err(-1);
    }
    // Resolve ordinary callers through their own app/common view. Presenting
    // an image does not require permission to name the entire filesystem.
    // Without either view, fail closed rather than using the unrooted fallback.
    if !crate::r::io::env::trueosfs_scope_granted()
        && crate::r::io::env::current_app_fs_root().is_none()
    {
        return Err(-13);
    }
    let mut paths = Vec::new();
    for path in payload.split(|byte| *byte == 0) {
        if paths.len() == 32 { return Err(-1); }
        let path = core::str::from_utf8(path).map_err(|_| -1)?;
        crate::r::path::FsPath::parse(path, false).map_err(|_| -1)?;
        let path = crate::r::io::env::resolve_fs_path(path, false).ok_or(-1)?;
        paths.push(path);
    }
    Ok(paths)
}

pub(crate) fn enqueue(vm: u8, payload: &[u8]) -> i32 {
    let paths = match crate::hv::with_guest_broker_context(vm, || sources(payload)) {
        Ok(paths) => paths,
        Err(code) => return code,
    };
    let Some(worker) = crate::workers::pick_background_spawner() else { return -3; };
    let target = crate::hv::blueprint_console_target(vm);
    let access_account = crate::access_key_db::principal_for_vm(vm);
    match launch_task(paths, target, access_account) {
        Ok(task) => { worker.spawn(task); 0 }
        Err(_) => -16,
    }
}

#[trueos_executor::task(pool_size = 4)]
async fn launch_task(paths: Vec<String>, target: Option<crate::shell3::MatrixTarget>, access_account: Option<trueos_crypto::AccountId>) {
    let result = (|| {
        let bytes = crate::app_db::get("img.bp")?
            .ok_or_else(|| String::from("img.bp is missing from AppDB"))?;
        let mut request = crate::hv::launcher::LaunchRequest::new("img.bp".into(), bytes);
        request.args = paths;
        request.access_account = access_account;
        request.script = Some("fs-scope trueosfs\n".into());
        crate::hv::launcher::enqueue(request, None).map(|_| ())
    })();
    if let Err(error) = result {
        crate::log_os::blueprint_important_line(format_args!("img: {error}\n"));
        if let Some(target) = target {
            crate::shell3::matrix_target_print_line(&target, &alloc::format!("SHOW FAILED · {error}"));
        }
    }
}
