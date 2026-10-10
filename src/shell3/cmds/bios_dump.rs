use alloc::string::String;
use trueos_executor::Spawner;

use super::super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_line,
    set_matrix_target_active,
};

pub(crate) const BIOS_DUMP_FILE_PATH: &str = "trueos/pci/bios.txt";

#[trueos_executor::task(pool_size = 1)]
async fn dump_task(target: MatrixTarget) {
    let mut out = String::new();
    super::bios_tlb_dump::append_dump(&mut out);

    print_matrix_target_line(
        &target,
        alloc::format!("Writing {} bytes to {}...", out.len(), BIOS_DUMP_FILE_PATH).as_str(),
    );

    let Some(handle) = crate::r::fs::trueosfs::primary_root_handle() else {
        print_matrix_target_line(&target, "bios dump: filesystem not ready");
        set_matrix_target_active(&target, false);
        return;
    };

    let bytes = out.into_bytes();
    match crate::r::fs::trueosfs::file_in_typed_async(
        handle,
        BIOS_DUMP_FILE_PATH,
        &bytes,
        infer::ContentTypeId::UTF8_TEXT,
    )
    .await
    {
        Ok(true) => print_matrix_target_line(
            &target,
            alloc::format!("bios dump: wrote {} bytes to {}", bytes.len(), BIOS_DUMP_FILE_PATH)
                .as_str(),
        ),
        Ok(false) => print_matrix_target_line(&target, "bios dump: write failed"),
        Err(error) => print_matrix_target_line(
            &target,
            alloc::format!("bios dump: write failed: {:?}", error).as_str(),
        ),
    }
    set_matrix_target_active(&target, false);
}

pub(crate) fn start(spawner: &Spawner, io: &'static dyn ShellBackend2) {
    let target = matrix_target_for_backend(io);
    set_matrix_target_active(&target, true);
    match dump_task(target.clone()) {
        Ok(token) => spawner.spawn(token),
        Err(_) => {
            set_matrix_target_active(&target, false);
            print_matrix_target_line(&target, "bios dump: task unavailable");
        }
    }
}
