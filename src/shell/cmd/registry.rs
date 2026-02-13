use alloc::boxed::Box;
use alloc::vec::Vec;
use core::fmt::Write;
use spin::{Mutex, Once};

use crate::shell::{ShellBackend, ShellIo, CommandAction};

// Re-export or common types needed by handlers
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum ArgType {
    Str,
    U8,
    Usize,
    /// Captures the remainder of the command line (may contain spaces).
    Rest,
}

impl ArgType {
    pub(crate) fn name(self) -> &'static str {
        match self {
            ArgType::Str => "str",
            ArgType::U8 => "u8",
            ArgType::Usize => "usize",
            ArgType::Rest => "rest",
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct ArgSpec {
    pub(crate) name: &'static str,
    pub(crate) ty: ArgType,
    pub(crate) mandatory: bool,
}

impl ArgSpec {
    pub(crate) const fn new(name: &'static str, ty: ArgType) -> Self {
        Self {
            name,
            ty,
            mandatory: false,
        }
    }

    pub(crate) const fn mandatory(mut self) -> Self {
        self.mandatory = true;
        self
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) enum ArgValue<'a> {
    Str(&'a str),
    U64(u64),
    Usize(usize),
}

impl<'a> ArgValue<'a> {
    pub(crate) fn as_str(self) -> Option<&'a str> {
        match self {
            ArgValue::Str(s) => Some(s),
            _ => None,
        }
    }

    pub(crate) fn as_u8(self) -> Option<u8> {
        match self {
            ArgValue::U64(v) => u8::try_from(v).ok(),
            _ => None,
        }
    }

    pub(crate) fn as_u64(self) -> Option<u64> {
        match self {
            ArgValue::U64(v) => Some(v),
            _ => None,
        }
    }

    pub(crate) fn as_usize(self) -> Option<usize> {
        match self {
            ArgValue::Usize(v) => Some(v),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ParsedArgs<'a> {
    values: Vec<ArgValue<'a>>,
}

impl<'a> ParsedArgs<'a> {
    pub(crate) fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub(crate) fn get(&self, idx: usize) -> Option<ArgValue<'a>> {
        self.values.get(idx).copied()
    }

    pub(crate) fn get_str(&self, idx: usize) -> Option<&'a str> {
        self.get(idx).and_then(|v| v.as_str())
    }

    pub(crate) fn get_u64(&self, idx: usize) -> Option<u64> {
        self.get(idx).and_then(|v| v.as_u64())
    }

    pub(crate) fn get_usize(&self, idx: usize) -> Option<usize> {
        self.get(idx).and_then(|v| v.as_usize())
    }

    pub(crate) fn get_u8(&self, idx: usize) -> Option<u8> {
        self.get(idx).and_then(|v| v.as_u8())
    }
}

pub(crate) struct ShellCommandCtx<'a> {
    pub(crate) line: &'a str,
    pub(crate) spawner: &'a embassy_executor::Spawner,
    pub(crate) io: &'a dyn ShellBackend,
    pub(crate) term_cols: &'a mut usize,
    pub(crate) term_rows: &'a mut usize,
    pub(crate) mode: &'a mut crate::shell::ShellMode,
}

pub(crate) type ShellCmdHandler = fn(&mut ShellCommandCtx<'_>, Option<&ParsedArgs<'_>>) -> CommandAction;

#[derive(Clone)]
pub(crate) struct ShellCommand {
    pub(crate) name: &'static str,
    pub(crate) args: &'static [ArgSpec],
    pub(crate) handler: ShellCmdHandler,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum RegisterError {
    DuplicateName,
    EmptyName,
}

static REGISTRY: Once<Mutex<Vec<&'static ShellCommand>>> = Once::new();
static BUILTINS_ONCE: Once<()> = Once::new();

fn registry() -> &'static Mutex<Vec<&'static ShellCommand>> {
    REGISTRY.call_once(|| Mutex::new(Vec::new()))
}

fn find_command(cmds: &[&'static ShellCommand], name: &str) -> Option<&'static ShellCommand> {
    // Fast path: length mismatch cannot match.
    let name = name.trim();
    let name_len = name.len();
    cmds.iter()
        .copied()
        .filter(|c| c.name.len() == name_len)
        .find(|c| c.name.eq_ignore_ascii_case(name))
}

pub(crate) fn list_command_names<const N: usize>(out: &mut heapless::Vec<&'static str, N>) {
    init_builtin_shell_commands();
    out.clear();
    let cmds = registry().lock();
    for c in cmds.iter() {
        let _ = out.push(c.name);
    }
}

pub(crate) fn usage_text_for_name<const N: usize>(name: &str, out: &mut heapless::String<N>) -> bool {
    init_builtin_shell_commands();
    let cmd = {
        let cmds = registry().lock();
        find_command(cmds.as_slice(), name)
    };
    let Some(cmd) = cmd else { return false };

    out.clear();
    let _ = write!(out, "usage: {}", cmd.name);
    for a in cmd.args.iter() {
        let _ = out.push(' ');
        if !a.mandatory {
            let _ = out.push('[');
        }
        let _ = out.push_str(a.name);
        let _ = out.push(':');
        let _ = out.push_str(a.ty.name());
        if !a.mandatory {
            let _ = out.push(']');
        }
    }
    true
}

pub(crate) fn reg_sh_cmd(
    name: &'static str,
    args: &'static [ArgSpec],
    handler: ShellCmdHandler,
) -> Result<(), RegisterError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(RegisterError::EmptyName);
    }

    let leaked: &'static ShellCommand = Box::leak(Box::new(ShellCommand { name, args, handler }));

    let mut cmds = registry().lock();
    if cmds.iter().any(|c| c.name.eq_ignore_ascii_case(name)) {
        return Err(RegisterError::DuplicateName);
    }
    cmds.push(leaked);
    Ok(())
}

/// Uppercase alias to match the requested API.
#[allow(non_snake_case)]
pub(crate) fn REGSHCMD(
    name: &'static str,
    args: &'static [ArgSpec],
    handler: ShellCmdHandler,
) -> Result<(), RegisterError> {
    reg_sh_cmd(name, args, handler)
}

/// Dispatch a command line to the registered command set.
pub(crate) fn dispatch_line(ctx: &mut ShellCommandCtx<'_>) -> Option<CommandAction> {
    init_builtin_shell_commands();

    let line = ctx.line.trim();
    if line.is_empty() {
        return None;
    }

    let (mut verb, mut rest) = split_verb_rest(line);

    // Enforce "§1" style, reject "§ 1"
    if verb == "§" {
        return None;
    }
    if verb.starts_with("§") && verb.len() > "§".len() {
        // Treat "§<something>" as command "§" with argument "<something>"
        // This effectively shifts the split point to right after "§"
        verb = "§";
        // Safe slicing because we know verb (and thus line) starts with "§"
        rest = &line["§".len()..];
    }

    let cmd = {
        let cmds = registry().lock();
        find_command(cmds.as_slice(), verb)
    };
    let Some(cmd) = cmd else { return None };

    match parse_args(cmd, rest) {
        Ok(parsed) => {
            if parsed.is_empty() {
                Some((cmd.handler)(ctx, None))
            } else {
                Some((cmd.handler)(ctx, Some(&parsed)))
            }
        }
        Err(err) => {
            print_arg_error(ctx.io, cmd, &err);
            Some(CommandAction::None)
        }
    }
}

fn split_verb_rest(line: &str) -> (&str, &str) {
    let mut iter = line.char_indices();
    while let Some((idx, ch)) = iter.next() {
        if ch.is_whitespace() {
            let a = &line[..idx];
            let b = line[idx..].trim();
            return (a, b);
        }
    }
    (line, "")
}

#[derive(Clone, Debug)]
struct ArgError {
    kind: ArgErrorKind,
}

#[derive(Clone, Debug)]
enum ArgErrorKind {
    Missing { name: &'static str, ty: ArgType },
    TooMany { expected: usize, got: usize },
    BadValue { name: &'static str, ty: ArgType, value: alloc::string::String, hint: &'static str },
    RestNotLast,
}

fn parse_args<'a>(cmd: &ShellCommand, rest: &'a str) -> Result<ParsedArgs<'a>, ArgError> {
    if cmd.args.is_empty() {
        let got = rest.split_whitespace().count();
        if got != 0 {
            return Err(ArgError { kind: ArgErrorKind::TooMany { expected: 0, got } });
        }
        return Ok(ParsedArgs { values: Vec::new() });
    }

