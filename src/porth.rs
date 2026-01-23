#![allow(dead_code)]

extern crate alloc;

use alloc::string::String as AString;
use alloc::vec::Vec as AVec;
use heapless::Vec;
use spin::Mutex;

const MAX_STACK: usize = 64;
const MAX_TOKENS: usize = 64;
const BYTECODE_MAGIC: &[u8; 4] = b"TPBC";
const BYTECODE_VERSION: u8 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompileError {
    UnknownWord,
    UnmatchedControl,
    ProgramTooLarge,
}

// Bytecode opcodes (must match Vm::exec_bytecode)
const OP_HALT: u8 = 0;
const OP_PUSH_I64: u8 = 1;
const OP_ADD: u8 = 2;
const OP_SUB: u8 = 3;
const OP_MUL: u8 = 4;
const OP_DIV: u8 = 5;
const OP_MOD: u8 = 6;
const OP_DUP: u8 = 7;
const OP_DROP: u8 = 8;
const OP_SWAP: u8 = 9;
const OP_OVER: u8 = 10;
const OP_ROT: u8 = 11;
const OP_EQ: u8 = 12;
const OP_LT: u8 = 13;
const OP_GT: u8 = 14;
const OP_DOT: u8 = 15;
const OP_DOTS: u8 = 16;
const OP_EMIT: u8 = 17;
const OP_CLEAR: u8 = 18;
const OP_JZ_REL32: u8 = 19;
const OP_JMP_REL32: u8 = 20;

const MAX_PROGRAM_BYTES: usize = 256 * 1024;

const PORTH_COMPILE_MAX_SRC_BYTES: usize = 16 * 1024;

struct PorthCompileSession {
    name: heapless::String<48>,
    src: AString,
}

static PORTH_COMPILE: Mutex<Option<PorthCompileSession>> = Mutex::new(None);
static PORTH_REPL: Mutex<bool> = Mutex::new(false);

#[derive(Clone, Copy, Debug)]
enum Frame {
    If { jz_pos: usize },
    Else { jmp_pos: usize },
    Begin { start_ip: usize },
}

fn tpbc_wrap(code: &[u8]) -> Result<AVec<u8>, CompileError> {
    if code.len() > (MAX_PROGRAM_BYTES.saturating_sub(12)) {
        return Err(CompileError::ProgramTooLarge);
    }
    let mut out: AVec<u8> = AVec::new();
    out.extend_from_slice(BYTECODE_MAGIC);
    out.push(BYTECODE_VERSION);
    out.push(0); // flags
    out.extend_from_slice(&0u16.to_le_bytes()); // reserved
    out.extend_from_slice(&(code.len() as u32).to_le_bytes());
    out.extend_from_slice(code);
    Ok(out)
}

fn emit_push_i64(code: &mut AVec<u8>, v: i64) {
    code.push(OP_PUSH_I64);
    code.extend_from_slice(&v.to_le_bytes());
}

fn emit_rel32_placeholder(code: &mut AVec<u8>, opcode: u8) -> usize {
    code.push(opcode);
    let rel_pos = code.len();
    code.extend_from_slice(&0i32.to_le_bytes());
    rel_pos
}

fn patch_rel32(code: &mut [u8], rel_pos: usize, target_ip: usize) {
    // rel_pos points to the start of the i32 immediate (right after opcode).
    let ip_after = rel_pos + 4;
    let rel = (target_ip as isize).wrapping_sub(ip_after as isize) as i32;
    code[rel_pos..rel_pos + 4].copy_from_slice(&rel.to_le_bytes());
}

