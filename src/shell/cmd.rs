use alloc::boxed::Box;
use alloc::vec::Vec;

use core::fmt::Write;

use spin::{Mutex, Once};
use embassy_executor::task;

use crate::shell::{ShellBackend, ShellIo};

// NOTE: This module intentionally keeps the registration API simple and
// runtime-driven. The kernel has no global constructors, so callers should
// invoke `init_builtin_shell_commands()` once during shell startup.

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum ArgType {
    /// Accepts any token and passes it through as a string slice.
    Any,
    Str,
    Bool,
    U8,
    U16,
    U32,
    U64,
    I32,
    I64,
    Usize,
    Isize,
    /// Captures the remainder of the command line (may contain spaces).
    Rest,
}

impl ArgType {
    pub(crate) fn name(self) -> &'static str {
        match self {
            ArgType::Any => "any",
            ArgType::Str => "str",
            ArgType::Bool => "bool",
            ArgType::U8 => "u8",
            ArgType::U16 => "u16",
            ArgType::U32 => "u32",
            ArgType::U64 => "u64",
            ArgType::I32 => "i32",
            ArgType::I64 => "i64",
            ArgType::Usize => "usize",
            ArgType::Isize => "isize",
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
    Bool(bool),
    U64(u64),
    I64(i64),
    Usize(usize),
    Isize(isize),
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

    pub(crate) fn as_i64(self) -> Option<i64> {
        match self {
            ArgValue::I64(v) => Some(v),
            _ => None,
        }
    }

    pub(crate) fn as_bool(self) -> Option<bool> {
        match self {
            ArgValue::Bool(v) => Some(v),
            _ => None,
        }
    }

    pub(crate) fn as_usize(self) -> Option<usize> {
        match self {
            ArgValue::Usize(v) => Some(v),
            _ => None,
        }
    }

    pub(crate) fn as_isize(self) -> Option<isize> {
        match self {
            ArgValue::Isize(v) => Some(v),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ParsedArgs<'a> {
    specs: &'static [ArgSpec],
    values: Vec<ArgValue<'a>>,
}

impl<'a> ParsedArgs<'a> {
    pub(crate) fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub(crate) fn len(&self) -> usize {
        self.values.len()
    }

    pub(crate) fn get(&self, idx: usize) -> Option<ArgValue<'a>> {
        self.values.get(idx).copied()
    }

    pub(crate) fn get_by_name(&self, name: &str) -> Option<ArgValue<'a>> {
        self.specs
            .iter()
            .position(|s| s.name.eq_ignore_ascii_case(name))
            .and_then(|idx| self.get(idx))
    }

    pub(crate) fn specs(&self) -> &'static [ArgSpec] {
        self.specs
    }
}

pub(crate) struct ShellCommandCtx<'a> {
    pub(crate) line: &'a str,
    pub(crate) spawner: &'a embassy_executor::Spawner,
    pub(crate) io: &'static dyn ShellBackend,
    pub(crate) term_cols: &'a mut usize,
    pub(crate) term_rows: &'a mut usize,
    pub(crate) go_mode: &'a mut bool,
    pub(crate) install_wizard: &'a mut Option<super::InstallWizardStage>,
}

pub(crate) type ShellCmdHandler = fn(&mut ShellCommandCtx<'_>, Option<&ParsedArgs<'_>>) -> super::CommandAction;

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
///
/// Returns `Some(action)` if the line matched a registered command name.
/// Returns `None` if no registered command matched (caller may fall back to legacy parsing).
pub(crate) fn dispatch_line(ctx: &mut ShellCommandCtx<'_>) -> Option<super::CommandAction> {
    init_builtin_shell_commands();

    let line = ctx.line.trim();
    if line.is_empty() {
        return None;
    }

    let (verb, rest) = split_verb_rest(line);
    // IMPORTANT: Don't hold the registry lock while parsing/executing the command.
    // Some commands (e.g. `args`) look back into the registry.
    let cmd = {
        let cmds = registry().lock();
        find_command(cmds.as_slice(), verb)
    };
    let Some(cmd) = cmd else { return None };

    // Parse arguments based on the command schema.
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
            Some(super::CommandAction::None)
        }
    }
}

fn split_verb_rest(line: &str) -> (&str, &str) {
    // `split_once` does not support predicate patterns on stable, so do this manually.
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
        return Ok(ParsedArgs { specs: cmd.args, values: Vec::new() });
    }

    // If there is a `rest` argument, it must be last.
    if let Some((idx, _)) = cmd.args.iter().enumerate().find(|(_, a)| a.ty == ArgType::Rest) {
        if idx + 1 != cmd.args.len() {
            return Err(ArgError { kind: ArgErrorKind::RestNotLast });
        }
    }

    // Special-case: a single Rest argument consumes the whole remainder.
    if cmd.args.len() == 1 && cmd.args[0].ty == ArgType::Rest {
        let arg0 = cmd.args[0];
        if arg0.mandatory && rest.is_empty() {
            return Err(ArgError { kind: ArgErrorKind::Missing { name: arg0.name, ty: arg0.ty } });
        }
        let mut values = Vec::new();
        if !rest.is_empty() {
            values.push(ArgValue::Str(rest));
        }
        return Ok(ParsedArgs { specs: cmd.args, values });
    }

    // Otherwise: positional parsing by whitespace tokens, with optional trailing Rest.
    let tokens: Vec<&'a str> = rest.split_whitespace().collect();

    let has_rest = cmd.args.last().map(|a| a.ty == ArgType::Rest).unwrap_or(false);
    let positional_count = if has_rest { cmd.args.len() - 1 } else { cmd.args.len() };

    if !has_rest && tokens.len() > cmd.args.len() {
        return Err(ArgError { kind: ArgErrorKind::TooMany { expected: cmd.args.len(), got: tokens.len() } });
    }

    // Validate mandatory positional args.
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
        // If rest is mandatory, require at least one leftover token.
        if spec.mandatory && tokens.len() <= positional_count {
            return Err(ArgError { kind: ArgErrorKind::Missing { name: spec.name, ty: spec.ty } });
        }

        if tokens.len() > positional_count {
            // Find the start byte offset of the first rest token in `rest`.
            let rest_tok = tokens[positional_count];
            let start = rest.find(rest_tok).unwrap_or(0);
            let tail = rest[start..].trim();
            if !tail.is_empty() {
                values.push(ArgValue::Str(tail));
            }
        }
    }

    Ok(ParsedArgs { specs: cmd.args, values })
}