    if let Some((idx, _)) = cmd.args.iter().enumerate().find(|(_, a)| a.ty == ArgType::Rest) {
        if idx + 1 != cmd.args.len() {
            return Err(ArgError { kind: ArgErrorKind::RestNotLast });
        }
    }

    if cmd.args.len() == 1 && cmd.args[0].ty == ArgType::Rest {
        let arg0 = cmd.args[0];
        if arg0.mandatory && rest.is_empty() {
            return Err(ArgError { kind: ArgErrorKind::Missing { name: arg0.name, ty: arg0.ty } });
        }
        let mut values = Vec::new();
        if !rest.is_empty() {
            values.push(ArgValue::Str(rest));
        }
        return Ok(ParsedArgs { values });
    }

    let tokens: Vec<&'a str> = rest.split_whitespace().collect();

    let has_rest = cmd.args.last().map(|a| a.ty == ArgType::Rest).unwrap_or(false);
    let positional_count = if has_rest { cmd.args.len() - 1 } else { cmd.args.len() };

    if !has_rest && tokens.len() > cmd.args.len() {
        return Err(ArgError { kind: ArgErrorKind::TooMany { expected: cmd.args.len(), got: tokens.len() } });
    }

    for i in 0..positional_count {
        let spec = cmd.args[i];
        if spec.mandatory && tokens.get(i).is_none() {
            return Err(ArgError { kind: ArgErrorKind::Missing { name: spec.name, ty: spec.ty } });
        }
    }