pub fn compile_source_to_tpbc(src: &str) -> Result<alloc::vec::Vec<u8>, CompileError> {
    let mut code: AVec<u8> = AVec::new();
    let mut frames: AVec<Frame> = AVec::new();

    for token in src.split_whitespace() {
        if let Some(v) = parse_number(token) {
            emit_push_i64(&mut code, v);
        } else {
            match token {
                "+" => code.push(OP_ADD),
                "-" => code.push(OP_SUB),
                "*" => code.push(OP_MUL),
                "/" => code.push(OP_DIV),
                "mod" => code.push(OP_MOD),
                "dup" => code.push(OP_DUP),
                "drop" => code.push(OP_DROP),
                "swap" => code.push(OP_SWAP),
                "over" => code.push(OP_OVER),
                "rot" => code.push(OP_ROT),
                "=" => code.push(OP_EQ),
                "<" => code.push(OP_LT),
                ">" => code.push(OP_GT),
                "." => code.push(OP_DOT),
                ".s" => code.push(OP_DOTS),
                "emit" => code.push(OP_EMIT),
                "clear" => code.push(OP_CLEAR),
                "true" => emit_push_i64(&mut code, 1),
                "false" => emit_push_i64(&mut code, 0),
                "if" => {
                    let jz_pos = emit_rel32_placeholder(&mut code, OP_JZ_REL32);
                    frames.push(Frame::If { jz_pos });
                }
                "begin" => {
                    let start_ip = code.len();
                    frames.push(Frame::Begin { start_ip });
                }
                "else" => {
                    let top = frames.pop().ok_or(CompileError::UnmatchedControl)?;
                    let Frame::If { jz_pos } = top else {
                        return Err(CompileError::UnmatchedControl);
                    };
                    let jmp_pos = emit_rel32_placeholder(&mut code, OP_JMP_REL32);
                    let else_body_start = code.len();
                    patch_rel32(&mut code, jz_pos, else_body_start);
                    frames.push(Frame::Else { jmp_pos });
                }
                "until" => {
                    let top = frames.pop().ok_or(CompileError::UnmatchedControl)?;
                    let Frame::Begin { start_ip } = top else {
                        return Err(CompileError::UnmatchedControl);
                    };
                    let jz_pos = emit_rel32_placeholder(&mut code, OP_JZ_REL32);
                    patch_rel32(&mut code, jz_pos, start_ip);
                }
                "end" => {
                    let top = frames.pop().ok_or(CompileError::UnmatchedControl)?;
                    let target = code.len();
                    match top {
                        Frame::If { jz_pos } => patch_rel32(&mut code, jz_pos, target),
                        Frame::Else { jmp_pos } => patch_rel32(&mut code, jmp_pos, target),
                        Frame::Begin { .. } => return Err(CompileError::UnmatchedControl),
                    }
                }
                _ => return Err(CompileError::UnknownWord),
            }
        }

        if code.len() > (MAX_PROGRAM_BYTES.saturating_sub(12)) {
            return Err(CompileError::ProgramTooLarge);
        }
    }

    if !frames.is_empty() {
        return Err(CompileError::UnmatchedControl);
    }

    code.push(OP_HALT);
    tpbc_wrap(&code)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EvalError {
    StackUnderflow,
    StackOverflow,
    DivByZero,
    UnknownWord,
    TokenOverflow,
    UnmatchedControl,
    InvalidBytecode,
    BytecodeEof,
    BadJump,
}

struct Vm {
    stack: Vec<i64, MAX_STACK>,
}

impl Vm {
    const fn new() -> Self {
        Self { stack: Vec::new() }
    }

    fn clear(&mut self) {
        self.stack.clear();
    }

    fn push(&mut self, v: i64) -> Result<(), EvalError> {
        self.stack.push(v).map_err(|_| EvalError::StackOverflow)
    }

    fn pop(&mut self) -> Result<i64, EvalError> {
        self.stack.pop().ok_or(EvalError::StackUnderflow)
    }

    fn peek(&self) -> Result<i64, EvalError> {
        self.stack.last().copied().ok_or(EvalError::StackUnderflow)
    }

    fn dump_stack(&self) {
        crate::shell::uart1_com1::write_str("<");
        for (idx, v) in self.stack.iter().enumerate() {
            if idx != 0 {
                crate::shell::uart1_com1::write_str(" ");
            }
            crate::shell::uart1_com1::write_fmt(format_args!("{}", v));
        }
        crate::shell::uart1_com1::write_str(">\r\n");
    }

    fn eval(&mut self, code: &str) -> Result<(), EvalError> {
        let mut tokens: Vec<&str, MAX_TOKENS> = Vec::new();
        for t in code.split_whitespace() {
            tokens.push(t).map_err(|_| EvalError::TokenOverflow)?;
        }

        let mut begin_stack: Vec<usize, MAX_TOKENS> = Vec::new();

        let mut ip: usize = 0;
        while ip < tokens.len() {
            let t = tokens[ip];

            if let Some(v) = parse_number(t) {
                self.push(v)?;
                ip += 1;
                continue;
            }

            match t {
                // Stack ops
                "dup" => {
                    let v = self.peek()?;
                    self.push(v)?;
                }
                "drop" => {
                    let _ = self.pop()?;
                }
                "swap" => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.push(b)?;
                    self.push(a)?;
                }
                "over" => {
                    if self.stack.len() < 2 {
                        return Err(EvalError::StackUnderflow);
                    }
                    let v = self.stack[self.stack.len() - 2];
                    self.push(v)?;
                }
                "rot" => {
                    if self.stack.len() < 3 {
                        return Err(EvalError::StackUnderflow);
                    }
                    let c = self.pop()?;
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.push(b)?;
                    self.push(c)?;
                    self.push(a)?;
                }

                // Arithmetic
                "+" => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.push(a.wrapping_add(b))?;
                }
                "-" => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.push(a.wrapping_sub(b))?;
                }
                "*" => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.push(a.wrapping_mul(b))?;
                }
                "/" => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    if b == 0 {
                        return Err(EvalError::DivByZero);
                    }
                    self.push(a / b)?;
                }
                "mod" => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    if b == 0 {
                        return Err(EvalError::DivByZero);
                    }
                    self.push(a % b)?;
                }

                // Comparisons (push 0/1)
                "=" => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.push((a == b) as i64)?;
                }
                "<" => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.push((a < b) as i64)?;
                }
                ">" => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.push((a > b) as i64)?;
                }

                // I/O
                "." => {
                    let v = self.pop()?;
                    crate::shell::uart1_com1::write_fmt(format_args!("{} ", v));
                }
                ".s" => {
                    self.dump_stack();
                }
                "emit" => {
                    let v = self.pop()?;
                    let ch = (v as u8) as char;
                    crate::shell::uart1_com1::write_char(ch);
                }

                // Control flow: if/else/end
                "if" => {
                    let cond = self.pop()?;
                    if cond == 0 {
                        ip = skip_if_false(&tokens, ip)?;
                        continue;
                    }
                }
                "begin" => {
                    begin_stack
                        .push(ip + 1)
                        .map_err(|_| EvalError::TokenOverflow)?;
                }
                "until" => {
                    let start = begin_stack.pop().ok_or(EvalError::UnmatchedControl)?;
                    let cond = self.pop()?;
                    if cond == 0 {
                        ip = start;
                        continue;
                    }
                }
                "else" => {
                    ip = skip_else(&tokens, ip)?;
                    continue;
                }
                "end" => {
                    // no-op marker
                }

                // VM management
                "clear" => {
                    self.clear();
                }
                "help" => {
                    write_help();
                }

                _ => return Err(EvalError::UnknownWord),
            }

            ip += 1;
        }

        Ok(())
    }

    fn exec_bytecode(&mut self, program: &[u8]) -> Result<(), EvalError> {
        let Some(code) = parse_program_bytes(program) else {
            return Err(EvalError::InvalidBytecode);
        };

        let mut ip: usize = 0;
        while ip < code.len() {
            let op = code[ip];
            ip += 1;
            match op {
                0 => break, // HALT
                1 => {
                    let v = read_i64(code, &mut ip)?;
                    self.push(v)?;
                }
                2 => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.push(a.wrapping_add(b))?;
                }
                3 => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.push(a.wrapping_sub(b))?;
                }
                4 => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.push(a.wrapping_mul(b))?;
                }
                5 => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    if b == 0 {
                        return Err(EvalError::DivByZero);
                    }
                    self.push(a / b)?;
                }
                6 => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    if b == 0 {
                        return Err(EvalError::DivByZero);
                    }
                    self.push(a % b)?;
                }
                7 => {
                    let v = self.peek()?;
                    self.push(v)?;
                }
                8 => {
                    let _ = self.pop()?;
                }
                9 => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.push(b)?;
                    self.push(a)?;
                }
                10 => {
                    if self.stack.len() < 2 {
                        return Err(EvalError::StackUnderflow);
                    }
                    let v = self.stack[self.stack.len() - 2];
                    self.push(v)?;
                }
                11 => {
                    if self.stack.len() < 3 {
                        return Err(EvalError::StackUnderflow);
                    }
                    let c = self.pop()?;
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.push(b)?;
                    self.push(c)?;
                    self.push(a)?;
                }
                12 => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.push((a == b) as i64)?;
                }
                13 => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.push((a < b) as i64)?;
                }
                14 => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.push((a > b) as i64)?;
                }
                15 => {
                    let v = self.pop()?;
                    crate::shell::uart1_com1::write_fmt(format_args!("{} ", v));
                }
                16 => {
                    self.dump_stack();
                }
                17 => {
                    let v = self.pop()?;
                    let ch = (v as u8) as char;
                    crate::shell::uart1_com1::write_char(ch);
                }
                18 => {
                    self.clear();
                }
                19 => {
                    let rel = read_i32(code, &mut ip)? as isize;
                    let cond = self.pop()?;
                    if cond == 0 {
                        ip = apply_rel_jump(ip, rel, code.len())?;
                    }
                }
                20 => {
                    let rel = read_i32(code, &mut ip)? as isize;
                    ip = apply_rel_jump(ip, rel, code.len())?;
                }
                _ => return Err(EvalError::InvalidBytecode),
            }
        }

        Ok(())
    }
}

