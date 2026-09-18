use alloc::{string::String, vec::Vec};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LauncherImport {
    pub(crate) id: u32,
    pub(crate) module: String,
    pub(crate) symbol: String,
    pub(crate) iat_rva: u32,
}

pub(crate) fn patch(
    image: &mut [u8],
    imports: &[LauncherImport],
    thunks: &mut [u8],
) -> Result<(), &'static str> {
    for import in imports {
        let thunk = super::thunk32::address(import.id).ok_or("wc3 thunk address overflow")?;
        let slot = usize::try_from(import.iat_rva).map_err(|_| "wc3 IAT rva")?;
        let output = thunks
            .get_mut(
                usize::try_from(import.id)
                    .map_err(|_| "wc3 import id")?
                    .saturating_mul(super::thunk32::THUNK_BYTES)..,
            )
            .ok_or("wc3 thunk output range")?;
        let kind = if is_get_version(import) {
            super::thunk32::Kind::Return
        } else if is_heap_create(import) {
            super::thunk32::Kind::HeapCreate
        } else if is_get_version_ex_a(import) {
            super::thunk32::Kind::GetVersionExA
        } else if is_initialize_critical_section(import) {
            super::thunk32::Kind::InitializeCriticalSection
        } else if is_enter_critical_section(import) {
            super::thunk32::Kind::EnterCriticalSection
        } else if is_leave_critical_section(import) {
            super::thunk32::Kind::LeaveCriticalSection
        } else if is_tls_alloc(import) {
            super::thunk32::Kind::TlsAlloc
        } else if is_heap_alloc(import) {
            super::thunk32::Kind::HeapAlloc
        } else if is_heap_free(import) {
            super::thunk32::Kind::HeapFree
        } else if is_create_event_a(import) {
            super::thunk32::Kind::CreateEventA
        } else if is_get_last_error(import) {
            super::thunk32::Kind::GetLastError
        } else if is_close_handle(import) {
            super::thunk32::Kind::CloseHandle
        } else if is_get_tick_count(import) {
            super::thunk32::Kind::GetTickCount
        } else if is_tls_set_value(import) {
            super::thunk32::Kind::TlsSetValue
        } else if is_get_current_thread_id(import) {
            super::thunk32::Kind::GetCurrentThreadId
        } else if is_get_startup_info_a(import) {
            super::thunk32::Kind::GetStartupInfoA
        } else if is_get_module_file_name_a(import) {
            super::thunk32::Kind::GetModuleFileNameA
        } else if is_get_module_handle_a(import) {
            super::thunk32::Kind::GetModuleHandleA
        } else if is_register_class_a(import) {
            super::thunk32::Kind::RegisterClassA
        } else if is_get_desktop_window(import) {
            super::thunk32::Kind::GetDesktopWindow
        } else if is_get_client_rect(import) {
            super::thunk32::Kind::GetClientRect
        } else if is_create_window_ex_a(import) {
            super::thunk32::Kind::CreateWindowExA
        } else if is_show_window(import) {
            super::thunk32::Kind::ShowWindow
        } else if is_update_window(import) {
            super::thunk32::Kind::UpdateWindow
        } else if is_peek_message_a(import) {
            super::thunk32::Kind::PeekMessageA
        } else if is_set_focus(import) {
            super::thunk32::Kind::SetFocus
        } else if is_create_thread(import) {
            super::thunk32::Kind::CreateThread
        } else if is_resume_thread(import) {
            super::thunk32::Kind::ResumeThread
        } else if is_get_std_handle(import) {
            super::thunk32::Kind::GetStdHandle
        } else if is_get_file_type(import) {
            super::thunk32::Kind::GetFileType
        } else if is_set_handle_count(import) {
            super::thunk32::Kind::SetHandleCount
        } else if is_get_command_line_a(import) {
            super::thunk32::Kind::GetCommandLineA
        } else if is_get_environment_strings_w(import) {
            super::thunk32::Kind::GetEnvironmentStringsW
        } else if is_get_environment_strings_a(import) {
            super::thunk32::Kind::GetEnvironmentStringsA
        } else if is_free_environment_strings_a(import) {
            super::thunk32::Kind::FreeEnvironmentStringsA
        } else if is_get_acp(import) {
            super::thunk32::Kind::GetACP
        } else if is_get_cp_info(import) {
            super::thunk32::Kind::GetCPInfo
        } else if is_get_string_type_w(import) {
            super::thunk32::Kind::GetStringTypeW
        } else if is_lc_map_string_w(import) {
            super::thunk32::Kind::LCMapStringW
        } else if is_wide_char_to_multi_byte(import) {
            super::thunk32::Kind::WideCharToMultiByte
        } else if is_multi_byte_to_wide_char(import) {
            super::thunk32::Kind::MultiByteToWideChar
        } else {
            super::thunk32::Kind::Stop
        };
        super::thunk32::write(import.id, kind, output)?;
        let iat = image
            .get_mut(slot..slot.checked_add(4).ok_or("wc3 IAT overflow")?)
            .ok_or("wc3 IAT outside image")?;
        iat.copy_from_slice(&thunk.to_le_bytes());
    }
    Ok(())
}