    let mut values: Vec<ArgValue<'a>> = Vec::new();

    for i in 0..positional_count {
        let spec = cmd.args[i];
        let Some(tok) = tokens.get(i).copied() else {
            continue;
        };
        let v = parse_token(spec, tok)?;
        values.push(v);
    }

    if has_rest {
        let spec = *cmd.args.last().unwrap();
        if spec.mandatory && tokens.len() <= positional_count {
            return Err(ArgError { kind: ArgErrorKind::Missing { name: spec.name, ty: spec.ty } });
        }

        if tokens.len() > positional_count {
            let rest_tok = tokens[positional_count];
            let start = rest.find(rest_tok).unwrap_or(0);
            let tail = rest[start..].trim();
            if !tail.is_empty() {
                values.push(ArgValue::Str(tail));
            }
        }
    }

    Ok(ParsedArgs { values })
}

fn parse_token<'a>(spec: ArgSpec, tok: &'a str) -> Result<ArgValue<'a>, ArgError> {
    match spec.ty {
        ArgType::Str => Ok(ArgValue::Str(tok)),
        ArgType::Rest => Ok(ArgValue::Str(tok)),
        ArgType::U8 => parse_u(tok)
            .and_then(|v| {
                if v <= u8::MAX as u64 {
                    Ok(v)
                } else {
                    Err("value out of range")
                }
            })
            .map(ArgValue::U64)
            .map_err(|e| bad(spec, tok, e)),
        ArgType::Usize => parse_u(tok)
            .and_then(|v| usize::try_from(v).map_err(|_| "value out of range"))
            .map(ArgValue::Usize)
            .map_err(|e| bad(spec, tok, e)),
    }
}

fn bad(spec: ArgSpec, tok: &str, hint: &'static str) -> ArgError {
    ArgError {
        kind: ArgErrorKind::BadValue {
            name: spec.name,
            ty: spec.ty,
            value: alloc::string::String::from(tok),
            hint,
        },
    }
}