static VM: Mutex<Vm> = Mutex::new(Vm::new());

fn sanitize_porth_name(name: &str) -> Option<heapless::String<48>> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let mut out: heapless::String<48> = heapless::String::new();
    for ch in name.chars() {
        let ok = ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.');
        if ok {
            let _ = out.push(ch);
        }
    }
    if out.is_empty() { None } else { Some(out) }
}

pub fn shell_handle_ctrl_c() -> bool {
    // Compile capture abort
    if PORTH_COMPILE.lock().take().is_some() {
        crate::shell::uart1_com1::write_str("^C\r\nporth: compile aborted\r\n");
        return true;
    }

    // REPL exit
    let mut repl = PORTH_REPL.lock();
    if *repl {
        *repl = false;
        crate::shell::uart1_com1::write_str("^C\r\nporth: repl exit\r\n");
        return true;
    }

    false
}

/// Handles Porth-related shell input.
/// Returns `true` when the line was consumed (no further shell dispatch needed).
pub fn shell_handle_line(line: &str) -> bool {
    let cmd = line.trim();
    if cmd.is_empty() {
        return true;
    }

    // Porth REPL mode: every line is treated as Porth code until exit.
    {
        let mut repl = PORTH_REPL.lock();
        if *repl {
            if cmd.eq_ignore_ascii_case("exit")
                || cmd.eq_ignore_ascii_case(".exit")
                || cmd.eq_ignore_ascii_case("quit")
            {
                *repl = false;
                crate::shell::uart1_com1::write_str("porth: repl exit\r\n");
            } else {
                crate::porth::eval_line(cmd);
            }
            return true;
        }
    }

    // Multiline porth-compile mode: capture raw lines until '.' then compile+write.
    {
        let mut sess_guard = PORTH_COMPILE.lock();
        if let Some(sess) = sess_guard.as_mut() {
            if cmd.eq_ignore_ascii_case("porth-compile-abort") || cmd.eq_ignore_ascii_case("pc-abort") {
                let _ = sess_guard.take();
                crate::shell::uart1_com1::write_str("porth: compile aborted\r\n");
                return true;
            }

            // Finish capture on '.' or 'END' (case-insensitive)
            if cmd == "." || cmd.eq_ignore_ascii_case("end") {
                let mut out_path: heapless::String<96> = heapless::String::new();
                let _ = out_path.push_str("/porth/");
                let _ = out_path.push_str(sess.name.as_str());
                if !sess.name.ends_with(".tpbc") {
                    let _ = out_path.push_str(".tpbc");
                }

                let src = core::mem::take(&mut sess.src);
                let _ = sess_guard.take();

                match crate::porth::compile_source_to_tpbc(&src) {
                    Ok(tpbc) => match crate::disc::files::write_usbms_file(out_path.as_str(), &tpbc) {
                        Ok(()) => crate::shell::uart1_com1::write_fmt(format_args!(
                            "porth: wrote {} ({} bytes)\r\n",
                            out_path.as_str(),
                            tpbc.len()
                        )),
                        Err(e) => crate::shell::uart1_com1::write_fmt(format_args!("porth: write failed: {:?}\r\n", e)),
                    },
                    Err(e) => crate::shell::uart1_com1::write_fmt(format_args!("porth: compile failed: {:?}\r\n", e)),
                }
            } else {
                if cmd.starts_with("porth")
                    || cmd.starts_with("pc")
                    || cmd.starts_with("pr")
                    || cmd.eq_ignore_ascii_case("install")
                {
                    crate::shell::uart1_com1::write_str(
                        "porth: still capturing; finish with '.' or 'END' (or abort)\r\n",
                    );
                }

                // Append line (keep newlines so error locations remain sensible later).
                if sess.src.len().saturating_add(cmd.len()).saturating_add(1) > PORTH_COMPILE_MAX_SRC_BYTES {
                    crate::shell::uart1_com1::write_str("porth: compile buffer full; aborting\r\n");
                    let _ = sess_guard.take();
                } else {
                    sess.src.push_str(cmd);
                    sess.src.push('\n');
                }
            }
            return true;
        }
    }

    if let Some(rest) = cmd.strip_prefix("porth ") {
        crate::porth::eval_line(rest);
        return true;
    }
    if cmd.eq_ignore_ascii_case("porth") {
        crate::shell::uart1_com1::write_str("usage: porth <code...>\r\n");
        crate::shell::uart1_com1::write_str("example: porth 2 3 + .\r\n");
        return true;
    }

    if let Some(rest) = cmd.strip_prefix("pr ") {
        let path = rest.trim();
        if path.is_empty() {
            crate::shell::uart1_com1::write_str("usage: pr <path>\r\n");
            return true;
        }
        match crate::disc::files::read_usbms_file(path) {
            Ok(bytes) => crate::porth::run_program_bytes(&bytes),
            Err(e) => crate::shell::uart1_com1::write_fmt(format_args!("pr: {:?}\r\n", e)),
        }
        return true;
    }
    if cmd.eq_ignore_ascii_case("pr") {
        crate::shell::uart1_com1::write_str("usage: pr <path>\r\n");
        crate::shell::uart1_com1::write_str("note: expects TPBC (compiled bytecode)\r\n");
        return true;
    }

    if let Some(rest) = cmd.strip_prefix("porth-run ") {
        let path = rest.trim();
        if path.is_empty() {
            crate::shell::uart1_com1::write_str("usage: porth-run <path>\r\n");
            return true;
        }
        match crate::disc::files::read_usbms_file(path) {
            Ok(bytes) => crate::porth::run_program_bytes(&bytes),
            Err(e) => crate::shell::uart1_com1::write_fmt(format_args!("porth-run: {:?}\r\n", e)),
        }
        return true;
    }
    if cmd.eq_ignore_ascii_case("porth-run") {
        crate::shell::uart1_com1::write_str("usage: porth-run <path>\r\n");
        crate::shell::uart1_com1::write_str("note: expects TPBC (compiled bytecode)\r\n");
        return true;
    }

    if cmd.eq_ignore_ascii_case("porth-reset") {
        crate::porth::reset();
        crate::shell::uart1_com1::write_str("porth: ok\r\n");
        return true;
    }
    if cmd.eq_ignore_ascii_case("porth-help") {
        crate::porth::eval_line("help");
        return true;
    }

    if cmd.eq_ignore_ascii_case("prepl") || cmd.eq_ignore_ascii_case("porth-repl") {
        *PORTH_REPL.lock() = true;
        crate::shell::uart1_com1::write_str("porth: repl mode (type code; 'exit' to return)\r\n");
        return true;
    }

    if let Some(rest) = cmd.strip_prefix("pc ") {
        let name = rest.trim();
        let Some(name) = sanitize_porth_name(name) else {
            crate::shell::uart1_com1::write_str("usage: pc <name>\r\n");
            crate::shell::uart1_com1::write_str("example: pc hello\r\n");
            return true;
        };

        *PORTH_COMPILE.lock() = Some(PorthCompileSession {
            name,
            src: AString::new(),
        });
        crate::shell::uart1_com1::write_str("porth: enter source, end with '.' on its own line\r\n");
        crate::shell::uart1_com1::write_str("porth: or finish with 'END'\r\n");
        crate::shell::uart1_com1::write_str("porth: abort with ^C or 'pc-abort'\r\n");
        crate::shell::uart1_com1::write_str("porth: output -> /porth/<name>.tpbc on usbms\r\n");
        return true;
    }
    if cmd.eq_ignore_ascii_case("pc") {
        crate::shell::uart1_com1::write_str("usage: pc <name>\r\n");
        crate::shell::uart1_com1::write_str("then type lines; finish with '.' or 'END'\r\n");
        return true;
    }

    if let Some(rest) = cmd.strip_prefix("porth-compile ") {
        let name = rest.trim();
        let Some(name) = sanitize_porth_name(name) else {
            crate::shell::uart1_com1::write_str("usage: porth-compile <name>\r\n");
            crate::shell::uart1_com1::write_str("example: porth-compile hello\r\n");
            return true;
        };

        *PORTH_COMPILE.lock() = Some(PorthCompileSession {
            name,
            src: AString::new(),
        });
        crate::shell::uart1_com1::write_str("porth: enter source, end with '.' on its own line\r\n");
        crate::shell::uart1_com1::write_str("porth: or finish with 'END'\r\n");
        crate::shell::uart1_com1::write_str("porth: abort with ^C or 'pc-abort'\r\n");
        crate::shell::uart1_com1::write_str("porth: output -> /porth/<name>.tpbc on usbms\r\n");
        return true;
    }
    if cmd.eq_ignore_ascii_case("porth-compile") {
        crate::shell::uart1_com1::write_str("usage: porth-compile <name>\r\n");
        crate::shell::uart1_com1::write_str("then type lines; finish with '.'\r\n");
        return true;
    }

    false
}

