#!/usr/bin/env python3
"""Select byte-oriented TRUEOS std environment, args, and standard streams.

This supplemental installer is deliberately separate from the shared thread
recipe. Call it before fingerprinting/compiling a private sysroot. It never
changes the installed Rust toolchain or another builder's in-use private tree.
"""

import argparse
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MODULES = {
    "env": "trueos_env.rs",
    "args": "trueos_args.rs",
    "stdio": "trueos_stdio.rs",
    "io/is_terminal": "trueos_is_terminal.rs",
    "paths": "trueos_paths.rs",
}


def replace_once(source, needle, replacement, path):
    if replacement in source:
        return source
    if source.count(needle) != 1:
        raise ValueError(f"{path}: missing or duplicate native adaptation marker {needle[:80]!r}")
    return source.replace(needle, replacement, 1)


def add_process_and_exit(changes, system):
    def edit(relative, needle, replacement):
        path = system / relative
        source = changes.get(path, path.read_text())
        changes[path] = replace_once(source, needle, replacement, path)

    # Keep std's public Unix command-configuration surface, but select its
    # existing Unsupported execution backend before descriptors/pre_exec can
    # have effects. Native Shell3 sessions use their own explicit transport.
    edit("pal/unix/mod.rs",
         '#[cfg(any(target_os = "espidf", target_os = "horizon", target_os = "vita", target_os = "nuttx"))]\npub mod unsupported {',
         '#[cfg(any(target_os = "trueos", target_os = "espidf", target_os = "horizon", target_os = "vita", target_os = "nuttx"))]\npub mod unsupported {')
    edit("process/unix/mod.rs",
         'any(target_os = "espidf", target_os = "horizon", target_os = "vita", target_os = "nuttx") => {',
         'any(target_os = "trueos", target_os = "espidf", target_os = "horizon", target_os = "vita", target_os = "nuttx") => {')
    path = system / "process/mod.rs"
    source = changes.get(path, path.read_text())
    for _ in range(2):
        # Both positive/negative cfgs on output must route to imp::output.
        needle = '            target_os = "nuttx"\n'
        replacement = '            target_os = "nuttx",\n            target_os = "trueos"\n'
        if source.count(replacement) == 2:
            break
        if needle not in source:
            raise ValueError(f"{path}: missing std output platform exclusion")
        source = source.replace(needle, replacement, 1)
    changes[path] = source
    common = system / "process/unix/common.rs"
    source = changes.get(common, common.read_text())
    for name, selector in [("getpid", 0), ("getppid", 1)]:
        needle = f'pub fn {name}() -> u32 {{\n    unsafe {{ libc::{name}() as u32 }}\n}}'
        replacement = f'''pub fn {name}() -> u32 {{
    #[cfg(target_os = "trueos")]
    {{
        unsafe extern "C" {{ fn trueos_cabi_process_identity_v1(which: u32) -> u32; }}
        unsafe {{ trueos_cabi_process_identity_v1({selector}) }}
    }}
    #[cfg(not(target_os = "trueos"))]
    {{ unsafe {{ libc::{name}() as u32 }} }}
}}'''
        source = replace_once(source, needle, replacement, common)
    source = replace_once(source, 'pub enum Stdio {\n', 'pub enum Stdio {\n    #[cfg(target_os = "trueos")]\n    NativeFile(File),\n', common)
    needle = 'impl From<File> for Stdio {\n    fn from(file: File) -> Stdio {\n        Stdio::Fd(file.into_inner())\n    }\n}'
    replacement = '''impl From<File> for Stdio {
    fn from(file: File) -> Stdio {
        #[cfg(target_os = "trueos")]
        { Stdio::NativeFile(file) }
        #[cfg(not(target_os = "trueos"))]
        { Stdio::Fd(file.into_inner()) }
    }
}'''
    source = replace_once(source, needle, replacement, common)
    source = replace_once(source,
        '    pub fn to_child_stdio(&self, readable: bool) -> io::Result<(ChildStdio, Option<ChildPipe>)> {',
        '''    #[cfg(target_os = "trueos")]
    pub fn to_child_stdio(&self, _readable: bool) -> io::Result<(ChildStdio, Option<ChildPipe>)> {
        crate::sys::pal::unsupported::unsupported()
    }
    #[cfg(not(target_os = "trueos"))]
    pub fn to_child_stdio(&self, readable: bool) -> io::Result<(ChildStdio, Option<ChildPipe>)> {''', common)
    source = replace_once(source, 'pub fn read_output(\n', '#[cfg(not(target_os = "trueos"))]\npub fn read_output(\n', common)
    marker = 'pub fn getpid() -> u32 {'
    native_read = '''#[cfg(target_os = "trueos")]
pub fn read_output(_out: ChildPipe, _stdout: &mut Vec<u8>, _err: ChildPipe, _stderr: &mut Vec<u8>) -> io::Result<()> {
    crate::sys::pal::unsupported::unsupported()
}

'''
    source = replace_once(source, marker, native_read + marker, common)
    # Keep the Unix command builder vocabulary without retaining FD ownership
    # that native spawning can never use. Actual native files keep their
    # existing capability in NativeFile until the rejected command is dropped.
    source = replace_once(source, '    Fd(FileDesc),',
        '    #[cfg(not(target_os = "trueos"))]\n    Fd(FileDesc),', common)
    source = replace_once(source, "    StaticFd(BorrowedFd<'static>),",
        "    #[cfg(not(target_os = \"trueos\"))]\n    StaticFd(BorrowedFd<'static>),", common)
    source = replace_once(source, 'impl From<FileDesc> for Stdio {',
        '#[cfg(not(target_os = "trueos"))]\nimpl From<FileDesc> for Stdio {', common)
    source = replace_once(source, 'pub type ChildPipe = crate::sys::pipe::Pipe;', '''pub type ChildPipe = crate::sys::pipe::Pipe;

#[cfg(target_os = "trueos")]
impl From<ChildPipe> for Stdio {
    fn from(pipe: ChildPipe) -> Self { pipe.diverge() }
}''', common)
    for stream in ["STDOUT", "STDERR"]:
        expression = f'Stdio::StaticFd(unsafe {{ BorrowedFd::borrow_raw(libc::{stream}_FILENO) }})'
        replacement = f'''#[cfg(target_os = "trueos")]
        {{ Stdio::Inherit }}
        #[cfg(not(target_os = "trueos"))]
        {{ {expression} }}'''
        source = replace_once(source, expression, replacement, common)
    changes[common] = source

    # Native child processes have no pipe endpoints. Select std's existing
    # uninhabited Unsupported pipe, so impossible child I/O retains no Unix FD.
    path = system / "pipe/mod.rs"
    source = changes.get(path, path.read_text())
    changes[path] = replace_once(source, 'cfg_select! {', '''cfg_select! {
    target_os = "trueos" => {
        mod unsupported;
        pub use unsupported::{Pipe, pipe};
    }''', path)

    # Arbitrary Unix descriptors cannot become native child-process stdio.
    # Command configuration and ExitStatus decoding remain available.
    path = system.parent / "os/unix/process.rs"
    source = changes.get(path, path.read_text())
    for implementation in [
        'impl FromRawFd for process::Stdio {',
        'impl From<OwnedFd> for process::Stdio {',
        'impl From<OwnedFd> for process::ChildStdin {',
        'impl From<OwnedFd> for process::ChildStdout {',
        'impl From<OwnedFd> for process::ChildStderr {',
    ]:
        source = replace_once(source, implementation,
            '#[cfg(not(target_os = "trueos"))]\n' + implementation, path)
    changes[path] = source

    # File objects have no Unix descriptor. The standard streams retain their
    # public AsFd wrapper solely to identify stream 0/1/2 for the typed query.
    path = system.parent / "io/stdio.rs"
    source = changes.get(path, path.read_text())
    needle = "impl_is_terminal!(File, Stdin, StdinLock<'_>, Stdout, StdoutLock<'_>, Stderr, StderrLock<'_>);"
    replacement = '''#[cfg(not(target_os = "trueos"))]
impl_is_terminal!(File);
#[cfg(target_os = "trueos")]
#[stable(feature = "is_terminal", since = "1.70.0")]
impl IsTerminal for File {
    fn is_terminal(&self) -> bool { false }
}
impl_is_terminal!(Stdin, StdinLock<'_>, Stdout, StdoutLock<'_>, Stderr, StderrLock<'_>);'''
    changes[path] = replace_once(source, needle, replacement, path)

    exit_path = system / "exit.rs"
    source = changes.get(exit_path, exit_path.read_text())
    declaration = '#[cfg(target_os = "trueos")]\n#[path = "exit/trueos.rs"]\nmod trueos;\n\n'
    if not source.startswith(declaration):
        source = declaration + source
    source = replace_once(source, 'pub fn exit(code: i32) -> ! {\n    cfg_select! {',
        'pub fn exit(code: i32) -> ! {\n    cfg_select! {\n        target_os = "trueos" => { trueos::exit(code) }', exit_path)
    changes[exit_path] = source
    changes[system / "exit/trueos.rs"] = (ROOT / "tools/rust-std/trueos_exit.rs").read_text()