fn parse_u(tok: &str) -> Result<u64, &'static str> {
    let t = tok.trim();
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).map_err(|_| "expected unsigned integer (dec or 0xHEX)")
    } else {
        t.parse::<u64>().map_err(|_| "expected unsigned integer (dec or 0xHEX)")
    }
}

#[inline]
fn style_cmd_name(name: &str) -> impl core::fmt::Display + '_ {
    crate::ecma48::bold(name)
}

#[inline]
fn style_arg_name(name: &str) -> impl core::fmt::Display + '_ {
    crate::ecma48::color(name, crate::shell::PROMPT_RGB)
}

#[inline]
fn style_arg_type(ty: ArgType) -> impl core::fmt::Display {
    crate::ecma48::dim(ty.name())
}

#[inline]
fn style_error_label(text: &str) -> impl core::fmt::Display + '_ {
    crate::ecma48::color(text, (255, 96, 96))
}

pub(crate) fn print_usage(io: &dyn ShellIo, cmd: &ShellCommand) {
    io.write_fmt(format_args!("{} ", crate::ecma48::dim("usage:")));
    io.write_fmt(format_args!("{}", style_cmd_name(cmd.name)));

    for a in cmd.args.iter() {
        io.write_str(" ");
        if !a.mandatory {
            io.write_str("[");
        }
        io.write_fmt(format_args!("{}", style_arg_name(a.name)));
        io.write_str(":");
        io.write_fmt(format_args!("{}", style_arg_type(a.ty)));
        if !a.mandatory {
            io.write_str("]");
        }
    }
    io.write_str("\r\n");
}

fn print_arg_error(io: &dyn ShellIo, cmd: &ShellCommand, err: &ArgError) {
    match &err.kind {
        ArgErrorKind::Missing { name, ty } => {
            io.write_fmt(format_args!(
                "{} {}: missing argument '{}' (expected {})\r\n",
                style_error_label("error"),
                style_cmd_name(cmd.name),
                style_arg_name(name),
                style_arg_type(*ty),
            ));
            print_usage(io, cmd);
        }
        ArgErrorKind::TooMany { expected, got } => {
            io.write_fmt(format_args!(
                "{} {}: too many arguments (expected {}, got {})\r\n",
                style_error_label("error"),
                style_cmd_name(cmd.name),
                expected,
                got
            ));
            print_usage(io, cmd);
        }
        ArgErrorKind::BadValue { name, ty, value, hint } => {
            io.write_fmt(format_args!(
                "{} {}: bad value for '{}' (expected {}, got '{}')\r\n",
                style_error_label("error"),
                style_cmd_name(cmd.name),
                style_arg_name(name),
                style_arg_type(*ty),
                value
            ));
            io.write_fmt(format_args!("{} {}\r\n", crate::ecma48::dim("hint:"), hint));
            print_usage(io, cmd);
        }
        ArgErrorKind::RestNotLast => {
            io.write_fmt(format_args!(
                "{} {}: internal schema error: rest argument must be last\r\n",
                style_error_label("error"),
                style_cmd_name(cmd.name),
            ));
            print_usage(io, cmd);
        }
    }
}

pub(crate) fn print_schema(io: &dyn ShellIo, cmd: &ShellCommand) {
    io.write_fmt(format_args!("{} {}\r\n", crate::ecma48::dim("cmd:"), style_cmd_name(cmd.name)));
    if cmd.args.is_empty() {
        io.write_fmt(format_args!("  {}\r\n", crate::ecma48::dim("(no args)")));
        return;
    }

    for a in cmd.args.iter() {
        io.write_str("  ");
        io.write_fmt(format_args!("{}", style_arg_name(a.name)));
        io.write_str(": ");
        io.write_fmt(format_args!("{}", style_arg_type(a.ty)));
        io.write_str("  ");
        if a.mandatory {
            io.write_fmt(format_args!("{}", crate::ecma48::bold("mandatory")));
        } else {
            io.write_fmt(format_args!("{}", crate::ecma48::dim("optional")));
        }
        io.write_str("\r\n");
    }
}