pub fn eval_line(code: &str) {
    let mut vm = VM.lock();
    match vm.eval(code) {
        Ok(()) => {
            vm.dump_stack();
        }
        Err(e) => {
            crate::shell::uart1_com1::write_fmt(format_args!("porth: error: {:?}\r\n", e));
        }
    }
}

pub fn reset() {
    VM.lock().clear();
}

pub fn run_program_bytes(program: &[u8]) {
    let mut vm = VM.lock();
    vm.clear();
    match vm.exec_bytecode(program) {
        Ok(()) => vm.dump_stack(),
        Err(e) => crate::shell::uart1_com1::write_fmt(format_args!("porth: error: {:?}\r\n", e)),
    }
}

fn write_help() {
    crate::shell::uart1_com1::write_str(
        "words: dup drop swap over rot  + - * / mod  = < >  . .s emit  if else end  begin until  clear help\r\n",
    );
}

fn parse_number(token: &str) -> Option<i64> {
    match token {
        "true" => return Some(1),
        "false" => return Some(0),
        _ => {}
    }

    let token = token.trim();
    if token.is_empty() {
        return None;
    }

    let (sign, body) = token.strip_prefix('-').map(|b| (-1i64, b)).unwrap_or((1, token));
    if let Some(hex) = body.strip_prefix("0x") {
        i64::from_str_radix(hex, 16).ok().map(|v| v.saturating_mul(sign))
    } else {
        body.parse::<i64>().ok().map(|v| v.saturating_mul(sign))
    }
}

