//! Process entry for the Tinker host. Logic lives in the `tinker` library.

use std::collections::HashMap;
use std::env;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use tinker::{CargoCompiler, HiddenInput};

fn main() -> ExitCode {
    let args: Vec<_> = env::args_os().collect();
    let catalog = catalog_dir();
    let no_color = env::var_os("NO_COLOR").is_some();
    let env_map: HashMap<String, String> = env::vars().collect();
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    let mut hidden = StdinHidden;
    let code = tinker::run(
        tinker::RunInput {
            args: &args,
            catalog_dir: &catalog,
            no_color,
            compiler: &CargoCompiler::new(),
            hidden: &mut hidden,
            env: &env_map,
            shutdown: tinker::Shutdown::Signal,
        },
        &mut stdout,
        &mut stderr,
    );
    let _ = stdout.flush();
    let _ = stderr.flush();
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}

/// TTY password prompt with echo off.
struct StdinHidden;

impl HiddenInput for StdinHidden {
    fn read_password(&mut self) -> Result<String, String> {
        rpassword::read_password().map_err(|e| e.to_string())
    }
}

fn catalog_dir() -> PathBuf {
    env::var_os("TINKER_CATALOG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(tinker::catalog_dir()))
}