def planned(root):
    library = root / "library" if (root / "library").is_dir() else root
    system = library / "std/src/sys"
    changes = {}
    for module, reference in MODULES.items():
        directory = system / module
        changes[directory / "trueos.rs"] = (ROOT / "tools/rust-std" / reference).read_text()
        selector = system / "io/mod.rs" if module == "io/is_terminal" else directory / "mod.rs"
        source = changes.get(selector, selector.read_text())
        marker = "cfg_select! {"
        insertion = '\n    target_os = "trueos" => {\n        mod trueos;\n        pub use trueos::*;\n    }'
        if module == "paths":
            insertion = '\n    target_os = "trueos" => {\n        mod trueos;\n        use trueos as imp;\n    }'
        if module == "io/is_terminal":
            insertion = '\n        target_os = "trueos" => {\n            mod trueos;\n            pub use trueos::*;\n        }'
        if insertion not in source:
            if 'target_os = "trueos" => {' in source:
                raise ValueError(f"{selector}: conflicting TRUEOS selector")
            if source.count(marker) != 1:
                raise ValueError(f"{selector}: expected one platform selector")
            source = source.replace(marker, marker + insertion, 1)
        changes[selector] = source
    add_process_and_exit(changes, system)
    return changes


def install(root, check=False):
    changes = planned(root)
    differing = [path for path, source in changes.items()
                 if not path.is_file() or path.read_text() != source]
    if check and differing:
        raise ValueError("native std environment/stdio backend not installed: " + ", ".join(map(str, differing)))
    if not check:
        for path in differing:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(changes[path])
    return differing


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    try:
        install(args.root, args.check)
    except (OSError, ValueError) as error:
        parser.exit(1, str(error) + "\n")
    print("trueos-rust-std: environment=realm-byte-pairs args=native-launch stdio=native-byte-streams")


if __name__ == "__main__":
    main()
