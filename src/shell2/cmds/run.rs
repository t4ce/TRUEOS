use alloc::string::String;
use alloc::vec::Vec;
use core::str::SplitWhitespace;

use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};

use super::super::{
    MatrixTarget, ShellBackend2, line_width_for_backend, matrix_target_for_backend,
    print_matrix_target_line, print_shell_line, set_matrix_target_active,
};
use super::tlb_helper::TlbTable;
use crate::shell2::shell2_cmd::ParseOutcome;

const TABLE_HEADERS: &[&str; 3] = &["id", "module", "source"];
const TAPP_HEADER_LEN: usize = 24;
const ELF64_HEADER_LEN: usize = 64;
const ELF64_SECTION_HEADER_LEN: usize = 64;
const ELF64_SYM_LEN: usize = 24;
const SHT_SYMTAB: u32 = 2;
const SHN_UNDEF: u16 = 0;
const STB_GLOBAL: u8 = 1;
const STB_WEAK: u8 = 2;

struct TappModule<'a> {
    version: u16,
    flags: u16,
    entry: u64,
    raw_payload_len: usize,
    payload: &'a [u8],
}

struct ElfImport<'a> {
    name: &'a str,
    resolved_addr: Option<usize>,
}

fn print_usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "run: usage `run` or `run <id> [args...]`");
}

fn le_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let raw: [u8; 2] = bytes.get(offset..offset + 2)?.try_into().ok()?;
    Some(u16::from_le_bytes(raw))
}

fn le_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let raw: [u8; 4] = bytes.get(offset..offset + 4)?.try_into().ok()?;
    Some(u32::from_le_bytes(raw))
}

fn le_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    let raw: [u8; 8] = bytes.get(offset..offset + 8)?.try_into().ok()?;
    Some(u64::from_le_bytes(raw))
}

fn elf_imports<'a>(bytes: &'a [u8]) -> Result<Vec<ElfImport<'a>>, &'static str> {
    if bytes.len() < ELF64_HEADER_LEN {
        return Err("ELF header truncated");
    }
    if bytes.get(0..4) != Some(b"\x7fELF") {
        return Err("payload is not ELF");
    }
    if bytes.get(4).copied() != Some(2) || bytes.get(5).copied() != Some(1) {
        return Err("unsupported ELF class/data");
    }

    let shoff = le_u64(bytes, 40).ok_or("ELF header truncated")? as usize;
    let shentsize = le_u16(bytes, 58).ok_or("ELF header truncated")? as usize;
    let shnum = le_u16(bytes, 60).ok_or("ELF header truncated")? as usize;
    if shentsize != ELF64_SECTION_HEADER_LEN {
        return Err("unsupported ELF section header size");
    }

    let mut imports = Vec::new();
    for section_index in 0..shnum {
        let shdr_off = shoff
            .checked_add(
                section_index
                    .checked_mul(shentsize)
                    .ok_or("ELF section header overflow")?,
            )
            .ok_or("ELF section header overflow")?;
        let shdr = bytes
            .get(shdr_off..shdr_off + ELF64_SECTION_HEADER_LEN)
            .ok_or("ELF section header truncated")?;

        let section_type = le_u32(shdr, 4).ok_or("ELF section header truncated")?;
        if section_type != SHT_SYMTAB {
            continue;
        }

        let sym_off = le_u64(shdr, 24).ok_or("ELF section header truncated")? as usize;
        let sym_size = le_u64(shdr, 32).ok_or("ELF section header truncated")? as usize;
        let link = le_u32(shdr, 40).ok_or("ELF section header truncated")? as usize;
        let entsize = le_u64(shdr, 56).ok_or("ELF section header truncated")? as usize;
        if entsize != ELF64_SYM_LEN {
            return Err("unsupported ELF symbol size");
        }

        let str_shdr_off = shoff
            .checked_add(link.checked_mul(shentsize).ok_or("ELF string table overflow")?)
            .ok_or("ELF string table overflow")?;
        let str_shdr = bytes
            .get(str_shdr_off..str_shdr_off + ELF64_SECTION_HEADER_LEN)
            .ok_or("ELF string table truncated")?;
        let str_off = le_u64(str_shdr, 24).ok_or("ELF string table truncated")? as usize;
        let str_size = le_u64(str_shdr, 32).ok_or("ELF string table truncated")? as usize;
        let strtab = bytes
            .get(str_off..str_off + str_size)
            .ok_or("ELF string table truncated")?;

        let symtab = bytes
            .get(sym_off..sym_off + sym_size)
            .ok_or("ELF symbol table truncated")?;

        for sym in symtab.chunks_exact(ELF64_SYM_LEN) {
            let name_off = le_u32(sym, 0).ok_or("ELF symbol truncated")? as usize;
            let info = *sym.get(4).ok_or("ELF symbol truncated")?;
            let shndx = le_u16(sym, 6).ok_or("ELF symbol truncated")?;
            let bind = info >> 4;
            if shndx != SHN_UNDEF || !(bind == STB_GLOBAL || bind == STB_WEAK) {
                continue;
            }

            let name_bytes = strtab
                .get(name_off..)
                .ok_or("ELF symbol name truncated")?;
            let name_len = name_bytes
                .iter()
                .position(|&b| b == 0)
                .ok_or("ELF symbol name unterminated")?;
            if name_len == 0 {
                continue;
            }
            let name = core::str::from_utf8(&name_bytes[..name_len])
                .map_err(|_| "ELF symbol name is not UTF-8")?;
            imports.push(ElfImport {
                name,
                resolved_addr: resolve_import(name),
            });
        }
    }

    imports.sort_by(|a, b| a.name.cmp(b.name));
    imports.dedup_by(|a, b| a.name == b.name);
    Ok(imports)
}