pub(crate) fn find_get_version(imports: &[LauncherImport]) -> bool {
    imports
        .iter()
        .filter(|import| is_get_version(import))
        .count()
        == 1
}

pub(crate) fn is_get_version(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("KERNEL32.dll") && import.symbol == "GetVersion"
}

pub(crate) fn find_heap_create(imports: &[LauncherImport]) -> bool {
    imports
        .iter()
        .filter(|import| is_heap_create(import))
        .count()
        == 1
}

pub(crate) fn is_heap_create(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("KERNEL32.dll") && import.symbol == "HeapCreate"
}

pub(crate) fn find_get_version_ex_a(imports: &[LauncherImport]) -> bool {
    imports
        .iter()
        .filter(|import| is_get_version_ex_a(import))
        .count()
        == 1
}

pub(crate) fn is_get_version_ex_a(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("KERNEL32.dll") && import.symbol == "GetVersionExA"
}

pub(crate) fn find_initialize_critical_section(imports: &[LauncherImport]) -> bool {
    imports
        .iter()
        .filter(|import| is_initialize_critical_section(import))
        .count()
        == 1
}

pub(crate) fn is_initialize_critical_section(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("KERNEL32.dll")
        && import.symbol == "InitializeCriticalSection"
}

pub(crate) fn is_enter_critical_section(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("KERNEL32.dll") && import.symbol == "EnterCriticalSection"
}

pub(crate) fn is_leave_critical_section(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("KERNEL32.dll") && import.symbol == "LeaveCriticalSection"
}

pub(crate) fn find_tls_alloc(imports: &[LauncherImport]) -> bool {
    imports.iter().filter(|import| is_tls_alloc(import)).count() == 1
}

pub(crate) fn is_tls_alloc(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("KERNEL32.dll") && import.symbol == "TlsAlloc"
}

pub(crate) fn find_heap_alloc(imports: &[LauncherImport]) -> bool {
    imports
        .iter()
        .filter(|import| is_heap_alloc(import))
        .count()
        == 1
}

pub(crate) fn is_heap_alloc(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("KERNEL32.dll") && import.symbol == "HeapAlloc"
}

pub(crate) fn is_heap_free(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("KERNEL32.dll") && import.symbol == "HeapFree"
}

pub(crate) fn is_create_event_a(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("KERNEL32.dll") && import.symbol == "CreateEventA"
}

pub(crate) fn is_get_last_error(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("KERNEL32.dll") && import.symbol == "GetLastError"
}

pub(crate) fn is_close_handle(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("KERNEL32.dll") && import.symbol == "CloseHandle"
}

pub(crate) fn find_tls_set_value(imports: &[LauncherImport]) -> bool {
    imports
        .iter()
        .filter(|import| is_tls_set_value(import))
        .count()
        == 1
}

pub(crate) fn is_tls_set_value(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("KERNEL32.dll") && import.symbol == "TlsSetValue"
}

pub(crate) fn find_get_current_thread_id(imports: &[LauncherImport]) -> bool {
    imports
        .iter()
        .filter(|import| is_get_current_thread_id(import))
        .count()
        == 1
}

pub(crate) fn is_get_current_thread_id(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("KERNEL32.dll") && import.symbol == "GetCurrentThreadId"
}

pub(crate) fn find_get_startup_info_a(imports: &[LauncherImport]) -> bool {
    imports
        .iter()
        .filter(|import| is_get_startup_info_a(import))
        .count()
        == 1
}

pub(crate) fn is_get_startup_info_a(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("KERNEL32.dll") && import.symbol == "GetStartupInfoA"
}

pub(crate) fn is_get_module_file_name_a(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("KERNEL32.dll") && import.symbol == "GetModuleFileNameA"
}

pub(crate) fn is_get_module_handle_a(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("KERNEL32.dll") && import.symbol == "GetModuleHandleA"
}

pub(crate) fn is_register_class_a(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("USER32.dll") && import.symbol == "RegisterClassA"
}

pub(crate) fn is_get_desktop_window(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("USER32.dll") && import.symbol == "GetDesktopWindow"
}