fn parse_token<'a>(spec: ArgSpec, tok: &'a str) -> Result<ArgValue<'a>, ArgError> {
    match spec.ty {
        ArgType::Any | ArgType::Str => Ok(ArgValue::Str(tok)),
        ArgType::Rest => Ok(ArgValue::Str(tok)),
        ArgType::Bool => parse_bool(spec.name, tok),
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
        ArgType::U16 => parse_u(tok)
            .and_then(|v| {
                if v <= u16::MAX as u64 {
                    Ok(v)
                } else {
                    Err("value out of range")
                }
            })
            .map(ArgValue::U64)
            .map_err(|e| bad(spec, tok, e)),
        ArgType::U32 => parse_u(tok)
            .and_then(|v| {
                if v <= u32::MAX as u64 {
                    Ok(v)
                } else {
                    Err("value out of range")
                }
            })
            .map(ArgValue::U64)
            .map_err(|e| bad(spec, tok, e)),
        ArgType::U64 => parse_u(tok).map(|v| ArgValue::U64(v)).map_err(|e| bad(spec, tok, e)),
        ArgType::I32 => parse_i(tok)
            .and_then(|v| {
                if v >= i32::MIN as i64 && v <= i32::MAX as i64 {
                    Ok(v)
                } else {
                    Err("value out of range")
                }
            })
            .map(ArgValue::I64)
            .map_err(|e| bad(spec, tok, e)),
        ArgType::I64 => parse_i(tok).map(|v| ArgValue::I64(v)).map_err(|e| bad(spec, tok, e)),
        ArgType::Usize => parse_u(tok)
            .and_then(|v| usize::try_from(v).map_err(|_| "value out of range"))
            .map(ArgValue::Usize)
            .map_err(|e| bad(spec, tok, e)),
        ArgType::Isize => parse_i(tok)
            .and_then(|v| isize::try_from(v).map_err(|_| "value out of range"))
            .map(ArgValue::Isize)
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

fn parse_bool<'a>(name: &'static str, tok: &'a str) -> Result<ArgValue<'a>, ArgError> {
    let t = tok.trim();
    let v = match t {
        "1" | "true" | "True" | "TRUE" | "yes" | "y" | "on" => true,
        "0" | "false" | "False" | "FALSE" | "no" | "n" | "off" => false,
        _ => {
            return Err(ArgError {
                kind: ArgErrorKind::BadValue {
                    name,
                    ty: ArgType::Bool,
                    value: alloc::string::String::from(tok),
                    hint: "expected bool: true/false, 1/0, yes/no, on/off",
                },
            })
        }
    };
    Ok(ArgValue::Bool(v))
}

fn parse_u(tok: &str) -> Result<u64, &'static str> {
    let t = tok.trim();
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).map_err(|_| "expected unsigned integer (dec or 0xHEX)")
    } else {
        t.parse::<u64>().map_err(|_| "expected unsigned integer (dec or 0xHEX)")
    }
}

fn parse_i(tok: &str) -> Result<i64, &'static str> {
    let t = tok.trim();
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        let u = u64::from_str_radix(hex, 16).map_err(|_| "expected integer (dec or 0xHEX)")?;
        i64::try_from(u).map_err(|_| "value out of range")
    } else {
        t.parse::<i64>().map_err(|_| "expected integer (dec or 0xHEX)")
    }
}

#[inline]
fn style_cmd_name(name: &str) -> impl core::fmt::Display + '_ {
    crate::ecma48::bold(name)
}

#[inline]
fn style_arg_name(name: &str) -> impl core::fmt::Display + '_ {
    crate::ecma48::color(name, super::PROMPT_RGB)
}

#[inline]
fn style_arg_type(ty: ArgType) -> impl core::fmt::Display {
    crate::ecma48::dim(ty.name())
}

#[inline]
fn style_error_label(text: &str) -> impl core::fmt::Display + '_ {
    crate::ecma48::color(text, (255, 96, 96))
}

