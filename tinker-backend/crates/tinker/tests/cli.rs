//! CLI integration tests for `tinker verify`.

use std::path::PathBuf;
use std::process::Command;

fn catalog_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../catalog")
        .canonicalize()
        .expect("catalog dir")
}

#[test]
fn verify_all_100() {
    let exe = env!("CARGO_BIN_EXE_tinker");
    let out = Command::new(exe)
        .args(["verify", "all", "100"])
        .env("TINKER_CATALOG_DIR", catalog_dir())
        .env("NO_COLOR", "1")
        .output()
        .expect("run tinker");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stdout={stdout} stderr={stderr}"
    );
    assert!(stdout.contains("int-sum: 100 passed"), "stdout={stdout}");
}

#[test]
fn help_exits_0() {
    let exe = env!("CARGO_BIN_EXE_tinker");
    let out = Command::new(exe)
        .arg("--help")
        .output()
        .expect("run tinker");
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains("verify"));
}