fn parse_program_bytes(bytes: &[u8]) -> Option<&[u8]> {
    if bytes.len() < 12 {
        return None;
    }
    if &bytes[0..4] != BYTECODE_MAGIC {
        return None;
    }
    if bytes[4] != BYTECODE_VERSION {
        return None;
    }
    // bytes[5] flags, bytes[6..8] reserved
    let code_len = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
    let start: usize = 12;
    let end = start.checked_add(code_len)?;
    if end > bytes.len() {
        return None;
    }
    Some(&bytes[start..end])
}

fn read_i64(code: &[u8], ip: &mut usize) -> Result<i64, EvalError> {
    if *ip + 8 > code.len() {
        return Err(EvalError::BytecodeEof);
    }
    let b = [
        code[*ip],
        code[*ip + 1],
        code[*ip + 2],
        code[*ip + 3],
        code[*ip + 4],
        code[*ip + 5],
        code[*ip + 6],
        code[*ip + 7],
    ];
    *ip += 8;
    Ok(i64::from_le_bytes(b))
}

fn read_i32(code: &[u8], ip: &mut usize) -> Result<i32, EvalError> {
    if *ip + 4 > code.len() {
        return Err(EvalError::BytecodeEof);
    }
    let b = [code[*ip], code[*ip + 1], code[*ip + 2], code[*ip + 3]];
    *ip += 4;
    Ok(i32::from_le_bytes(b))
}