fn print_usage(io: &dyn ShellIo, cmd: &ShellCommand) {
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
        static ARGS_ARGS: [ArgSpec; 1] = [ArgSpec::new("command", ArgType::Str).mandatory()];
        static ECMA48_ARGS: [ArgSpec; 1] = [ArgSpec::new("arg", ArgType::Rest)];
        static GET_ARGS: [ArgSpec; 1] = [ArgSpec::new("url", ArgType::Str).mandatory()];
        static HTTPS_ARGS: [ArgSpec; 1] = [ArgSpec::new("host", ArgType::Str)];
        static NET_ARGS: [ArgSpec; 2] = [
            ArgSpec::new("op", ArgType::Str).mandatory(),
            ArgSpec::new("target", ArgType::Str),
        ];
        static NO_ARGS: [ArgSpec; 0] = [];
        static QJS_ARGS: [ArgSpec; 1] = [ArgSpec::new("src", ArgType::Rest)];
        static MV_ARGS: [ArgSpec; 2] = [
            ArgSpec::new("src", ArgType::Str).mandatory(),
            ArgSpec::new("dst", ArgType::Str).mandatory(),
        ];
        static IDLE_ARGS: [ArgSpec; 1] = [ArgSpec::new("policy", ArgType::Str)];
        static PSTATE_ARGS: [ArgSpec; 1] = [ArgSpec::new("ratio", ArgType::U8)];
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

        let _ = REGSHCMD("args", &ARGS_ARGS, builtin_args);
        let _ = REGSHCMD("§", &SECTION_ARGS, cmd_section);
        let _ = REGSHCMD("ecma48", &ECMA48_ARGS, cmd_ecma48);
        let _ = REGSHCMD("get", &GET_ARGS, cmd_get);
        let _ = REGSHCMD("https", &HTTPS_ARGS, cmd_https);
        let _ = REGSHCMD("net", &NET_ARGS, cmd_net);
        let _ = REGSHCMD("update", &NO_ARGS, cmd_update);
        let _ = REGSHCMD("install", &[], cmd_install);
        let _ = REGSHCMD("format", &NO_ARGS, cmd_format);
        let _ = REGSHCMD("file", &FILE_ARGS, cmd_file);
        let _ = REGSHCMD("qjs", &QJS_ARGS, cmd_qjs);
        let _ = REGSHCMD("mv", &MV_ARGS, cmd_mv);
        let _ = REGSHCMD("reset", &[], cmd_reset);
        let _ = REGSHCMD("s5", &[], cmd_s5);
        let _ = REGSHCMD("go", &[], cmd_go);
        let _ = REGSHCMD("mandel", &[], cmd_mandel);
        let _ = REGSHCMD("time", &[], cmd_time);
        let _ = REGSHCMD("set", &SET_ARGS, cmd_set);
        let _ = REGSHCMD("idle", &IDLE_ARGS, cmd_idle);
        let _ = REGSHCMD("pstate", &PSTATE_ARGS, cmd_pstate);
        let _ = REGSHCMD("turbo", &TURBO_ARGS, cmd_turbo);
        let _ = REGSHCMD("smp", &SMP_ARGS, cmd_smp);
        let _ = REGSHCMD("cube", &[], cmd_cube);
        let _ = REGSHCMD("ico", &[], cmd_ico);
        let _ = REGSHCMD("txt", &NO_ARGS, cmd_txt);
        let _ = REGSHCMD("insane", &[], cmd_insane);
        let _ = REGSHCMD("usb", &[], cmd_usb);
        let _ = REGSHCMD("pci", &[], cmd_pci);
    });
}

fn cmd_section(ctx: &mut ShellCommandCtx<'_>, args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    // No args: list slots.
    let Some(args) = args else {
        let mut buf: heapless::String<512> = heapless::String::new();
        crate::matrix::list_slots(&mut buf);
        ctx.io.write_str(buf.as_str());
        return super::CommandAction::None;
    };

    // With id: dump + free.
    let id = args.get(0).and_then(|v| v.as_u8()).unwrap_or(0);
    if id == 0 {
        ctx.io.write_str("§: ids are 1..\r\n");
        return super::CommandAction::None;
    }
    let slot_id = id - 1;

    // Prefer dumping the slot blob (full, untruncated). This is what `get`/`https`
    // jobs store their main payload in.
    if let Some((state, title, has_blob, blob_len)) = crate::matrix::with_slot(slot_id, |s| {
        (s.state, s.title.clone(), !s.blob.is_empty(), s.blob.len())
    }) {
        if has_blob {
            let blob = crate::matrix::take_blob(slot_id).unwrap_or_default();
            ctx.io.write_fmt(format_args!("#{} {:?} {}\r\n", id, state, title.as_str()));
            ctx.io.write_fmt(format_args!("(blob {} bytes)\r\n", blob_len));
            for &b in blob.iter() {
                ctx.io.write_byte(b);
            }
            if !blob.ends_with(b"\n") {
                ctx.io.write_str("\r\n");
            }
            let _ = crate::matrix::free_slot(slot_id);
            crate::matrix::refresh_matrix_symbols(ctx.io, *ctx.term_cols);
            return super::CommandAction::None;
        }
    }

    // Fallback: preview dump (limited, but useful for non-blob jobs).
    let mut buf: heapless::String<1024> = heapless::String::new();
    if crate::matrix::dump_slot(&mut buf, slot_id) {
        ctx.io.write_str(buf.as_str());
        let _ = crate::matrix::free_slot(slot_id);
        crate::matrix::refresh_matrix_symbols(ctx.io, *ctx.term_cols);
    } else {
        ctx.io.write_str("§: not found\r\n");
    }

    super::CommandAction::None
}

fn lookup_registered(name: &str) -> Option<&'static ShellCommand> {
    let cmds = registry().lock();
    find_command(cmds.as_slice(), name)
}

fn builtin_args(ctx: &mut ShellCommandCtx<'_>, args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    let Some(args) = args else {
        ctx.io.write_str("args: usage args <command>\r\n");
        return super::CommandAction::None;
    };
    let Some(ArgValue::Str(name)) = args.get(0) else {
        ctx.io.write_str("args: internal parse error\r\n");
        return super::CommandAction::None;
    };

    let Some(cmd) = lookup_registered(name) else {
        ctx.io.write_str("args: unknown command\r\n");
        return super::CommandAction::None;
    };

    print_usage(ctx.io, cmd);
    print_schema(ctx.io, cmd);
    super::CommandAction::None
}

fn cmd_ecma48(ctx: &mut ShellCommandCtx<'_>, args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    let arg = args
        .and_then(|a| a.get(0))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    super::ecma48::handle_ecma48(ctx.io, arg);
    super::CommandAction::None
}