pub(crate) fn init_builtin_shell_commands() {
    BUILTINS_ONCE.call_once(|| {
        use crate::shell::cmd;

        static ECMA48_ARGS: [ArgSpec; 1] = [ArgSpec::new("arg", ArgType::Rest)];
        // static GET_ARGS: [ArgSpec; 1] = [ArgSpec::new("url", ArgType::Str).mandatory()];
        // static HTTPS_ARGS: [ArgSpec; 1] = [ArgSpec::new("host", ArgType::Str)];
        /*
        static NET_ARGS: [ArgSpec; 2] = [
            ArgSpec::new("op", ArgType::Str).mandatory(),
            ArgSpec::new("target", ArgType::Str),
        ];
        */
        #[cfg(feature = "dma_nic_fpga")]
        static DMAFPGA_ARGS: [ArgSpec; 1] = [ArgSpec::new("arg", ArgType::Rest).mandatory()];
        static NO_ARGS: [ArgSpec; 0] = [];
        static NET_ARGS: [ArgSpec; 0] = [];
        static NET_ICMP_ARGS: [ArgSpec; 1] = [ArgSpec::new("target", ArgType::Str).mandatory()];
        static NET_NIC_ARGS: [ArgSpec; 1] = [ArgSpec::new("index", ArgType::Rest)];
        static NET_HOSTNAME_ARGS: [ArgSpec; 1] = [ArgSpec::new("name", ArgType::Str)];
        static NET_HTTP_ARGS: [ArgSpec; 1] = [ArgSpec::new("url", ArgType::Str).mandatory()];
        static NET_HTTPS_ARGS: [ArgSpec; 1] = [ArgSpec::new("host", ArgType::Str)];
        
        static QJS_ARGS: [ArgSpec; 1] = [ArgSpec::new("src", ArgType::Rest)];

        static AI_ARGS: [ArgSpec; 1] = [ArgSpec::new("msg", ArgType::Rest)];
        static TURBO_ARGS: [ArgSpec; 2] = [
            ArgSpec::new("op", ArgType::Str),
            ArgSpec::new("spins", ArgType::Usize),
        ];
        static SMP_ARGS: [ArgSpec; 1] = [ArgSpec::new("slot", ArgType::Usize)];
        static SET_ARGS: [ArgSpec; 2] = [
            ArgSpec::new("cols", ArgType::Usize).mandatory(),
            ArgSpec::new("rows", ArgType::Usize).mandatory(),
        ];
        static SECTION_ARGS: [ArgSpec; 1] = [ArgSpec::new("id", ArgType::U8)];
        static FILE_ARGS: [ArgSpec; 1] = [ArgSpec::new("id", ArgType::Rest)];
        static ACPI_ARGS: [ArgSpec; 1] = [ArgSpec::new("state", ArgType::Str).mandatory()];
        static HV_ARGS: [ArgSpec; 1] = [ArgSpec::new("op", ArgType::Str)];
        static PCI_USB_ARGS: [ArgSpec; 1] = [ArgSpec::new("cmd", ArgType::Str)];

        let _ = REGSHCMD("§", &SECTION_ARGS, cmd::cmd_section);
        let _ = REGSHCMD("cmd", &NO_ARGS, cmd::cmd_cmd); 
        let _ = REGSHCMD("ecma48", &ECMA48_ARGS, cmd::cmd_ecma48);
        
        // Network
        let _ = REGSHCMD("net", &NET_ARGS, cmd::cmd_net);           // Prints help
        let _ = REGSHCMD("net.icmp", &NET_ICMP_ARGS, cmd::cmd_net_icmp);
        let _ = REGSHCMD("net.nic", &NET_NIC_ARGS, cmd::cmd_net_nic);
        let _ = REGSHCMD("net.hostname", &NET_HOSTNAME_ARGS, cmd::cmd_net_hostname);
        let _ = REGSHCMD("net.http", &NET_HTTP_ARGS, cmd::cmd_net_http);
        let _ = REGSHCMD("net.https", &NET_HTTPS_ARGS, cmd::cmd_net_https);
        
        #[cfg(feature = "dma_nic_fpga")]
        let _ = REGSHCMD("dmafpga", &DMAFPGA_ARGS, cmd::cmd_dmafpga);
        let _ = REGSHCMD("update", &NO_ARGS, cmd::cmd_update);
        let _ = REGSHCMD("install", &[], cmd::cmd_install);
        let _ = REGSHCMD("format", &NO_ARGS, cmd::cmd_format);
        let _ = REGSHCMD("bench", &NO_ARGS, cmd::cmd_bench);
        let _ = REGSHCMD("bench.net", &NO_ARGS, cmd::cmd_netbench);
        let _ = REGSHCMD("file", &FILE_ARGS, cmd::cmd_file);
        let _ = REGSHCMD("qjs", &QJS_ARGS, cmd::cmd_qjs);
        let _ = REGSHCMD("acpi", &ACPI_ARGS, cmd::cmd_acpi);
        // let _ = REGSHCMD("https", &HTTPS_ARGS, cmd::cmd_https);
        let _ = REGSHCMD("hv", &HV_ARGS, cmd::cmd_hv);
        let _ = REGSHCMD("go", &[], cmd::cmd_go);

        // Table commands
        let _ = REGSHCMD("tlb", &NO_ARGS, cmd::cmd_tlb);
        let _ = REGSHCMD("tlb.pci", &NO_ARGS, cmd::cmd_tlb_pci);
        let _ = REGSHCMD("tlb.mem", &NO_ARGS, cmd::cmd_tlb_mem);
        let _ = REGSHCMD("tlb.cpu", &NO_ARGS, cmd::cmd_tlb_cpu);
        let _ = REGSHCMD("tlb.acpi", &NO_ARGS, cmd::cmd_tlb_acpi);
        let _ = REGSHCMD("tlb.acpi.facp", &NO_ARGS, cmd::cmd_tlb_acpi_facp);
        let _ = REGSHCMD("tlb.acpi.madt", &NO_ARGS, cmd::cmd_tlb_acpi_madt);
        let _ = REGSHCMD("tlb.acpi.hpet", &NO_ARGS, cmd::cmd_tlb_acpi_hpet);
        let _ = REGSHCMD("tlb.acpi.mcfg", &NO_ARGS, cmd::cmd_tlb_acpi_mcfg);
        let _ = REGSHCMD("tlb.acpi.ssdt", &NO_ARGS, cmd::cmd_tlb_acpi_ssdt);
        let _ = REGSHCMD("tlb.x2apic", &NO_ARGS, cmd::cmd_tlb_x2apic);
        let _ = REGSHCMD("tlb.uefi", &NO_ARGS, cmd::cmd_tlb_uefi);
        let _ = REGSHCMD("tlb.dump", &NO_ARGS, cmd::cmd_tlb_dump);

        let _ = REGSHCMD("ai", &AI_ARGS, crate::ai::cmd_ai);
        
        let _ = REGSHCMD("mandel", &[], cmd::cmd_mandel);
        let _ = REGSHCMD("set", &SET_ARGS, cmd::cmd_set);
        let _ = REGSHCMD("turbo", &TURBO_ARGS, cmd::cmd_turbo);
        let _ = REGSHCMD("smp", &SMP_ARGS, cmd::cmd_smp);
        let _ = REGSHCMD("cube", &[], cmd::cmd_cube);
        let _ = REGSHCMD("cube.ico", &[], cmd::cmd_ico);
        let _ = REGSHCMD("txt", &NO_ARGS, cmd::cmd_txt);
        let _ = REGSHCMD("insane", &[], cmd::cmd_insane);
        let _ = REGSHCMD("pci.usb", &PCI_USB_ARGS, cmd::cmd_pci_usb);
        let _ = REGSHCMD("usb", &NO_ARGS, cmd::cmd_usb);
    });
}