fn apply_rel_jump(ip_after: usize, rel: isize, code_len: usize) -> Result<usize, EvalError> {
    let next = (ip_after as isize).checked_add(rel).ok_or(EvalError::BadJump)?;
    if next < 0 {
        return Err(EvalError::BadJump);
    }
    let next = next as usize;
    if next > code_len {
        return Err(EvalError::BadJump);
    }
    Ok(next)
}

// When `if` is false, skip to `else` (at same nesting) or `end`.
fn skip_if_false(tokens: &Vec<&str, MAX_TOKENS>, if_ip: usize) -> Result<usize, EvalError> {
    let mut depth: usize = 0;
    let mut ip = if_ip + 1;
    while ip < tokens.len() {
        match tokens[ip] {
            "if" => depth += 1,
            "end" => {
                if depth == 0 {
                    return Ok(ip + 1);
                }
                depth -= 1;
            }
            "else" => {
                if depth == 0 {
                    return Ok(ip + 1);
                }
            }
            _ => {}
        }
        ip += 1;
    }
    Err(EvalError::UnmatchedControl)
}

// When we hit `else` in a taken branch, skip to its matching `end`.
fn skip_else(tokens: &Vec<&str, MAX_TOKENS>, else_ip: usize) -> Result<usize, EvalError> {
    let mut depth: usize = 0;
    let mut ip = else_ip + 1;
    while ip < tokens.len() {
        match tokens[ip] {
            "if" => depth += 1,
            "end" => {
                if depth == 0 {
                    return Ok(ip + 1);
                }
                depth -= 1;
            }
            _ => {}
        }
        ip += 1;
    }
    Err(EvalError::UnmatchedControl)
}