fn cmd_get(ctx: &mut ShellCommandCtx<'_>, args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    let url = args.and_then(|a| a.get(0)).and_then(|v| v.as_str()).unwrap_or("");
    if url.is_empty() {
        ctx.io.write_str("get: usage get <host|http://url>\r\n");
        ctx.io.write_str("get: example get http://example.com/\r\n");
        ctx.io.write_str("get: note: plaintext HTTP only (no TLS)\r\n");
        return super::CommandAction::None;
    }

    let mut title: heapless::String<{ crate::matrix::TITLE_LEN }> = heapless::String::new();
    let _ = title.push_str("get ");
    for ch in url.chars() {
        if title.push(ch).is_err() {
            break;
        }
    }

    match crate::matrix::alloc_slot(title.as_str()) {
        Some(slot) => {
            let mut u: heapless::String<256> = heapless::String::new();
            for ch in url.chars() {
                if u.push(ch).is_err() {
                    break;
                }
            }
                let _ = crate::v::taskmon::spawn(
                    ctx.spawner,
                    "html-get-matrix",
                    crate::tst::html::http_get_matrix_job(slot, u),
                );
            ctx.io.write_fmt(format_args!("get: started §{}\r\n", slot + 1));
            crate::matrix::refresh_matrix_symbols(ctx.io, *ctx.term_cols);
        }
        None => ctx.io.write_str("get: matrix full\r\n"),
    }

    super::CommandAction::None
}

fn cmd_https(ctx: &mut ShellCommandCtx<'_>, args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    let host = args.and_then(|a| a.get(0)).and_then(|v| v.as_str()).unwrap_or("");

    let mut title: heapless::String<{ crate::matrix::TITLE_LEN }> = heapless::String::new();
    let _ = title.push_str("https ");
    let show_host = if host.is_empty() { "example.com" } else { host };
    for ch in show_host.chars() {
        if title.push(ch).is_err() {
            break;
        }
    }

    match crate::matrix::alloc_slot(title.as_str()) {
        Some(slot) => {
            let mut h: heapless::String<96> = heapless::String::new();
            for ch in host.chars() {
                if h.push(ch).is_err() {
                    break;
                }
            }
                let _ = crate::v::taskmon::spawn(
                    ctx.spawner,
                    "tls-demo-matrix",
                    crate::tst::tls_demo::tls_demo_matrix_job(slot, h),
                );
            ctx.io.write_fmt(format_args!("https: started §{}\r\n", slot + 1));
            crate::matrix::refresh_matrix_symbols(ctx.io, *ctx.term_cols);
        }
        None => ctx.io.write_str("https: matrix full\r\n"),
    }

    super::CommandAction::None
}

fn cmd_net(ctx: &mut ShellCommandCtx<'_>, args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    let Some(args) = args else {
        ctx.io.write_str("net: usage net ping <host>\r\n");
        ctx.io.write_str("net: usage net mac [index]\r\n");
        return super::CommandAction::None;
    };

    let op = args.get(0).and_then(|v| v.as_str()).unwrap_or("");
    let target = args.get(1).and_then(|v| v.as_str()).unwrap_or("");

    if op.eq_ignore_ascii_case("mac") {
        if target.is_empty() {
            let count = crate::net::device_count();
            if count == 0 {
                ctx.io.write_str("net: no nics\r\n");
                return super::CommandAction::None;
            }
            for index in 0..count {
                let mac = if index == 0 {
                    crate::net::mac_address()
                } else {
                    crate::net::mac_address_at(index)
                };
                if let Some([a, b, c, d, e, f]) = mac {
                    ctx.io.write_fmt(format_args!(
                        "net: mac[{}]={:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}\r\n",
                        index, a, b, c, d, e, f
                    ));
                } else {
                    ctx.io.write_fmt(format_args!("net: mac[{}]=unavailable\r\n", index));
                }
            }
        } else if let Ok(index) = target.parse::<usize>() {
            let mac = if index == 0 {
                crate::net::mac_address()
            } else {
                crate::net::mac_address_at(index)
            };

            if let Some([a, b, c, d, e, f]) = mac {
                ctx.io.write_fmt(format_args!(
                    "net: mac[{}]={:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}\r\n",
                    index, a, b, c, d, e, f
                ));
            } else {
                ctx.io.write_fmt(format_args!("net: mac[{}]=unavailable\r\n", index));
            }
        } else {
            ctx.io.write_str("net: usage net mac [index]\r\n");
        }
        return super::CommandAction::None;
    }

    if op != "ping" || target.is_empty() {
        ctx.io.write_str("net: usage net ping <host>\r\n");
        ctx.io.write_str("net: usage net mac [index]\r\n");
        return super::CommandAction::None;
    }

    ctx.io.write_fmt(format_args!("net: ping {}\r\n", target));
    let mut t: heapless::String<64> = heapless::String::new();
    for ch in target.chars() {
        if t.push(ch).is_err() {
            break;
        }
    }
        if crate::v::taskmon::spawn(ctx.spawner, "net-ping", net_ping_task(ctx.io, t)).is_err() {
            ctx.io.write_str("net: ping spawn failed\r\n");
    }

    super::CommandAction::None
}

