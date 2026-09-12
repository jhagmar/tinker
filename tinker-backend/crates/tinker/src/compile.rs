//! Compile the fat catalog binary from `CatalogDir`.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use std::{fmt, thread};

/// How to turn `CatalogDir` into a binary. Tests inject a stub.
pub trait CatalogCompiler {
    /// Build the catalog package. `term_color` is `always`, `never`, or `auto`.
    ///
    /// # Errors
    ///
    /// Returns [`CompileError`] when the manifest is missing, cargo fails, or the wait times out.
    fn compile(&self, catalog_dir: &Path, term_color: &str) -> Result<(), CompileError>;
}

/// `cargo build` of `catalog/Cargo.toml`.
pub struct CargoCompiler {
    cargo: PathBuf,
    timeout: Duration,
}

impl CargoCompiler {
    /// `cargo` on `PATH`, 120s timeout.
    #[must_use]
    pub fn new() -> Self {
        Self {
            cargo: PathBuf::from("cargo"),
            timeout: Duration::from_secs(120),
        }
    }

    /// Override the cargo executable (tests).
    #[must_use]
    pub fn with_cargo(mut self, cargo: PathBuf) -> Self {
        self.cargo = cargo;
        self
    }

    /// Override the wait timeout (tests).
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

impl Default for CargoCompiler {
    fn default() -> Self {
        Self::new()
    }
}

/// Why catalog compile failed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompileError {
    /// `CatalogDir/Cargo.toml` is missing.
    MissingManifest,
    /// `CatalogDir/Cargo.lock` is missing.
    MissingLockfile,
    /// `cargo` could not be started.
    Spawn(String),
    /// The wait exceeded the timeout.
    Timeout,
    /// Cargo exited with a failure status.
    Failed {
        /// Process exit code when present.
        code: Option<i32>,
        /// Captured stderr.
        stderr: String,
    },
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingManifest => f.write_str("catalog Cargo.toml is missing"),
            Self::MissingLockfile => f.write_str("catalog Cargo.lock is missing"),
            Self::Spawn(msg) => write!(f, "could not start cargo: {msg}"),
            Self::Timeout => f.write_str("catalog compile timed out"),
            Self::Failed { code, stderr } => match code {
                Some(c) => write!(f, "catalog compile failed (exit {c}): {stderr}"),
                None => write!(f, "catalog compile failed: {stderr}"),
            },
        }
    }
}