pub(crate) fn is_get_client_rect(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("USER32.dll") && import.symbol == "GetClientRect"
}

pub(crate) fn is_create_window_ex_a(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("USER32.dll") && import.symbol == "CreateWindowExA"
}

pub(crate) fn is_show_window(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("USER32.dll") && import.symbol == "ShowWindow"
}

pub(crate) fn is_update_window(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("USER32.dll") && import.symbol == "UpdateWindow"
}

pub(crate) fn is_peek_message_a(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("USER32.dll") && import.symbol == "PeekMessageA"
}

pub(crate) fn is_set_focus(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("USER32.dll") && import.symbol == "SetFocus"
}

pub(crate) fn is_create_thread(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("KERNEL32.dll") && import.symbol == "CreateThread"
}

pub(crate) fn is_resume_thread(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("KERNEL32.dll") && import.symbol == "ResumeThread"
}

pub(crate) fn find_get_std_handle(imports: &[LauncherImport]) -> bool {
    imports
        .iter()
        .filter(|import| is_get_std_handle(import))
        .count()
        == 1
}

pub(crate) fn is_get_std_handle(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("KERNEL32.dll") && import.symbol == "GetStdHandle"
}

pub(crate) fn find_get_file_type(imports: &[LauncherImport]) -> bool {
    imports
        .iter()
        .filter(|import| is_get_file_type(import))
        .count()
        == 1
}

pub(crate) fn is_get_file_type(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("KERNEL32.dll") && import.symbol == "GetFileType"
}

pub(crate) fn find_set_handle_count(imports: &[LauncherImport]) -> bool {
    imports
        .iter()
        .filter(|import| is_set_handle_count(import))
        .count()
        == 1
}

pub(crate) fn is_set_handle_count(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("KERNEL32.dll") && import.symbol == "SetHandleCount"
}

pub(crate) fn find_get_command_line_a(imports: &[LauncherImport]) -> bool {
    imports
        .iter()
        .filter(|import| is_get_command_line_a(import))
        .count()
        == 1
}

pub(crate) fn is_get_command_line_a(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("KERNEL32.dll") && import.symbol == "GetCommandLineA"
}

pub(crate) fn find_get_environment_strings_w(imports: &[LauncherImport]) -> bool {
    imports
        .iter()
        .filter(|import| is_get_environment_strings_w(import))
        .count()
        == 1
}

pub(crate) fn is_get_environment_strings_w(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("KERNEL32.dll") && import.symbol == "GetEnvironmentStringsW"
}

pub(crate) fn find_get_environment_strings_a(imports: &[LauncherImport]) -> bool {
    imports
        .iter()
        .filter(|import| is_get_environment_strings_a(import))
        .count()
        == 1
}

pub(crate) fn is_get_environment_strings_a(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("KERNEL32.dll") && import.symbol == "GetEnvironmentStrings"
}

pub(crate) fn find_free_environment_strings_a(imports: &[LauncherImport]) -> bool {
    imports
        .iter()
        .filter(|import| is_free_environment_strings_a(import))
        .count()
        == 1
}

pub(crate) fn is_free_environment_strings_a(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("KERNEL32.dll") && import.symbol == "FreeEnvironmentStringsA"
}

pub(crate) fn find_get_acp(imports: &[LauncherImport]) -> bool {
    imports.iter().filter(|import| is_get_acp(import)).count() == 1
}

pub(crate) fn is_get_acp(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("KERNEL32.dll") && import.symbol == "GetACP"
}

pub(crate) fn find_get_cp_info(imports: &[LauncherImport]) -> bool {
    imports
        .iter()
        .filter(|import| is_get_cp_info(import))
        .count()
        == 1
}

pub(crate) fn is_get_cp_info(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("KERNEL32.dll") && import.symbol == "GetCPInfo"
}

pub(crate) fn is_get_string_type_w(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("KERNEL32.dll") && import.symbol == "GetStringTypeW"
}

pub(crate) fn is_multi_byte_to_wide_char(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("KERNEL32.dll") && import.symbol == "MultiByteToWideChar"
}

pub(crate) fn is_lc_map_string_w(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("KERNEL32.dll") && import.symbol == "LCMapStringW"
}

pub(crate) fn is_wide_char_to_multi_byte(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("KERNEL32.dll") && import.symbol == "WideCharToMultiByte"
}

pub(crate) fn is_get_tick_count(import: &LauncherImport) -> bool {
    import.module.eq_ignore_ascii_case("KERNEL32.dll") && import.symbol == "GetTickCount"
}

pub(crate) fn new_table() -> Vec<LauncherImport> {
    Vec::new()
}