#[task]
async fn net_ping_task(io: &'static dyn ShellBackend, target: heapless::String<64>) {
    let res = crate::v::net::ping::ping_once(target.as_str()).await;
    match res {
        Ok(result) => {
            let [a, b, c, d] = result.ip;
            io.write_fmt(format_args!(
                "net: reply from {}.{}.{}.{} rtt={}ms\r\n",
                a, b, c, d, result.rtt_ms
            ));
        }
        Err(err) => match err {
            crate::v::net::ping::PingError::NoNic => {
                io.write_str("net: ping failed (no nic)\r\n");
            }
            crate::v::net::ping::PingError::BadHost => {
                io.write_str("net: ping failed (bad host)\r\n");
            }
            crate::v::net::ping::PingError::DnsFailed => {
                io.write_str("net: ping failed (dns)\r\n");
            }
            crate::v::net::ping::PingError::Timeout => {
                io.write_str("net: ping timeout\r\n");
            }
            crate::v::net::ping::PingError::SendFailed => {
                io.write_str("net: ping failed (send)\r\n");
            }
        },
    }
}

fn cmd_update(ctx: &mut ShellCommandCtx<'_>, _args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    *ctx.install_wizard = Some(super::InstallWizardStage::UpdateSelectDisk);
    super::CommandAction::ShowUpdateDiskTable
}

fn cmd_install(ctx: &mut ShellCommandCtx<'_>, _args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    *ctx.install_wizard = Some(super::InstallWizardStage::SelectDisk);
    super::CommandAction::ShowInstallDiskTable
}

fn cmd_format(ctx: &mut ShellCommandCtx<'_>, _args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    *ctx.install_wizard = Some(super::InstallWizardStage::FormatSelectDisk);
    super::CommandAction::ShowFormatDiskTable
}

fn cmd_file(ctx: &mut ShellCommandCtx<'_>, _args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    *ctx.install_wizard = Some(super::InstallWizardStage::FileSelectMount);
    super::CommandAction::ShowFileMountTable
}

fn cmd_qjs(ctx: &mut ShellCommandCtx<'_>, args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    let src = args.and_then(|a| a.get(0)).and_then(|v| v.as_str()).unwrap_or("");
    let src = src.trim();
    if src.is_empty() {
        super::shellqjs::help(ctx.io);
        super::CommandAction::None
    } else {
        let mut buf: heapless::String<192> = heapless::String::new();
        for ch in src.chars() {
            if buf.push(ch).is_err() {
                break;
            }
        }
        super::CommandAction::Qjs { src: buf }
    }
}

fn cmd_mv(ctx: &mut ShellCommandCtx<'_>, args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    let Some(args) = args else {
        ctx.io.write_str("mv: usage mv <src> <dst>\r\n");
        return super::CommandAction::None;
    };
    let src = args.get(0).and_then(|v| v.as_str()).unwrap_or("");
    let dst = args.get(1).and_then(|v| v.as_str()).unwrap_or("");
    if src.is_empty() || dst.is_empty() {
        ctx.io.write_str("mv: usage mv <src> <dst>\r\n");
        return super::CommandAction::None;
    }

    fn path_160(s: &str) -> heapless::String<160> {
        let mut out: heapless::String<160> = heapless::String::new();
        for ch in s.chars() {
            if out.push(ch).is_err() {
                break;
            }
        }
        out
    }

    super::CommandAction::Mv {
        src: path_160(src),
        dst: path_160(dst),
    }
}

fn cmd_reset(_ctx: &mut ShellCommandCtx<'_>, _args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    super::CommandAction::Pending(super::PendingAction::Reset)
}

fn cmd_s5(_ctx: &mut ShellCommandCtx<'_>, _args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    super::CommandAction::Pending(super::PendingAction::S5)
}

fn cmd_go(ctx: &mut ShellCommandCtx<'_>, _args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    super::set_go_mode(ctx.io, ctx.go_mode, true);
    super::CommandAction::None
}

fn cmd_mandel(ctx: &mut ShellCommandCtx<'_>, _args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    crate::vga::draw_mandelbrot();
    ctx.io.write_str("mandel ok\r\n");
    super::CommandAction::None
}

fn cmd_time(ctx: &mut ShellCommandCtx<'_>, _args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    if let Some(ts) = crate::time::unix_time_seconds() {
        let (year, month, day, hour, minute, second) = super::unix_timestamp_to_ymdhms(ts);
        let mut buf: heapless::String<64> = heapless::String::new();
        let _ = write!(
            &mut buf,
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
            year,
            month,
            day,
            hour,
            minute,
            second
        );
        ctx.io.write_fmt(format_args!("{}\r\n", crate::ecma48::underline(buf.as_str())));
    } else {
        ctx.io.write_str("time: boot timestamp unavailable\r\n");
    }
    super::CommandAction::None
}

fn cmd_set(ctx: &mut ShellCommandCtx<'_>, args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    let Some(args) = args else {
        ctx.io.write_str("set: usage set <cols> <rows>\r\n");
        return super::CommandAction::None;
    };

    let cols = args.get(0).and_then(|v| v.as_usize()).unwrap_or(0);
    let rows = args.get(1).and_then(|v| v.as_usize()).unwrap_or(0);

    if cols == 0 || rows == 0 {
        ctx.io.write_str("set: cols/rows must be >= 1\r\n");
        ctx.io.write_str("usage: set <cols> <rows>\r\n");
        return super::CommandAction::None;
    }

    *ctx.term_cols = cols;
    *ctx.term_rows = rows;

    let mut buf: heapless::String<64> = heapless::String::new();
    let _ = write!(&mut buf, "term set: {}x{}\r\n", cols, rows);
    ctx.io.write_str(buf.as_str());
    super::draw_corners(ctx.io, cols, rows);
    super::CommandAction::None
}