fn resolve_import(name: &str) -> Option<usize> {
    match name {
        "trueos_cabi_alloc" => {
            Some(crate::surface::io::cabi::trueos_cabi_alloc as *const () as usize)
        }
        "trueos_cabi_free" => {
            Some(crate::surface::io::cabi::trueos_cabi_free as *const () as usize)
        }
        "trueos_cabi_realloc" => {
            Some(crate::surface::io::cabi::trueos_cabi_realloc as *const () as usize)
        }
        "trueos_cabi_write" => {
            Some(crate::surface::io::cabi::trueos_cabi_write as *const () as usize)
        }
        "memcpy" => Some(trueos_qjs::trueos_shims::memcpy as *const () as usize),
        "memset" => Some(trueos_qjs::trueos_shims::memset as *const () as usize),
        "memcmp" => Some(trueos_qjs::trueos_shims::memcmp as *const () as usize),
        "strlen" => Some(trueos_qjs::trueos_shims::strlen as *const () as usize),
        _ => None,
    }
}

fn parse_tapp(bytes: &[u8]) -> Result<TappModule<'_>, &'static str> {
    if bytes.len() < TAPP_HEADER_LEN {
        return Err("module truncated");
    }
    if bytes.get(0..4) != Some(b"TAPP") {
        return Err("bad TAPP magic");
    }

    let version = le_u16(bytes, 4).ok_or("module truncated")?;
    let flags = le_u16(bytes, 6).ok_or("module truncated")?;
    let entry = le_u64(bytes, 8).ok_or("module truncated")?;
    let payload_len = le_u32(bytes, 16).ok_or("module truncated")? as usize;
    let raw_payload_len = le_u32(bytes, 20).ok_or("module truncated")? as usize;
    let payload_end = TAPP_HEADER_LEN
        .checked_add(payload_len)
        .ok_or("module too large")?;
    let payload = bytes
        .get(TAPP_HEADER_LEN..payload_end)
        .ok_or("payload truncated")?;

    Ok(TappModule {
        version,
        flags,
        entry,
        raw_payload_len,
        payload,
    })
}

fn unpack_tapp(module: &TappModule<'_>) -> Result<Vec<u8>, &'static str> {
    match module.flags {
        1 => Ok(module.payload.to_vec()),
        2 => crate::z7::extract_single_file_to_vec(module.payload)
            .map_err(|_| "7z payload decode failed"),
        _ => Err("unsupported TAPP payload flags"),
    }
}

fn root_archives() -> Result<Vec<String>, &'static str> {
    let Some(disk) = crate::v::fs::trueosfs::primary_root_handle() else {
        return Err("no TRUEOSFS root mounted");
    };

    let listing = crate::wait::spawn_and_wait_local(async move {
        crate::v::fs::trueosfs::list_dir_async(disk, "").await
    })
    .map_err(|_| "root listing failed")?
    .ok_or("root is not TRUEOSFS")?;

    let mut out = listing
        .lines()
        .map(str::trim)
        .filter(|name| name.ends_with(".tapp"))
        .map(String::from)
        .collect::<Vec<_>>();
    out.sort();
    Ok(out)
}

fn print_archive_table(io: &'static dyn ShellBackend2, archives: &[String]) {
    let table = TlbTable::with_width(TABLE_HEADERS, line_width_for_backend(io).saturating_sub(2));
    table.emit_header(|text| print_shell_line(io, text));
    for (idx, archive) in archives.iter().enumerate() {
        let id = alloc::format!("{}", idx + 1);
        let row = [id.as_str(), archive.as_str(), "TRUEOSFS root"];
        table.emit_row(&row, |text| print_shell_line(io, text));
    }
    table.emit_footer(|text| print_shell_line(io, text));
}

