//! Orchestrator role: public and admin HTTP listeners.

mod admission;
mod config;
mod http;
mod jwt;
mod password;

pub use config::{Config, load};
pub use http::Running;
pub use password::hash_password;

use std::sync::Arc;

#[cfg(test)]
use std::io::Write;

use admission::{Admission, OsEntropy, SystemClock};

/// Format an IO or parse error for CLI and HTTP bind failures.
pub(crate) fn display_err(e: impl core::fmt::Display) -> String {
    e.to_string()
}

/// How long [`run`] waits after bind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Shutdown {
    /// Return as soon as both sockets are bound (tests).
    Immediate,
    /// Wait for SIGINT / Ctrl+C.
    Signal,
}

/// Bind listeners, write addresses, wait, then stop.
///
/// # Errors
///
/// Returns a message when the denylist cannot be opened or a bind fails.
#[cfg(test)]
pub async fn run(cfg: Config, stdout: &mut dyn Write, shutdown: Shutdown) -> Result<(), String> {
    let running = bind(cfg).await?;
    writeln!(stdout, "public {}", running.public).map_err(display_err)?;
    writeln!(stdout, "admin {}", running.admin).map_err(display_err)?;
    wait_shutdown(shutdown).await;
    running.shutdown().await;
    Ok(())
}

/// Bind public and admin sockets.
///
/// # Errors
///
/// Returns a message when the denylist cannot be opened or a bind fails.
pub async fn bind(cfg: Config) -> Result<Running, String> {
    let admission = Arc::new(Admission::new(
        cfg.clone(),
        Box::new(SystemClock),
        Box::new(OsEntropy),
    )?);
    http::serve(cfg, admission).await
}

/// Block until [`Shutdown`] fires.
pub async fn wait_shutdown(shutdown: Shutdown) {
    match shutdown {
        Shutdown::Immediate => {}
        Shutdown::Signal => {
            let _ = tokio::signal::ctrl_c().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestrate::password::hash_password;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn scratch() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("t")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("tinker-orch-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("d");
        dir
    }

    #[tokio::test]
    async fn bind_logs_and_immediate_shutdown() {
        let dir = scratch();
        let hash = hash_password("pw").expect("h");
        let cfg = Config {
            public_listen: "127.0.0.1:0".parse().expect("p"),
            admin_listen: "127.0.0.1:0".parse().expect("a"),
            admin_password_hash: hash,
            jwt_hs256_secret: b"orch-secret".to_vec(),
            default_ttl_seconds: 3600,
            max_ttl_seconds: 604_800,
            pending_ttl_seconds: 900,
            revoke_deny_file: dir.join("deny"),
        };
        let mut out = Vec::new();
        run(cfg, &mut out, Shutdown::Immediate).await.expect("run");
        let text = String::from_utf8(out).expect("utf8");
        assert!(text.contains("public 127.0.0.1:"));
        assert!(text.contains("admin 127.0.0.1:"));
        assert!(!text.contains("orch-secret"));
        assert!(!text.contains("$argon2"));
    }

    #[tokio::test]
    async fn bind_fails_when_denylist_parent_is_a_file() {
        let dir = scratch();
        let hash = hash_password("pw").expect("h");
        let parent = dir.join("not-dir");
        std::fs::write(&parent, b"x").expect("file");
        let cfg = Config {
            public_listen: "127.0.0.1:0".parse().expect("p"),
            admin_listen: "127.0.0.1:0".parse().expect("a"),
            admin_password_hash: hash,
            jwt_hs256_secret: b"orch-secret".to_vec(),
            default_ttl_seconds: 3600,
            max_ttl_seconds: 604_800,
            pending_ttl_seconds: 900,
            revoke_deny_file: parent.join("deny"),
        };
        assert!(bind(cfg).await.is_err());
    }

    #[tokio::test]
    async fn wait_immediate_returns() {
        wait_shutdown(Shutdown::Immediate).await;
    }

    #[tokio::test]
    async fn wait_signal_installs_handler() {
        let wait = tokio::spawn(wait_shutdown(Shutdown::Signal));
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        wait.abort();
        let _ = wait.await;
    }

    #[test]
    fn display_err_formats() {
        assert_eq!(display_err("io"), "io");
    }

    struct FailAfter {
        ok: usize,
        n: usize,
    }

    impl std::io::Write for FailAfter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if self.n >= self.ok {
                return Err(std::io::Error::other("fail"));
            }
            self.n += 1;
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn sample_cfg(dir: &std::path::Path) -> Config {
        Config {
            public_listen: "127.0.0.1:0".parse().expect("p"),
            admin_listen: "127.0.0.1:0".parse().expect("a"),
            admin_password_hash: hash_password("pw").expect("h"),
            jwt_hs256_secret: b"orch-secret".to_vec(),
            default_ttl_seconds: 3600,
            max_ttl_seconds: 604_800,
            pending_ttl_seconds: 900,
            revoke_deny_file: dir.join("deny"),
        }
    }

    #[tokio::test]
    async fn run_fails_when_stdout_cannot_write() {
        let mut flushed = FailAfter { ok: 10, n: 0 };
        assert!(std::io::Write::flush(&mut flushed).is_ok());
        let dir = scratch();
        let cfg = sample_cfg(&dir);
        let mut out = FailAfter { ok: 0, n: 0 };
        assert!(run(cfg, &mut out, Shutdown::Immediate).await.is_err());
        let cfg = sample_cfg(&dir);
        let mut out = FailAfter { ok: 1, n: 0 };
        assert!(run(cfg, &mut out, Shutdown::Immediate).await.is_err());
    }
}