fn cmd_idle(ctx: &mut ShellCommandCtx<'_>, args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    let policy = args.and_then(|a| a.get(0)).and_then(|v| v.as_str()).unwrap_or("").trim();
    if policy.is_empty() {
        ctx.io
            .write_fmt(format_args!("idle: {}\r\n", crate::power::idle_policy().as_str()));
        return super::CommandAction::None;
    }

    let policy = match policy {
        "spin" => crate::power::IdlePolicy::Spin,
        "hlt" => crate::power::IdlePolicy::Halt,
        _ => {
            ctx.io.write_str("idle: usage idle [spin|hlt]\r\n");
            return super::CommandAction::None;
        }
    };
    let prev = crate::power::set_idle_policy(policy);
    ctx.io.write_fmt(format_args!(
        "idle: {} -> {}\r\n",
        prev.as_str(),
        policy.as_str()
    ));
    super::CommandAction::None
}

fn cmd_pstate(ctx: &mut ShellCommandCtx<'_>, args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    let ratio = args.and_then(|a| a.get(0)).and_then(|v| v.as_u64());

    if ratio.is_none() {
        let cur = crate::power::current_ratio();
        let armed = crate::power::msr_armed();
        let details = crate::power::msr_details().copied();

        match (cur, armed, details) {
            (Some(cur), true, Some(d)) => ctx.io.write_fmt(format_args!(
                "pstate: current={} min={} max={}\r\n",
                cur,
                d.min_ratio.unwrap_or(0),
                d.max_ratio.unwrap_or(0)
            )),
            (_, false, _) => ctx.io.write_str("pstate: msr disarmed\r\n"),
            (_, true, None) => ctx.io.write_str("pstate: msr details not probed\r\n"),
            _ => ctx.io.write_str("pstate: unsupported\r\n"),
        }
        return super::CommandAction::None;
    }

    let req_u64 = ratio.unwrap();
    let Ok(req) = u8::try_from(req_u64) else {
        ctx.io.write_str("pstate: usage pstate <ratio>\r\n");
        return super::CommandAction::None;
    };

    match crate::power::set_pstate_ratio(req) {
        Ok(applied) => ctx.io.write_fmt(format_args!("pstate: applied {}\r\n", applied)),
        Err(err) => ctx.io.write_fmt(format_args!("pstate: failed: {}\r\n", err)),
    }

    super::CommandAction::None
}

fn smp_state_name(st: u8) -> &'static str {
    match st {
        crate::smp::STATE_IDLE => "idle",
        crate::smp::STATE_PENDING => "pending",
        crate::smp::STATE_RUNNING => "running",
        crate::smp::STATE_DONE => "done",
        _ => "unknown",
    }
}

fn cmd_smp(ctx: &mut ShellCommandCtx<'_>, args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    if !crate::smp::is_init() {
        ctx.io.write_str("smp: not initialized\r\n");
        return super::CommandAction::None;
    }

    let total = crate::smp::cpu_count();
    ctx.io
        .write_fmt(format_args!("smp: cpu_count={}\r\n", total));

    let slot_opt = args.and_then(|a| a.get(0)).and_then(|v| v.as_usize());

    let dump_slot = |slot: usize| {
        let Some(r) = crate::smp::read(slot) else {
            ctx.io
                .write_fmt(format_args!("smp: slot={} <unavailable>\r\n", slot));
            return;
        };
        ctx.io.write_fmt(format_args!(
            "smp: slot={} online={} state={} seq={} ret=0x{:016X}\r\n",
            slot,
            if r.online { 1 } else { 0 },
            smp_state_name(r.state),
            r.seq,
            r.ret
        ));
    };

    if let Some(slot) = slot_opt {
        if slot >= total {
            ctx.io.write_str("smp: usage smp [slot]\r\n");
            return super::CommandAction::None;
        }
        dump_slot(slot);
        return super::CommandAction::None;
    }

    for slot in 0..total {
        dump_slot(slot);
    }

    super::CommandAction::None
}

fn cmd_turbo(ctx: &mut ShellCommandCtx<'_>, args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    let op = args
        .and_then(|a| a.get(0))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();

    if op.is_empty() || op.eq_ignore_ascii_case("status") {
        let armed = crate::turbo::armed();
        match crate::turbo::local_state() {
            Ok(st) => {
                ctx.io.write_fmt(format_args!("turbo: armed={} state={:?}\r\n", armed, st));
            }
            Err(crate::turbo::TurboSetError::Unsupported) => {
                ctx.io.write_fmt(format_args!("turbo: unsupported (intel-only)\r\n"));
            }
            Err(crate::turbo::TurboSetError::Disarmed) => {
                // Reads should never require arming; keep for forward-compat.
                ctx.io.write_fmt(format_args!("turbo: disarmed\r\n"));
            }
        }
        if !armed {
            ctx.io.write_str("turbo: writes are disarmed (run 'turbo arm')\r\n");
        }
        return super::CommandAction::None;
    }

    if op.eq_ignore_ascii_case("arm") {
        crate::turbo::set_armed(true);
        ctx.io.write_str("turbo: armed\r\n");
        return super::CommandAction::None;
    }
    if op.eq_ignore_ascii_case("disarm") {
        crate::turbo::set_armed(false);
        ctx.io.write_str("turbo: disarmed\r\n");
        return super::CommandAction::None;
    }

    if op.eq_ignore_ascii_case("verify") {
        let spins = args
            .and_then(|a| a.get(1))
            .and_then(|v| v.as_usize())
            .unwrap_or(200_000);

        match crate::turbo::verify_all(spins) {
            Ok(r) => {
                ctx.io.write_fmt(format_args!(
                    "turbo: verify spins={} turbo={} noturbo={} unknown={} completed_aps={}/{} online_aps={} busy={} total_cpus={} seq={}{}\r\n",
                    spins,
                    r.turbo_cpus,
                    r.noturbo_cpus,
                    r.unknown_cpus,
                    r.completed_aps,
                    r.submitted_aps,
                    r.online_aps,
                    r.busy_aps,
                    r.total_cpus,
                    r.seq,
                    if r.timed_out { " TIMEOUT" } else { "" }
                ));
            }
            Err(crate::turbo::TurboSetError::Disarmed) => {
                // verify is read-only; keep for forward-compat and clarity.
                ctx.io.write_str("turbo: msr disarmed (verify should not require arm)\r\n");
            }
            Err(crate::turbo::TurboSetError::Unsupported) => {
                ctx.io.write_str("turbo: unsupported (intel-only)\r\n");
            }
        }

        return super::CommandAction::None;
    }

    let enable = if op.eq_ignore_ascii_case("on") {
        Some(true)
    } else if op.eq_ignore_ascii_case("off") {
        Some(false)
    } else {
        None
    };

    let Some(enable) = enable else {
        ctx.io.write_str("turbo: usage turbo [status|arm|disarm|on|off|verify [spins]]\r\n");
        return super::CommandAction::None;
    };

    match crate::turbo::set_enabled_all(enable) {
        Ok(r) => {
            ctx.io.write_fmt(format_args!(
                "turbo: requested={} ap_submitted={}/{} busy={} total_cpus={} seq={}\r\n",
                if r.requested_enable { "on" } else { "off" },
                r.submitted_aps,
                r.targeted_aps,
                r.busy_aps,
                r.total_cpus,
                r.seq
            ));
        }
        Err(crate::turbo::TurboSetError::Disarmed) => {
            ctx.io.write_str("turbo: msr disarmed (run 'turbo arm')\r\n");
        }
        Err(crate::turbo::TurboSetError::Unsupported) => {
            ctx.io.write_str("turbo: unsupported (intel-only)\r\n");
        }
    }

    super::CommandAction::None
}

