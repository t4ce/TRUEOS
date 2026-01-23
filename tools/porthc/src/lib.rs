const MAGIC: &[u8; 4] = b"TPBC";
const VERSION: u8 = 1;

#[derive(Debug)]
pub enum Error {
    UnmatchedControl,
    UnknownWord(String),
}

#[derive(Clone, Copy, Debug)]
enum Frame {
    If { jz_pos: usize },
    Else { jmp_pos: usize },
    Begin { start_ip: usize },
}

pub fn compile_to_tpbc(src: &str) -> Result<Vec<u8>, Error> {
    let code = compile_to_code(src)?;

    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.push(0); // flags
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(code.len() as u32).to_le_bytes());
    out.extend_from_slice(&code);

    Ok(out)
}

pub fn compile_to_code(src: &str) -> Result<Vec<u8>, Error> {
    let mut code: Vec<u8> = Vec::new();
    let mut frames: Vec<Frame> = Vec::new();

    for token in src.split_whitespace() {
        if let Some(v) = parse_number(token) {
            emit_push_i64(&mut code, v);
            continue;
        }

        match token {
            "+" => code.push(2),
            "-" => code.push(3),
            "*" => code.push(4),
            "/" => code.push(5),
            "mod" => code.push(6),
            "dup" => code.push(7),
            "drop" => code.push(8),
            "swap" => code.push(9),
            "over" => code.push(10),
            "rot" => code.push(11),
            "=" => code.push(12),
            "<" => code.push(13),
            ">" => code.push(14),
            "." => code.push(15),
            ".s" => code.push(16),
            "emit" => code.push(17),
            "clear" => code.push(18),
            "if" => {
                // JumpIfZero rel32 placeholder
                code.push(19);
                let jz_pos = code.len();
                code.extend_from_slice(&0i32.to_le_bytes());
                frames.push(Frame::If { jz_pos });
            }
            "begin" => {
                let start_ip = code.len();
                frames.push(Frame::Begin { start_ip });
            }
            "else" => {
                let top = frames.pop().ok_or(Error::UnmatchedControl)?;
                let Frame::If { jz_pos } = top else {
                    return Err(Error::UnmatchedControl);
                };

                // Jump rel32 placeholder to end (patched at `end`)
                code.push(20);
                let jmp_pos = code.len();
                code.extend_from_slice(&0i32.to_le_bytes());

                // Else-body starts right after the JMP immediate.
                let else_body_start = code.len();

                // Patch the earlier JZ to jump here (start of else-body)
                patch_rel32(&mut code, jz_pos, else_body_start);

                frames.push(Frame::Else { jmp_pos });
            }
            "until" => {
                let top = frames.pop().ok_or(Error::UnmatchedControl)?;
                let Frame::Begin { start_ip } = top else {
                    return Err(Error::UnmatchedControl);
                };

                // JumpIfZero rel32 back to loop start
                code.push(19);
                let jz_pos = code.len();
                code.extend_from_slice(&0i32.to_le_bytes());
                patch_rel32(&mut code, jz_pos, start_ip);
            }
            "end" => {
                let top = frames.pop().ok_or(Error::UnmatchedControl)?;
                match top {
                    Frame::If { jz_pos } => {
                        // No else: patch JZ to jump to after end
                        let target = code.len();
                        patch_rel32(&mut code, jz_pos, target);
                    }
                    Frame::Else { jmp_pos } => {
                        // Patch JMP to jump to after end
                        let target = code.len();
                        patch_rel32(&mut code, jmp_pos, target);
                    }
                    Frame::Begin { .. } => return Err(Error::UnmatchedControl),
                }
            }
            "true" => emit_push_i64(&mut code, 1),
            "false" => emit_push_i64(&mut code, 0),
            _ => return Err(Error::UnknownWord(token.to_string())),
        }
    }

    if !frames.is_empty() {
        return Err(Error::UnmatchedControl);
    }

    // Optional: terminate
    code.push(0);
    Ok(code)
}

fn emit_push_i64(code: &mut Vec<u8>, v: i64) {
    code.push(1);
    code.extend_from_slice(&v.to_le_bytes());
}

fn patch_rel32(code: &mut [u8], rel_pos: usize, target_ip: usize) {
    // rel_pos points to the start of the i32 immediate (right after opcode).
    let ip_after = rel_pos + 4;
    let rel = (target_ip as isize).wrapping_sub(ip_after as isize) as i32;
    code[rel_pos..rel_pos + 4].copy_from_slice(&rel.to_le_bytes());
}

fn parse_number(token: &str) -> Option<i64> {
    let token = token.trim();
    if token.is_empty() {
        return None;
    }

    let (sign, body) = token
        .strip_prefix('-')
        .map(|b| (-1i64, b))
        .unwrap_or((1, token));

    if let Some(hex) = body.strip_prefix("0x") {
        i64::from_str_radix(hex, 16)
            .ok()
            .map(|v| v.saturating_mul(sign))
    } else {
        body.parse::<i64>().ok().map(|v| v.saturating_mul(sign))
    }
}
