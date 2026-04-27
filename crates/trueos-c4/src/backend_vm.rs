extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use crate::ast::{
    AssignKind, BinaryOp, Expr, ExprKind, Program, Stmt, StmtKind, Symbol, Type, UnaryOp,
};

const MAGIC: &[u8; 4] = b"TC4O";
const VERSION: u16 = 1;
const HEADER_LEN: u16 = 32;

const OP_CONST_I64: u8 = 0x01;
const OP_CONST_F64_BITS: u8 = 0x02;
const OP_CONST_BOOL: u8 = 0x03;
const OP_LOAD_LOCAL: u8 = 0x10;
const OP_STORE_LOCAL: u8 = 0x11;
const OP_LOAD_INDEX_I32: u8 = 0x12;
const OP_STORE_INDEX_I32: u8 = 0x13;
const OP_NEG: u8 = 0x20;
const OP_NOT: u8 = 0x21;
const OP_ADD: u8 = 0x30;
const OP_SUB: u8 = 0x31;
const OP_MUL: u8 = 0x32;
const OP_DIV: u8 = 0x33;
const OP_LT: u8 = 0x34;
const OP_LE: u8 = 0x35;
const OP_GT: u8 = 0x36;
const OP_GE: u8 = 0x37;
const OP_EQ: u8 = 0x38;
const OP_NE: u8 = 0x39;
const OP_AND: u8 = 0x3a;
const OP_OR: u8 = 0x3b;
const OP_JMP: u8 = 0x40;
const OP_JMP_FALSE: u8 = 0x41;
const OP_HALT: u8 = 0xff;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VmObject {
    pub bytes: Vec<u8>,
    pub code_len: usize,
    pub symbol_count: usize,
    pub stack_bytes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VmObjectError {
    Unsupported(&'static str),
    TooLarge(&'static str),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VmRunReport {
    pub code_len: usize,
    pub symbol_count: usize,
    pub stack_bytes: usize,
    pub steps: usize,
    pub locals: Vec<VmLocalReport>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VmLocalReport {
    pub name: String,
    pub ty: u8,
    pub offset: usize,
    pub width: usize,
    pub value: VmValue,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VmValue {
    Int(i64),
    Bool(bool),
    FloatBits(u64),
    Bytes(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VmRunError {
    BadMagic,
    UnsupportedVersion(u16),
    Truncated(&'static str),
    BadJump,
    BadLocal,
    BadIndex,
    StackUnderflow,
    StepLimit,
    UnsupportedOpcode(u8),
}

pub fn emit_vm_object(program: &Program) -> Result<VmObject, VmObjectError> {
    let mut emitter = VmEmitter {
        symbols: BTreeMap::new(),
        ordered_symbols: Vec::new(),
        code: Vec::new(),
        loop_breaks: Vec::new(),
    };
    emitter.emit_stmt(&program.block)?;
    emitter.op(OP_HALT);
    emitter.finish()
}

pub fn run_vm_object(bytes: &[u8], step_limit: usize) -> Result<VmRunReport, VmRunError> {
    let image = VmImage::parse(bytes)?;
    let mut stack = Vec::<VmValue>::new();
    let mut locals = image
        .symbols
        .iter()
        .map(|symbol| match symbol.ty {
            1 | 3 => VmValue::Int(0),
            2 => VmValue::FloatBits(0),
            4 => VmValue::Bool(false),
            _ => VmValue::Bytes(alloc::vec![0; symbol.width]),
        })
        .collect::<Vec<_>>();

    let mut pc = 0usize;
    let mut steps = 0usize;
    while pc < image.code.len() {
        if steps >= step_limit {
            return Err(VmRunError::StepLimit);
        }
        steps += 1;

        let op = read_u8(image.code, &mut pc)?;
        match op {
            OP_CONST_I64 => stack.push(VmValue::Int(read_i64(image.code, &mut pc)?)),
            OP_CONST_F64_BITS => stack.push(VmValue::FloatBits(read_u64(image.code, &mut pc)?)),
            OP_CONST_BOOL => stack.push(VmValue::Bool(read_u8(image.code, &mut pc)? != 0)),
            OP_LOAD_LOCAL => {
                let slot = read_u16(image.code, &mut pc)? as usize;
                stack.push(locals.get(slot).ok_or(VmRunError::BadLocal)?.clone());
            }
            OP_STORE_LOCAL => {
                let slot = read_u16(image.code, &mut pc)? as usize;
                let value = stack.pop().ok_or(VmRunError::StackUnderflow)?;
                let local = locals.get_mut(slot).ok_or(VmRunError::BadLocal)?;
                *local = value;
            }
            OP_LOAD_INDEX_I32 => {
                let slot = read_u16(image.code, &mut pc)? as usize;
                let index = pop_i64(&mut stack)? as usize;
                let local = locals.get(slot).ok_or(VmRunError::BadLocal)?;
                let VmValue::Bytes(bytes) = local else {
                    return Err(VmRunError::BadLocal);
                };
                let offset = index.checked_mul(4).ok_or(VmRunError::BadIndex)?;
                let value = read_i32_at(bytes, offset)? as i64;
                stack.push(VmValue::Int(value));
            }
            OP_STORE_INDEX_I32 => {
                let slot = read_u16(image.code, &mut pc)? as usize;
                let value = pop_i64(&mut stack)? as i32;
                let index = pop_i64(&mut stack)? as usize;
                let local = locals.get_mut(slot).ok_or(VmRunError::BadLocal)?;
                let VmValue::Bytes(bytes) = local else {
                    return Err(VmRunError::BadLocal);
                };
                let offset = index.checked_mul(4).ok_or(VmRunError::BadIndex)?;
                write_i32_at(bytes, offset, value)?;
            }
            OP_NEG => {
                let value = pop_i64(&mut stack)?;
                stack.push(VmValue::Int(-value));
            }
            OP_NOT => {
                let value = pop_bool(&mut stack)?;
                stack.push(VmValue::Bool(!value));
            }
            OP_ADD | OP_SUB | OP_MUL | OP_DIV | OP_LT | OP_LE | OP_GT | OP_GE | OP_EQ | OP_NE
            | OP_AND | OP_OR => run_binary(op, &mut stack)?,
            OP_JMP => {
                pc = read_jump(image.code, &mut pc)?;
            }
            OP_JMP_FALSE => {
                let target = read_jump(image.code, &mut pc)?;
                if !pop_bool(&mut stack)? {
                    pc = target;
                }
            }
            OP_HALT => break,
            _ => return Err(VmRunError::UnsupportedOpcode(op)),
        }
    }

    let locals = image
        .symbols
        .iter()
        .zip(locals)
        .map(|(symbol, value)| VmLocalReport {
            name: symbol.name.clone(),
            ty: symbol.ty,
            offset: symbol.offset,
            width: symbol.width,
            value,
        })
        .collect();

    Ok(VmRunReport {
        code_len: image.code.len(),
        symbol_count: image.symbols.len(),
        stack_bytes: image.stack_bytes,
        steps,
        locals,
    })
}

struct VmImage<'a> {
    code: &'a [u8],
    symbols: Vec<VmSymbol>,
    stack_bytes: usize,
}

struct VmSymbol {
    name: String,
    ty: u8,
    offset: usize,
    width: usize,
}

impl<'a> VmImage<'a> {
    fn parse(bytes: &'a [u8]) -> Result<Self, VmRunError> {
        if bytes.get(0..4) != Some(MAGIC) {
            return Err(VmRunError::BadMagic);
        }
        let version = le_u16_at(bytes, 4)?;
        if version != VERSION {
            return Err(VmRunError::UnsupportedVersion(version));
        }
        let header_len = le_u16_at(bytes, 6)? as usize;
        let code_len = le_u32_at(bytes, 8)? as usize;
        let symbol_count = le_u32_at(bytes, 12)? as usize;
        let stack_bytes = le_u32_at(bytes, 16)? as usize;
        if header_len < HEADER_LEN as usize || bytes.len() < header_len {
            return Err(VmRunError::Truncated("header"));
        }

        let mut off = header_len;
        let mut symbols = Vec::new();
        for _ in 0..symbol_count {
            let name_len = *bytes.get(off).ok_or(VmRunError::Truncated("symbol"))? as usize;
            let ty = *bytes.get(off + 1).ok_or(VmRunError::Truncated("symbol"))?;
            let width = le_u16_at(bytes, off + 2)? as usize;
            let offset = le_u32_at(bytes, off + 4)? as usize;
            off += 8;
            let name_bytes = bytes
                .get(off..off + name_len)
                .ok_or(VmRunError::Truncated("symbol name"))?;
            off += name_len;
            symbols.push(VmSymbol {
                name: String::from_utf8_lossy(name_bytes).into_owned(),
                ty,
                offset,
                width,
            });
        }

        let code = bytes
            .get(off..off + code_len)
            .ok_or(VmRunError::Truncated("code"))?;
        Ok(Self {
            code,
            symbols,
            stack_bytes,
        })
    }
}

struct VmEmitter {
    symbols: BTreeMap<String, Local>,
    ordered_symbols: Vec<Local>,
    code: Vec<u8>,
    loop_breaks: Vec<Vec<usize>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Local {
    name: String,
    ty: Type,
    slot: u16,
    offset: usize,
    width: usize,
}

impl VmEmitter {
    fn finish(self) -> Result<VmObject, VmObjectError> {
        let code_len = checked_u32(self.code.len(), "code")?;
        let symbol_count = checked_u32(self.ordered_symbols.len(), "symbols")?;
        let stack_bytes = self
            .ordered_symbols
            .iter()
            .map(|local| local.offset.saturating_add(local.width))
            .max()
            .unwrap_or(0);
        let stack_bytes_u32 = checked_u32(stack_bytes, "stack")?;

        let mut bytes = Vec::new();
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&VERSION.to_le_bytes());
        bytes.extend_from_slice(&HEADER_LEN.to_le_bytes());
        bytes.extend_from_slice(&code_len.to_le_bytes());
        bytes.extend_from_slice(&symbol_count.to_le_bytes());
        bytes.extend_from_slice(&stack_bytes_u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());

        for local in &self.ordered_symbols {
            let name = local.name.as_bytes();
            if name.len() > u8::MAX as usize {
                return Err(VmObjectError::TooLarge("symbol name"));
            }
            bytes.push(name.len() as u8);
            bytes.push(type_tag(&local.ty));
            bytes.extend_from_slice(&(local.width as u16).to_le_bytes());
            bytes.extend_from_slice(&checked_u32(local.offset, "symbol offset")?.to_le_bytes());
            bytes.extend_from_slice(name);
        }
        bytes.extend_from_slice(&self.code);

        Ok(VmObject {
            bytes,
            code_len: code_len as usize,
            symbol_count: symbol_count as usize,
            stack_bytes: stack_bytes_u32 as usize,
        })
    }

    fn emit_stmt(&mut self, stmt: &Stmt) -> Result<(), VmObjectError> {
        match &stmt.kind {
            StmtKind::Empty => {}
            StmtKind::Block(stmts) => {
                for stmt in stmts {
                    self.emit_stmt(stmt)?;
                }
            }
            StmtKind::Assign(assign) => self.emit_assign(assign)?,
            StmtKind::If {
                condition,
                then_branch,
            } => {
                self.emit_expr(condition)?;
                let false_patch = self.emit_jump(OP_JMP_FALSE);
                self.emit_stmt(then_branch)?;
                self.patch_jump(false_patch)?;
            }
            StmtKind::IfElse {
                condition,
                then_branch,
                else_branch,
            } => {
                self.emit_expr(condition)?;
                let false_patch = self.emit_jump(OP_JMP_FALSE);
                self.emit_stmt(then_branch)?;
                let done_patch = self.emit_jump(OP_JMP);
                self.patch_jump(false_patch)?;
                self.emit_stmt(else_branch)?;
                self.patch_jump(done_patch)?;
            }
            StmtKind::While { condition, body } => {
                let loop_start = self.code.len();
                self.loop_breaks.push(Vec::new());
                self.emit_expr(condition)?;
                let done_patch = self.emit_jump(OP_JMP_FALSE);
                self.emit_stmt(body)?;
                self.emit_jump_to(OP_JMP, loop_start)?;
                self.patch_jump(done_patch)?;
                self.patch_loop_breaks()?;
            }
            StmtKind::DoWhile { body, condition } => {
                let loop_start = self.code.len();
                self.loop_breaks.push(Vec::new());
                self.emit_stmt(body)?;
                self.emit_expr(condition)?;
                let done_patch = self.emit_jump(OP_JMP_FALSE);
                self.emit_jump_to(OP_JMP, loop_start)?;
                self.patch_jump(done_patch)?;
                self.patch_loop_breaks()?;
            }
            StmtKind::For {
                init,
                condition,
                step,
                body,
            } => {
                self.emit_assign(init)?;
                let loop_start = self.code.len();
                self.loop_breaks.push(Vec::new());
                self.emit_expr(condition)?;
                let done_patch = self.emit_jump(OP_JMP_FALSE);
                self.emit_stmt(body)?;
                self.emit_assign(step)?;
                self.emit_jump_to(OP_JMP, loop_start)?;
                self.patch_jump(done_patch)?;
                self.patch_loop_breaks()?;
            }
            StmtKind::Break => {
                if self.loop_breaks.is_empty() {
                    return Err(VmObjectError::Unsupported("break outside loop"));
                }
                let patch = self.emit_jump(OP_JMP);
                self.loop_breaks.last_mut().unwrap().push(patch);
            }
        }
        Ok(())
    }

    fn emit_assign(&mut self, assign: &AssignKind) -> Result<(), VmObjectError> {
        match assign {
            AssignKind::Var { target, value } => {
                let slot = self.ensure_local(target)?;
                self.emit_expr(value)?;
                self.op_u16(OP_STORE_LOCAL, slot);
            }
            AssignKind::Index { target, value } => {
                let (slot, index) = indexed_local(target)?;
                let slot = self.ensure_local(&slot)?;
                self.emit_expr(index)?;
                self.emit_expr(value)?;
                self.op_u16(OP_STORE_INDEX_I32, slot);
            }
        }
        Ok(())
    }

    fn emit_expr(&mut self, expr: &Expr) -> Result<(), VmObjectError> {
        match &expr.kind {
            ExprKind::Id(symbol) => {
                let slot = self.ensure_local(symbol)?;
                self.op_u16(OP_LOAD_LOCAL, slot);
            }
            ExprKind::Int(value) => {
                self.op(OP_CONST_I64);
                self.code.extend_from_slice(&value.to_le_bytes());
            }
            ExprKind::Float(value) => {
                let parsed = value
                    .parse::<f64>()
                    .map_err(|_| VmObjectError::Unsupported("float literal"))?;
                self.op(OP_CONST_F64_BITS);
                self.code.extend_from_slice(&parsed.to_bits().to_le_bytes());
            }
            ExprKind::Bool(value) => self.op_u8(OP_CONST_BOOL, if *value { 1 } else { 0 }),
            ExprKind::Unary { op, expr } => {
                self.emit_expr(expr)?;
                self.op(match op {
                    UnaryOp::Neg => OP_NEG,
                    UnaryOp::Not => OP_NOT,
                });
            }
            ExprKind::Binary { op, lhs, rhs } => {
                self.emit_expr(lhs)?;
                self.emit_expr(rhs)?;
                self.op(binary_opcode(*op));
            }
            ExprKind::Index { base, index } => {
                let symbol = root_local(base)?;
                let slot = self.ensure_local(symbol)?;
                self.emit_expr(index)?;
                self.op_u16(OP_LOAD_INDEX_I32, slot);
            }
        }
        Ok(())
    }

    fn ensure_local(&mut self, symbol: &Symbol) -> Result<u16, VmObjectError> {
        if let Some(local) = self.symbols.get(&symbol.name) {
            return Ok(local.slot);
        }
        let slot = checked_u16(self.ordered_symbols.len(), "locals")?;
        let offset = self
            .ordered_symbols
            .iter()
            .map(|local| local.offset.saturating_add(local.width))
            .max()
            .unwrap_or(0);
        let local = Local {
            name: symbol.name.clone(),
            ty: symbol.ty.clone(),
            slot,
            offset,
            width: symbol.ty.width(),
        };
        self.symbols.insert(symbol.name.clone(), local.clone());
        self.ordered_symbols.push(local);
        Ok(slot)
    }

    fn patch_loop_breaks(&mut self) -> Result<(), VmObjectError> {
        let breaks = self.loop_breaks.pop().unwrap_or_default();
        for patch in breaks {
            self.patch_jump(patch)?;
        }
        Ok(())
    }

    fn emit_jump(&mut self, op: u8) -> usize {
        self.op(op);
        let patch = self.code.len();
        self.code.extend_from_slice(&0u32.to_le_bytes());
        patch
    }

    fn emit_jump_to(&mut self, op: u8, target: usize) -> Result<(), VmObjectError> {
        self.op(op);
        self.code
            .extend_from_slice(&checked_u32(target, "jump target")?.to_le_bytes());
        Ok(())
    }

    fn patch_jump(&mut self, patch: usize) -> Result<(), VmObjectError> {
        let target = checked_u32(self.code.len(), "jump target")?.to_le_bytes();
        let Some(slot) = self.code.get_mut(patch..patch + 4) else {
            return Err(VmObjectError::TooLarge("jump patch"));
        };
        slot.copy_from_slice(&target);
        Ok(())
    }

    fn op(&mut self, op: u8) {
        self.code.push(op);
    }

    fn op_u8(&mut self, op: u8, value: u8) {
        self.code.push(op);
        self.code.push(value);
    }

    fn op_u16(&mut self, op: u8, value: u16) {
        self.code.push(op);
        self.code.extend_from_slice(&value.to_le_bytes());
    }
}

fn indexed_local(expr: &Expr) -> Result<(Symbol, &Expr), VmObjectError> {
    match &expr.kind {
        ExprKind::Index { base, index } => match &base.kind {
            ExprKind::Id(symbol) => Ok((symbol.clone(), index.as_ref())),
            ExprKind::Index { .. } => Err(VmObjectError::Unsupported("nested array indexing")),
            _ => Err(VmObjectError::Unsupported("indexed base")),
        },
        _ => Err(VmObjectError::Unsupported("indexed assignment target")),
    }
}

fn root_local(expr: &Expr) -> Result<&Symbol, VmObjectError> {
    match &expr.kind {
        ExprKind::Id(symbol) => Ok(symbol),
        ExprKind::Index { .. } => Err(VmObjectError::Unsupported("nested array indexing")),
        _ => Err(VmObjectError::Unsupported("indexed base")),
    }
}

fn binary_opcode(op: BinaryOp) -> u8 {
    match op {
        BinaryOp::Add => OP_ADD,
        BinaryOp::Sub => OP_SUB,
        BinaryOp::Mul => OP_MUL,
        BinaryOp::Div => OP_DIV,
        BinaryOp::Less => OP_LT,
        BinaryOp::LessEq => OP_LE,
        BinaryOp::Greater => OP_GT,
        BinaryOp::GreaterEq => OP_GE,
        BinaryOp::Eq => OP_EQ,
        BinaryOp::NotEq => OP_NE,
        BinaryOp::And => OP_AND,
        BinaryOp::Or => OP_OR,
    }
}

fn type_tag(ty: &Type) -> u8 {
    match ty {
        Type::Int => 1,
        Type::Float => 2,
        Type::Char => 3,
        Type::Bool => 4,
        Type::Array { .. } => 0x80,
    }
}

fn run_binary(op: u8, stack: &mut Vec<VmValue>) -> Result<(), VmRunError> {
    match op {
        OP_AND | OP_OR => {
            let rhs = pop_bool(stack)?;
            let lhs = pop_bool(stack)?;
            stack.push(VmValue::Bool(if op == OP_AND { lhs && rhs } else { lhs || rhs }));
        }
        OP_LT | OP_LE | OP_GT | OP_GE | OP_EQ | OP_NE => {
            let rhs = pop_i64(stack)?;
            let lhs = pop_i64(stack)?;
            let value = match op {
                OP_LT => lhs < rhs,
                OP_LE => lhs <= rhs,
                OP_GT => lhs > rhs,
                OP_GE => lhs >= rhs,
                OP_EQ => lhs == rhs,
                OP_NE => lhs != rhs,
                _ => unreachable!(),
            };
            stack.push(VmValue::Bool(value));
        }
        _ => {
            let rhs = pop_i64(stack)?;
            let lhs = pop_i64(stack)?;
            let value = match op {
                OP_ADD => lhs.wrapping_add(rhs),
                OP_SUB => lhs.wrapping_sub(rhs),
                OP_MUL => lhs.wrapping_mul(rhs),
                OP_DIV if rhs != 0 => lhs / rhs,
                OP_DIV => 0,
                _ => return Err(VmRunError::UnsupportedOpcode(op)),
            };
            stack.push(VmValue::Int(value));
        }
    }
    Ok(())
}

fn pop_i64(stack: &mut Vec<VmValue>) -> Result<i64, VmRunError> {
    match stack.pop().ok_or(VmRunError::StackUnderflow)? {
        VmValue::Int(value) => Ok(value),
        VmValue::Bool(value) => Ok(if value { 1 } else { 0 }),
        VmValue::FloatBits(value) => Ok(value as i64),
        VmValue::Bytes(_) => Err(VmRunError::BadLocal),
    }
}

fn pop_bool(stack: &mut Vec<VmValue>) -> Result<bool, VmRunError> {
    match stack.pop().ok_or(VmRunError::StackUnderflow)? {
        VmValue::Bool(value) => Ok(value),
        VmValue::Int(value) => Ok(value != 0),
        VmValue::FloatBits(value) => Ok(value != 0),
        VmValue::Bytes(_) => Err(VmRunError::BadLocal),
    }
}

fn read_u8(code: &[u8], pc: &mut usize) -> Result<u8, VmRunError> {
    let value = *code.get(*pc).ok_or(VmRunError::Truncated("opcode"))?;
    *pc += 1;
    Ok(value)
}

fn read_u16(code: &[u8], pc: &mut usize) -> Result<u16, VmRunError> {
    let value = le_u16_at(code, *pc)?;
    *pc += 2;
    Ok(value)
}

fn read_u64(code: &[u8], pc: &mut usize) -> Result<u64, VmRunError> {
    let bytes = code.get(*pc..*pc + 8).ok_or(VmRunError::Truncated("u64"))?;
    *pc += 8;
    Ok(u64::from_le_bytes(bytes.try_into().unwrap()))
}

fn read_i64(code: &[u8], pc: &mut usize) -> Result<i64, VmRunError> {
    Ok(read_u64(code, pc)? as i64)
}

fn read_jump(code: &[u8], pc: &mut usize) -> Result<usize, VmRunError> {
    let target = le_u32_at(code, *pc)? as usize;
    *pc += 4;
    if target > code.len() {
        return Err(VmRunError::BadJump);
    }
    Ok(target)
}

fn read_i32_at(bytes: &[u8], off: usize) -> Result<i32, VmRunError> {
    let raw = bytes.get(off..off + 4).ok_or(VmRunError::BadIndex)?;
    Ok(i32::from_le_bytes(raw.try_into().unwrap()))
}

fn write_i32_at(bytes: &mut [u8], off: usize, value: i32) -> Result<(), VmRunError> {
    let raw = bytes.get_mut(off..off + 4).ok_or(VmRunError::BadIndex)?;
    raw.copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn le_u16_at(bytes: &[u8], off: usize) -> Result<u16, VmRunError> {
    let raw = bytes
        .get(off..off + 2)
        .ok_or(VmRunError::Truncated("u16"))?;
    Ok(u16::from_le_bytes(raw.try_into().unwrap()))
}

fn le_u32_at(bytes: &[u8], off: usize) -> Result<u32, VmRunError> {
    let raw = bytes
        .get(off..off + 4)
        .ok_or(VmRunError::Truncated("u32"))?;
    Ok(u32::from_le_bytes(raw.try_into().unwrap()))
}

fn checked_u16(value: usize, what: &'static str) -> Result<u16, VmObjectError> {
    u16::try_from(value).map_err(|_| VmObjectError::TooLarge(what))
}

fn checked_u32(value: usize, what: &'static str) -> Result<u32, VmObjectError> {
    u32::try_from(value).map_err(|_| VmObjectError::TooLarge(what))
}

#[cfg(test)]
mod tests {
    use super::{HEADER_LEN, MAGIC, VmValue, emit_vm_object, run_vm_object};
    use crate::Parser;

    #[test]
    fn emits_vm_object_header_symbols_and_code() {
        let src = r#"
        {
            int i, sum;
            int[4] values;
            i = 0;
            sum = 0;
            while (i < 4) {
                values[i] = i + 1;
                sum = sum + values[i];
                i = i + 1;
            }
        }
        "#;

        let program = Parser::new(src).unwrap().parse_program().unwrap();
        let object = emit_vm_object(&program).unwrap();

        assert!(object.bytes.starts_with(MAGIC));
        assert_eq!(u16::from_le_bytes([object.bytes[6], object.bytes[7]]), HEADER_LEN);
        assert_eq!(object.symbol_count, 3);
        assert!(object.stack_bytes >= 24);
        assert!(object.code_len > 16);
        assert_eq!(*object.bytes.last().unwrap(), 0xff);
    }

    #[test]
    fn runs_vm_object_and_reports_final_locals() {
        let src = r#"
        {
            int i, sum;
            bool ok;
            int[4] values;
            i = 0;
            sum = 0;
            ok = true;
            while (i < 4) {
                values[i] = i + 1;
                sum = sum + values[i];
                i = i + 1;
            }
            if (ok && (sum == 10)) {
                sum = sum + 32;
            } else {
                sum = 0;
            }
        }
        "#;

        let program = Parser::new(src).unwrap().parse_program().unwrap();
        let object = emit_vm_object(&program).unwrap();
        let report = run_vm_object(object.bytes.as_slice(), 1024).unwrap();
        let sum = report
            .locals
            .iter()
            .find(|local| local.name == "sum")
            .unwrap();

        assert_eq!(sum.value, VmValue::Int(42));
        assert_eq!(report.symbol_count, 4);
    }
}