fn cmd_cube(_ctx: &mut ShellCommandCtx<'_>, _args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    super::CommandAction::EnterCube
}

fn cmd_ico(_ctx: &mut ShellCommandCtx<'_>, _args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    super::CommandAction::EnterIco
}

fn cmd_txt(ctx: &mut ShellCommandCtx<'_>, args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    let arg = args.and_then(|a| a.get(0)).and_then(|v| v.as_str()).unwrap_or("").trim();

    if !arg.is_empty() {
        ctx.io.write_str("txt: argument no longer supported\r\n");
    }

    let Some(slot_id) = crate::matrix::alloc_slot("txt") else {
        ctx.io.write_str("txt: matrix full\r\n");
        return super::CommandAction::None;
    };

    let mut filename: heapless::String<48> = heapless::String::new();
    let _ = write!(filename, "§{}", slot_id + 1);
    super::CommandAction::EnterTxtEdt { filename, slot_id }
}

fn cmd_insane(ctx: &mut ShellCommandCtx<'_>, _args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    let cols = (*ctx.term_cols).max(1);
    ctx.io.write_str("insane: iterating U+0000..=U+10FFFF (Ctrl-C to abort)\r\n");

    let mut col: usize = 0;
    for cp in 0u32..=0x10FFFF {
        if (cp & 0x3FF) == 0 {
            if let Some(b) = ctx.io.read_byte() {
                if b == 0x03 {
                    ctx.io.write_str("\r\ninsane: aborted\r\n");
                    return super::CommandAction::None;
                }
            }
        }

        let ch = match core::char::from_u32(cp) {
            Some(ch) if !ch.is_control() => ch,
            Some(_) => '.',
            None => '�',
        };

        ctx.io.write_char(ch);

        col += 1;
        if col >= cols {
            ctx.io.write_str("\r\n");
            col = 0;
        }
    }

    if col != 0 {
        ctx.io.write_str("\r\n");
    }
    ctx.io.write_str("insane: done\r\n");
    super::CommandAction::None
}

fn cmd_usb(ctx: &mut ShellCommandCtx<'_>, _args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    let sub = _args
        .and_then(|a| a.get(0))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();

    if sub == "dump" {
        ctx.io.write_str(
            "usb: targeted descriptor dump is printed automatically when an unclaimed device matches vid=0x0416 pid=0xA125 (JGINYUE 'LED SheBei').\r\n",
        );
        ctx.io.write_str(
            "usb: replug the device (or reboot) to re-trigger enumeration.\r\n",
        );
        return super::CommandAction::None;
    }

    let ctrls = crate::usb::xhci::xhc_list();
    if ctrls.is_empty() {
        ctx.io.write_str("usb: no xhci controllers\r\n");
        return super::CommandAction::None;
    }

    for info in ctrls.iter() {
        ctx.io.write_fmt(format_args!(
            "usb: xHCI {} {:02X}:{:02X}.{} bar0=0x{:X} size=0x{:X} ac64={}\r\n",
            info.controller_id,
            info.bus,
            info.slot,
            info.function,
            info.bar_phys,
            info.bar_size,
            info.supports_64bit
        ));

        let devs = crate::usb::list_device_summaries(info.controller_id);
        if devs.is_empty() {
            ctx.io.write_str("  (no devices)\r\n");
            continue;
        }

        for d in devs.iter() {
            ctx.io.write_fmt(format_args!(
                "  port={} slot={} kind={} vid=0x{:04X} pid=0x{:04X} cls={:02X}/{:02X}/{:02X}\r\n",
                d.port,
                d.slot_id,
                d.kind,
                d.vid.unwrap_or(0),
                d.pid.unwrap_or(0),
                d.class.unwrap_or(0),
                d.subclass.unwrap_or(0),
                d.protocol.unwrap_or(0)
            ));
        }
    }

    super::CommandAction::None
}

