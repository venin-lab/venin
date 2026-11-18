//! Writes a synthetic lab in the served layout, for building the site before a real run:
//! `cargo run -p venin-core --example synthetic -- <out dir> <periods>`. Its manifest names the league
//! "synthetic fixture".

mod fake;

use flate2::Compression;
use flate2::write::GzEncoder;
use std::io::Write;
use std::path::{Path, PathBuf};

fn write(out: &Path, path: &str, bytes: &[u8]) -> Result<(), String> {
    let (path, bytes) = match path.strip_suffix(".jsonl") {
        Some(stem) if path.starts_with("runs/") => {
            let mut gz = GzEncoder::new(Vec::new(), Compression::best());
            gz.write_all(bytes).map_err(|e| e.to_string())?;
            (
                format!("{stem}.jsonl.gz"),
                gz.finish().map_err(|e| e.to_string())?,
            )
        }
        _ => (path.to_string(), bytes.to_vec()),
    };
    let target = out.join(path);
    if let Some(dir) = target.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(&target, bytes).map_err(|e| format!("{}: {e}", target.display()))
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [out, periods] = args.as_slice() else {
        return Err("usage: synthetic <out dir> <periods>".into());
    };
    let periods: u64 = periods
        .parse()
        .map_err(|_| format!("periods must be a number, not {periods}"))?;
    let library = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../library");
    let files = fake::generate(&fake::read_library(&library)?, periods)?;
    for (path, bytes) in &files {
        write(Path::new(out), path, bytes)?;
    }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("synthetic: {e}");
        std::process::exit(1);
    }
}
