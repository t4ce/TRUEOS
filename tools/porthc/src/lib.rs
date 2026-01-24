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
    compile_error!("Porth tooling removed from TRUEOS.");
    out.push(VERSION);