fn cmd_pci(ctx: &mut ShellCommandCtx<'_>, _args: Option<&ParsedArgs<'_>>) -> super::CommandAction {
    let mut len: usize = 0;
    crate::pci::with_devices(|list| {
        len = list.len();
    });
    if len == 0 {
        crate::pci::enumerate_silent();
    }

    // Optional enrichment via cached `pci.ids`.
    // Keep this best-effort and preserve the existing output when missing.
    let pci_ids_db = crate::pci::pciids::load_sanitized_from_root_blocking().ok().flatten();

    crate::pci::with_devices(|list| {
        ctx.io.write_fmt(format_args!("pci: devices={}\r\n", list.len()));
        if list.is_empty() {
            ctx.io.write_str("pci: no devices\r\n");
            return;
        }

        fn walk_caps(
            bus: u8,
            slot: u8,
            function: u8,
        ) -> (bool, bool, Option<(u8, u8)>) {
            // Returns (has_msi, has_msix, pcie_link(gen, width)).
            let status = crate::pci::config_read_u16(bus, slot, function, 0x06);
            let has_caps = (status & (1 << 4)) != 0;
            if !has_caps {
                return (false, false, None);
            }

            let mut has_msi = false;
            let mut has_msix = false;
            let mut pcie_link: Option<(u8, u8)> = None;

            // Standard capability list is single-byte pointer at 0x34.
            let mut cap_ptr = crate::pci::config_read_u8(bus, slot, function, 0x34) & 0xFC;
            let mut iters = 0u8;
            while cap_ptr >= 0x40 && cap_ptr <= 0xFC && iters < 48 {
                iters = iters.wrapping_add(1);
                let cap_id = crate::pci::config_read_u8(bus, slot, function, cap_ptr as u16);
                let next = crate::pci::config_read_u8(bus, slot, function, (cap_ptr as u16) + 1) & 0xFC;

                match cap_id {
                    0x05 => has_msi = true,
                    0x11 => has_msix = true,
                    0x10 => {
                        // PCI Express Capability.
                        // Link Status is at cap+0x12 (16-bit): speed[3:0], width[9:4].
                        let link_status = crate::pci::config_read_u16(
                            bus,
                            slot,
                            function,
                            (cap_ptr as u16) + 0x12,
                        );
                        let speed = (link_status & 0x000F) as u8;
                        let width = ((link_status >> 4) & 0x003F) as u8;
                        if speed != 0 && width != 0 {
                            pcie_link = Some((speed, width));
                        }
                    }
                    _ => {}
                }

                if next == 0 || next == cap_ptr {
                    break;
                }
                cap_ptr = next;
            }

            (has_msi, has_msix, pcie_link)
        }

        for dev in list.iter() {
            let (bar0_lo, bar0_hi) = crate::pci::read_bar0_raw(dev.bus, dev.slot, dev.function);
            let irq_line = crate::pci::config_read_u8(dev.bus, dev.slot, dev.function, 0x3C);
            let irq_pin = crate::pci::config_read_u8(dev.bus, dev.slot, dev.function, 0x3D);
            let rev = crate::pci::config_read_u8(dev.bus, dev.slot, dev.function, 0x08);
            let subsys_vid = crate::pci::config_read_u16(dev.bus, dev.slot, dev.function, 0x2C);
            let subsys_did = crate::pci::config_read_u16(dev.bus, dev.slot, dev.function, 0x2E);
            let (has_msi, has_msix, pcie_link) = walk_caps(dev.bus, dev.slot, dev.function);

            let name_suffix = if let Some(db) = pci_ids_db.as_deref() {
                if let Some((v, d)) = crate::pci::pciids::lookup_vendor_device_from_db(
                    db,
                    dev.vendor,
                    dev.device,
                ) {
                    let v = alloc::string::String::from(alloc::string::String::from_utf8_lossy(v).trim());
                    let d = alloc::string::String::from(alloc::string::String::from_utf8_lossy(d).trim());
                    alloc::format!(" name=\"{} {}\"", v, d)
                } else {
                    alloc::string::String::new()
                }
            } else {
                alloc::string::String::new()
            };

            if let Some(hi) = bar0_hi {
                ctx.io.write_fmt(format_args!(
                    "pci: {:02X}:{:02X}.{} vid=0x{:04X} did=0x{:04X} subsys=0x{:04X}:0x{:04X} cls={:02X}/{:02X}/{:02X} rev=0x{:02X} msi={} msix={} pcie={:?} bar0=0x{:08X}{:08X} irq_line={} irq_pin={}{}\r\n",
                    dev.bus,
                    dev.slot,
                    dev.function,
                    dev.vendor,
                    dev.device,
                    subsys_vid,
                    subsys_did,
                    dev.class,
                    dev.subclass,
                    dev.prog_if,
                    rev,
                    has_msi,
                    has_msix,
                    pcie_link,
                    hi,
                    bar0_lo,
                    irq_line,
                    irq_pin,
                    name_suffix,
                ));
            } else {
                ctx.io.write_fmt(format_args!(
                    "pci: {:02X}:{:02X}.{} vid=0x{:04X} did=0x{:04X} subsys=0x{:04X}:0x{:04X} cls={:02X}/{:02X}/{:02X} rev=0x{:02X} msi={} msix={} pcie={:?} bar0=0x{:08X} irq_line={} irq_pin={}{}\r\n",
                    dev.bus,
                    dev.slot,
                    dev.function,
                    dev.vendor,
                    dev.device,
                    subsys_vid,
                    subsys_did,
                    dev.class,
                    dev.subclass,
                    dev.prog_if,
                    rev,
                    has_msi,
                    has_msix,
                    pcie_link,
                    bar0_lo,
                    irq_line,
                    irq_pin,
                    name_suffix,
                ));
            }
        }
    });

    super::CommandAction::None
}
