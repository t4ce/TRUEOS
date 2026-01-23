use std::env;
use std::fs;
use std::path::Path;

#[derive(Debug)]
enum ErrKind {
    Usage,
    Io(std::io::Error),
    Compile(porthc::Error),
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e:?}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), ErrKind> {
    let mut args = env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 2 {
        return Err(ErrKind::Usage);
    }
    let input = args.remove(0);
    let output = args.remove(0);

    let src = fs::read_to_string(&input).map_err(ErrKind::Io)?;
    let out = porthc::compile_to_tpbc(&src).map_err(ErrKind::Compile)?;

    if let Some(parent) = Path::new(&output).parent() {
        if !parent.as_os_str().is_empty() {
            let _ = fs::create_dir_all(parent);
        }
    }
    fs::write(&output, out).map_err(ErrKind::Io)?;
    Ok(())
}
