//! Process entry for the Tinker host. Logic lives in the `tinker` library.

use std::env;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use tinker::CargoCompiler;

fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().collect();
    let catalog = catalog_dir();
    let no_color = env::var_os("NO_COLOR").is_some();
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    let code = tinker::run(
        &args,
        &catalog,
        no_color,
        &CargoCompiler::new(),
        &mut stdout,
        &mut stderr,
    );
    let _ = stdout.flush();
    let _ = stderr.flush();
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}

fn catalog_dir() -> PathBuf {
    env::var_os("TINKER_CATALOG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(tinker::catalog_dir()))
}