impl CatalogCompiler for CargoCompiler {
    fn compile(&self, catalog_dir: &Path, term_color: &str) -> Result<(), CompileError> {
        let manifest = catalog_dir.join("Cargo.toml");
        if !manifest.is_file() {
            return Err(CompileError::MissingManifest);
        }
        let lock = catalog_dir.join("Cargo.lock");
        if !lock.is_file() {
            return Err(CompileError::MissingLockfile);
        }
        let target_dir = catalog_dir.join("target");
        let mut cmd = Command::new(&self.cargo);
        cmd.arg("build")
            .arg("--manifest-path")
            .arg(&manifest)
            .arg("--offline")
            .arg("--locked")
            .env("CARGO_TERM_COLOR", term_color)
            .env("CARGO_TARGET_DIR", &target_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd
            .spawn()
            .map_err(|e| CompileError::Spawn(e.to_string()))?;
        let start = Instant::now();
        loop {
            match child.try_wait().expect("wait catalog cargo") {
                Some(status) => {
                    let mut stdout = Vec::new();
                    let mut stderr = Vec::new();
                    if let Some(mut out) = child.stdout.take() {
                        let _ = out.read_to_end(&mut stdout);
                    }
                    if let Some(mut err) = child.stderr.take() {
                        let _ = err.read_to_end(&mut stderr);
                    }
                    drop(stdout);
                    if status.success() {
                        return Ok(());
                    }
                    return Err(CompileError::Failed {
                        code: status.code(),
                        stderr: String::from_utf8_lossy(&stderr).into_owned(),
                    });
                }
                None if start.elapsed() >= self.timeout => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(CompileError::Timeout);
                }
                None => thread::sleep(Duration::from_millis(20)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::{env, fs};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    fn scratch(label: &str) -> PathBuf {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = env::temp_dir().join(format!(
            "tinker-compile-{}-{}-{}",
            label,
            std::process::id(),
            n
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    fn write_package(dir: &Path, main_rs: &str) {
        fs::create_dir_all(dir.join("src")).expect("src");
        fs::write(
            dir.join("Cargo.toml"),
            "[workspace]\n\n[package]\nname = \"tiny-cat\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .expect("toml");
        fs::write(dir.join("src/main.rs"), main_rs).expect("main");
    }

    fn lockfile(dir: &Path) {
        let st = Command::new("cargo")
            .arg("generate-lockfile")
            .arg("--manifest-path")
            .arg(dir.join("Cargo.toml"))
            .status()
            .expect("cargo");
        assert!(st.success(), "generate-lockfile");
    }

    #[test]
    fn display_variants() {
        assert_eq!(
            CompileError::MissingManifest.to_string(),
            "catalog Cargo.toml is missing"
        );
        assert_eq!(
            CompileError::MissingLockfile.to_string(),
            "catalog Cargo.lock is missing"
        );
        assert_eq!(
            CompileError::Spawn("x".into()).to_string(),
            "could not start cargo: x"
        );
        assert_eq!(
            CompileError::Timeout.to_string(),
            "catalog compile timed out"
        );
        assert_eq!(
            CompileError::Failed {
                code: Some(7),
                stderr: "e".into(),
            }
            .to_string(),
            "catalog compile failed (exit 7): e"
        );
        assert_eq!(
            CompileError::Failed {
                code: None,
                stderr: "e".into(),
            }
            .to_string(),
            "catalog compile failed: e"
        );
    }

    #[test]
    fn missing_manifest_and_lockfile() {
        let dir = scratch("empty");
        let c = CargoCompiler::new();
        assert_eq!(
            c.compile(&dir, "never").unwrap_err(),
            CompileError::MissingManifest
        );
        write_package(&dir, "fn main() {}\n");
        assert_eq!(
            c.compile(&dir, "never").unwrap_err(),
            CompileError::MissingLockfile
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn spawn_failure() {
        let dir = scratch("spawn");
        write_package(&dir, "fn main() {}\n");
        lockfile(&dir);
        let c = CargoCompiler::new().with_cargo(PathBuf::from("/no/such/tinker-cargo"));
        let err = c.compile(&dir, "never").unwrap_err();
        assert!(matches!(err, CompileError::Spawn(_)), "{err}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn compile_ok_and_fail() {
        let ok = scratch("ok");
        write_package(&ok, "fn main() {}\n");
        lockfile(&ok);
        CargoCompiler::new()
            .compile(&ok, "never")
            .expect("ok compile");
        CargoCompiler::default()
            .compile(&ok, "always")
            .expect("ok compile color");
        let _ = fs::remove_dir_all(&ok);

        let bad = scratch("bad");
        write_package(&bad, "fn main() { let x: u32 = \"no\"; }\n");
        lockfile(&bad);
        let err = CargoCompiler::new().compile(&bad, "auto").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("exit 101"), "{msg}");
        assert!(msg.len() > 20, "{msg}");
        let _ = fs::remove_dir_all(&bad);
    }

    #[cfg(unix)]
    #[test]
    fn timeout_kills_child() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("timeout");
        write_package(&dir, "fn main() {}\n");
        lockfile(&dir);
        let hang = dir.join("hang-cargo");
        fs::write(&hang, "#!/bin/sh\nexec sleep 30\n").expect("script");
        let mut perm = fs::metadata(&hang).expect("meta").permissions();
        perm.set_mode(0o755);
        fs::set_permissions(&hang, perm).expect("chmod");
        let c = CargoCompiler::new()
            .with_cargo(hang)
            .with_timeout(Duration::from_millis(80));
        assert_eq!(c.compile(&dir, "never").unwrap_err(), CompileError::Timeout);
        let _ = fs::remove_dir_all(&dir);
    }
}