pub(crate) fn submit_run(
    io: &'static dyn ShellBackend2,
    archive: String,
    app_args: Vec<String>,
) {
    let Some(worker_spawner) = trueos_qjs::workers::pick_background_spawner() else {
        print_shell_line(io, "run: no background worker spawner available");
        return;
    };

    let target = matrix_target_for_backend(io);
    print_matrix_target_line(
        &target,
        alloc::format!("run: queued {}", archive.as_str()).as_str(),
    );
    set_matrix_target_active(&target, true);

    if worker_spawner
        .spawn(run_command_task(target.clone(), archive, app_args))
        .is_err()
    {
        set_matrix_target_active(&target, false);
        print_shell_line(io, "run: spawn failed");
    }
}

#[embassy_executor::task(pool_size = 2)]
async fn run_command_task(target: MatrixTarget, archive: String, app_args: Vec<String>) {
    let task_target = target.clone();
    async move {
        Timer::after(EmbassyDuration::from_millis(1)).await;

        let log = |line: &str| {
            print_matrix_target_line(&task_target, line);
        };

        log(alloc::format!("run: worker start module={}", archive.as_str()).as_str());

        let module_bytes = match crate::surface::io::kfs::read_file(archive.as_str()) {
            Ok(bytes) => bytes,
            Err(_) => {
                log("run: failed to read selected module from TRUEOSFS");
                return;
            }
        };
        log(alloc::format!("run: module bytes={}", module_bytes.len()).as_str());

        let module = match parse_tapp(module_bytes.as_slice()) {
            Ok(module) => module,
            Err(err) => {
                log(alloc::format!("run: {}", err).as_str());
                return;
            }
        };
        let unpacked = match unpack_tapp(&module) {
            Ok(bytes) => bytes,
            Err(err) => {
                log(alloc::format!("run: {}", err).as_str());
                return;
            }
        };

        log(
            alloc::format!(
                "run: module={} version={} flags={} entry=0x{:x}",
                archive, module.version, module.flags, module.entry
            )
            .as_str(),
        );
        log(
            alloc::format!(
                "run: payload compressed={} unpacked={} header_raw={}",
                module.payload.len(),
                unpacked.len(),
                module.raw_payload_len
            )
            .as_str(),
        );
        if unpacked.len() != module.raw_payload_len {
            log("run: warning: unpacked payload size does not match header_raw");
        }
        if unpacked.starts_with(b"\x7fELF") {
            log("run: unpacked payload looks like ELF");
        } else {
            log("run: unpacked payload does not look like ELF");
        }
        if unpacked.starts_with(b"\x7fELF") {
            match elf_imports(unpacked.as_slice()) {
                Ok(imports) => {
                    if imports.is_empty() {
                        log("run: ELF imports=0");
                    } else {
                        let resolved = imports
                            .iter()
                            .filter(|import| import.resolved_addr.is_some())
                            .count();
                        log(
                            alloc::format!(
                                "run: ELF imports={} resolved={}",
                                imports.len(),
                                resolved
                            )
                            .as_str(),
                        );
                        for import in imports.iter() {
                            match import.resolved_addr {
                                Some(addr) => log(
                                    alloc::format!(
                                        "run: import {} -> 0x{:x}",
                                        import.name, addr
                                    )
                                    .as_str(),
                                ),
                                None => log(
                                    alloc::format!(
                                        "run: import {} -> unresolved",
                                        import.name
                                    )
                                    .as_str(),
                                ),
                            }
                        }
                    }
                }
                Err(err) => {
                    log(alloc::format!("run: ELF import scan failed: {}", err).as_str());
                }
            }
        }
        if !app_args.is_empty() {
            log(
                alloc::format!(
                    "run: args staged={} (execution still not wired)",
                    app_args.len()
                )
                .as_str(),
            );
        }
    }
    .await;
    set_matrix_target_active(&target, false);
}

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    let _ = spawner;
    let archives = match root_archives() {
        Ok(archives) => archives,
        Err(err) => {
            print_shell_line(io, alloc::format!("run: {}", err).as_str());
            return ParseOutcome::Handled;
        }
    };

    let Some(id_text) = args.next() else {
        if archives.is_empty() {
            print_shell_line(io, "run: no .tapp modules in TRUEOSFS root");
            return ParseOutcome::Handled;
        }
        print_archive_table(io, archives.as_slice());
        return ParseOutcome::Handled;
    };

    let archive_index = match id_text.parse::<usize>() {
        Ok(id) if id > 0 => id - 1,
        _ => {
            print_usage(io);
            return ParseOutcome::Handled;
        }
    };

    let Some(archive) = archives.get(archive_index) else {
        print_shell_line(io, "run: unknown archive id");
        print_archive_table(io, archives.as_slice());
        return ParseOutcome::Handled;
    };

    let _ = spawner;
    let app_args = args.map(String::from).collect::<Vec<_>>();
    submit_run(io, archive.clone(), app_args);

    ParseOutcome::Handled
}
