//! Orchestrator process config: TOML plus environment (env wins).

use std::collections::HashMap;
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use tinker_protocol::{DEFAULT_TTL_SECONDS, MAX_TTL_SECONDS, PENDING_TTL_SECONDS};

use super::password::is_argon2id_hash;

/// Loaded orchestrator config for slice 5 (listeners and admission).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    /// Public bind.
    pub public_listen: SocketAddr,
    /// Admin bind.
    pub admin_listen: SocketAddr,
    /// argon2id PHC string.
    pub admin_password_hash: String,
    /// HS256 secret bytes (UTF-8 from config).
    pub jwt_hs256_secret: Vec<u8>,
    /// Default session TTL.
    pub default_ttl_seconds: u32,
    /// Max session TTL.
    pub max_ttl_seconds: u32,
    /// Pending apply TTL.
    pub pending_ttl_seconds: u32,
    /// Revoke denylist path.
    pub revoke_deny_file: PathBuf,
}

/// Why [`load`] failed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConfigError {
    /// IO while reading a file.
    Io(String),
    /// TOML parse or type error.
    Toml(String),
    /// A plaintext password field was present.
    PlaintextPassword,
    /// Admin hash missing or not argon2id v19.
    AdminHash,
    /// JWT secret missing or empty.
    JwtSecret,
    /// Listen address failed to parse.
    Listen(String),
    /// A TTL field was zero.
    Ttl,
}

impl core::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Io(s) | Self::Toml(s) | Self::Listen(s) => f.write_str(s),
            Self::PlaintextPassword => f.write_str("plaintext admin password fields are rejected"),
            Self::AdminHash => f.write_str("admin password hash is missing"),
            Self::JwtSecret => f.write_str("JWT HS256 secret is missing"),
            Self::Ttl => f.write_str("TTL values must be at least 1"),
        }
    }
}

/// Load from an optional TOML file plus `env` (environment wins).
///
/// # Errors
///
/// Returns [`ConfigError`] when a file cannot be read, the TOML is invalid, a
/// plaintext password field is set, or required secrets are missing.
pub fn load(path: Option<&Path>, env: &HashMap<String, String>) -> Result<Config, ConfigError> {
    let mut table = toml::Table::new();
    if let Some(path) = path {
        let text = fs::read_to_string(path).map_err(|e| ConfigError::Io(e.to_string()))?;
        table = text
            .parse::<toml::Table>()
            .map_err(|e| ConfigError::Toml(e.to_string()))?;
    }
    if table.contains_key("admin_password")
        || table.contains_key("password")
        || nested_has(&table, "admin", "password")
    {
        return Err(ConfigError::PlaintextPassword);
    }

    let public_listen = socket(
        env_or(
            env,
            "TINKER_PUBLIC_LISTEN",
            str_val(&table, "public_listen"),
        )
        .as_deref()
        .unwrap_or("0.0.0.0:8080"),
    )?;
    let admin_listen = socket(
        env_or(env, "TINKER_ADMIN_LISTEN", str_val(&table, "admin_listen"))
            .as_deref()
            .unwrap_or("127.0.0.1:8081"),
    )?;

    let hash = env_or(
        env,
        "TINKER_ADMIN_PASSWORD_HASH",
        str_val(&table, "admin_password_hash")
            .or_else(|| nested_str(&table, "admin", "password_hash")),
    );
    let hash = match hash {
        Some(h) if !h.is_empty() => h,
        _ => {
            if let Some(p) = env_or(
                env,
                "TINKER_ADMIN_PASSWORD_HASH_FILE",
                str_val(&table, "admin_password_hash_file")
                    .or_else(|| nested_str(&table, "admin", "password_hash_file")),
            ) {
                fs::read_to_string(&p)
                    .map_err(|e| ConfigError::Io(e.to_string()))?
                    .trim()
                    .to_owned()
            } else {
                return Err(ConfigError::AdminHash);
            }
        }
    };
    if !is_argon2id_hash(&hash) {
        return Err(ConfigError::AdminHash);
    }

    let secret = env_or(
        env,
        "TINKER_JWT_HS256_SECRET",
        str_val(&table, "jwt_hs256_secret").or_else(|| nested_str(&table, "jwt", "hs256_secret")),
    );
    let secret = match secret {
        Some(s) if !s.is_empty() => s,
        _ => {
            if let Some(p) = env_or(
                env,
                "TINKER_JWT_HS256_SECRET_FILE",
                str_val(&table, "jwt_hs256_secret_file")
                    .or_else(|| nested_str(&table, "jwt", "hs256_secret_file")),
            ) {
                fs::read_to_string(&p)
                    .map_err(|e| ConfigError::Io(e.to_string()))?
                    .trim_end_matches(['\n', '\r'])
                    .to_owned()
            } else {
                return Err(ConfigError::JwtSecret);
            }
        }
    };
    if secret.is_empty() {
        return Err(ConfigError::JwtSecret);
    }

    let default_ttl_seconds = u32_val(&table, "default_ttl_seconds", DEFAULT_TTL_SECONDS)?;
    let max_ttl_seconds = u32_val(&table, "max_ttl_seconds", MAX_TTL_SECONDS)?;
    let pending_ttl_seconds = u32_val(&table, "pending_ttl_seconds", PENDING_TTL_SECONDS)?;
    if default_ttl_seconds == 0 || max_ttl_seconds == 0 || pending_ttl_seconds == 0 {
        return Err(ConfigError::Ttl);
    }

    let revoke_deny_file = env_or(
        env,
        "TINKER_REVOKE_DENY_FILE",
        str_val(&table, "revoke_deny_file"),
    )
    .map(PathBuf::from)
    .unwrap_or_else(|| PathBuf::from("revoke.deny"));

    Ok(Config {
        public_listen,
        admin_listen,
        admin_password_hash: hash,
        jwt_hs256_secret: secret.into_bytes(),
        default_ttl_seconds,
        max_ttl_seconds,
        pending_ttl_seconds,
        revoke_deny_file,
    })
}

fn socket(raw: &str) -> Result<SocketAddr, ConfigError> {
    raw.parse()
        .map_err(|_| ConfigError::Listen(format!("invalid listen address {raw}")))
}

fn env_or(env: &HashMap<String, String>, key: &str, fallback: Option<String>) -> Option<String> {
    env.get(key).cloned().filter(|s| !s.is_empty()).or(fallback)
}

fn str_val(table: &toml::Table, key: &str) -> Option<String> {
    table.get(key).and_then(|v| v.as_str()).map(str::to_owned)
}

fn nested_str(table: &toml::Table, a: &str, b: &str) -> Option<String> {
    table
        .get(a)
        .and_then(|v| v.as_table())
        .and_then(|t| t.get(b))
        .and_then(|v| v.as_str())
        .map(str::to_owned)
}

fn nested_has(table: &toml::Table, a: &str, b: &str) -> bool {
    table
        .get(a)
        .and_then(|v| v.as_table())
        .is_some_and(|t| t.contains_key(b))
}

fn u32_val(table: &toml::Table, key: &str, default: u32) -> Result<u32, ConfigError> {
    match table.get(key) {
        None => Ok(default),
        Some(v) => v
            .as_integer()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| ConfigError::Toml(format!("invalid integer {key}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestrate::password::hash_password;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn scratch() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("tinker-cfg-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&dir).expect("dir");
        dir
    }

    #[test]
    fn load_env_and_files() {
        let dir = scratch();
        let hash = hash_password("cfg-secret").expect("h");
        let hash_file = dir.join("hash");
        fs::write(&hash_file, format!("{hash}\n")).expect("hash file");
        let secret_file = dir.join("jwt");
        fs::write(&secret_file, "jwt-bytes\n").expect("jwt file");
        let toml_path = dir.join("tinker.toml");
        fs::write(
            &toml_path,
            format!(
                "public_listen = \"127.0.0.1:0\"\nadmin_listen = \"127.0.0.1:0\"\nadmin_password_hash_file = \"{}\"\njwt_hs256_secret_file = \"{}\"\nrevoke_deny_file = \"{}\"\n",
                hash_file.display(),
                secret_file.display(),
                dir.join("deny").display()
            ),
        )
        .expect("toml");
        let cfg = load(Some(&toml_path), &HashMap::new()).expect("load");
        assert_eq!(cfg.jwt_hs256_secret, b"jwt-bytes");
        assert_eq!(cfg.default_ttl_seconds, DEFAULT_TTL_SECONDS);

        let mut env = HashMap::new();
        env.insert("TINKER_ADMIN_PASSWORD_HASH".into(), hash.clone());
        env.insert("TINKER_JWT_HS256_SECRET".into(), "from-env".into());
        env.insert("TINKER_PUBLIC_LISTEN".into(), "127.0.0.1:9".into());
        env.insert("TINKER_ADMIN_LISTEN".into(), "127.0.0.1:10".into());
        env.insert(
            "TINKER_REVOKE_DENY_FILE".into(),
            dir.join("d2").display().to_string(),
        );
        let cfg = load(Some(&toml_path), &env).expect("env wins");
        assert_eq!(cfg.jwt_hs256_secret, b"from-env");
        assert_eq!(cfg.public_listen, "127.0.0.1:9".parse().expect("p"));
    }

    #[test]
    fn rejects_plaintext_and_missing() {
        let dir = scratch();
        let p = dir.join("bad.toml");
        fs::write(&p, "admin_password = \"x\"\n").expect("w");
        assert_eq!(
            load(Some(&p), &HashMap::new()),
            Err(ConfigError::PlaintextPassword)
        );
        fs::write(&p, "[admin]\npassword = \"x\"\n").expect("w");
        assert_eq!(
            load(Some(&p), &HashMap::new()),
            Err(ConfigError::PlaintextPassword)
        );
        fs::write(&p, "password = \"x\"\n").expect("w");
        assert_eq!(
            load(Some(&p), &HashMap::new()),
            Err(ConfigError::PlaintextPassword)
        );
        assert_eq!(load(None, &HashMap::new()), Err(ConfigError::AdminHash));
        let mut env = HashMap::new();
        env.insert("TINKER_ADMIN_PASSWORD_HASH".into(), "nope".into());
        assert_eq!(load(None, &env), Err(ConfigError::AdminHash));
        env.insert(
            "TINKER_ADMIN_PASSWORD_HASH".into(),
            hash_password("z").expect("h"),
        );
        assert_eq!(load(None, &env), Err(ConfigError::JwtSecret));
        env.insert("TINKER_JWT_HS256_SECRET".into(), String::new());
        assert_eq!(load(None, &env), Err(ConfigError::JwtSecret));
        assert!(
            ConfigError::PlaintextPassword
                .to_string()
                .contains("plaintext")
        );
        assert!(ConfigError::JwtSecret.to_string().contains("JWT"));
        assert!(ConfigError::Ttl.to_string().contains("TTL"));
        assert!(ConfigError::Io("x".into()).to_string().contains('x'));
        assert!(
            ConfigError::Listen("bad".into())
                .to_string()
                .contains("bad")
        );
        let missing = dir.join("missing.toml");
        assert!(matches!(
            load(Some(&missing), &HashMap::new()),
            Err(ConfigError::Io(_))
        ));
        let hash = hash_password("z").expect("h");
        fs::write(
            &p,
            format!(
                "default_ttl_seconds = true\nadmin_password_hash = \"{hash}\"\njwt_hs256_secret = \"s\"\n"
            ),
        )
        .expect("w");
        let err = load(Some(&p), &HashMap::new()).expect_err("toml");
        assert!(matches!(err, ConfigError::Toml(_)));
        assert!(!err.to_string().is_empty());
        fs::write(&p, "[").expect("w");
        assert!(matches!(
            load(Some(&p), &HashMap::new()),
            Err(ConfigError::Toml(_))
        ));
    }

    #[test]
    fn nested_keys_and_ttl() {
        let dir = scratch();
        let hash = hash_password("n").expect("h");
        let p = dir.join("n.toml");
        fs::write(
            &p,
            format!(
                "public_listen = \"not-an-addr\"\n[admin]\npassword_hash = \"{hash}\"\n[jwt]\nhs256_secret = \"s\"\n"
            ),
        )
        .expect("w");
        assert!(matches!(
            load(Some(&p), &HashMap::new()),
            Err(ConfigError::Listen(_))
        ));
        fs::write(
            &p,
            format!(
                "[admin]\npassword_hash = \"{hash}\"\n[jwt]\nhs256_secret = \"s\"\ndefault_ttl_seconds = 0\n"
            ),
        )
        .expect("w");
        // default_ttl at root
        fs::write(
            &p,
            format!(
                "default_ttl_seconds = 0\n[admin]\npassword_hash = \"{hash}\"\n[jwt]\nhs256_secret = \"s\"\n"
            ),
        )
        .expect("w");
        assert_eq!(load(Some(&p), &HashMap::new()), Err(ConfigError::Ttl));
        fs::write(
            &p,
            format!(
                "max_ttl_seconds = 0\n[admin]\npassword_hash = \"{hash}\"\n[jwt]\nhs256_secret = \"s\"\n"
            ),
        )
        .expect("w");
        assert_eq!(load(Some(&p), &HashMap::new()), Err(ConfigError::Ttl));
        fs::write(
            &p,
            format!(
                "pending_ttl_seconds = 0\n[admin]\npassword_hash = \"{hash}\"\n[jwt]\nhs256_secret = \"s\"\n"
            ),
        )
        .expect("w");
        assert_eq!(load(Some(&p), &HashMap::new()), Err(ConfigError::Ttl));
        fs::write(
            &p,
            format!(
                "default_ttl_seconds = true\n[admin]\npassword_hash = \"{hash}\"\n[jwt]\nhs256_secret = \"s\"\n"
            ),
        )
        .expect("w");
        assert!(matches!(
            load(Some(&p), &HashMap::new()),
            Err(ConfigError::Toml(_))
        ));
        let hf = dir.join("h");
        fs::write(&hf, &hash).expect("hf");
        let sf = dir.join("s");
        fs::write(&sf, "sec").expect("sf");
        let mut env = HashMap::new();
        env.insert(
            "TINKER_ADMIN_PASSWORD_HASH_FILE".into(),
            hf.display().to_string(),
        );
        env.insert(
            "TINKER_JWT_HS256_SECRET_FILE".into(),
            sf.display().to_string(),
        );
        load(None, &env).expect("files from env");
        env.insert(
            "TINKER_ADMIN_PASSWORD_HASH_FILE".into(),
            dir.join("missing-hash").display().to_string(),
        );
        assert!(matches!(load(None, &env), Err(ConfigError::Io(_))));
        env.insert(
            "TINKER_ADMIN_PASSWORD_HASH_FILE".into(),
            hf.display().to_string(),
        );
        env.insert(
            "TINKER_JWT_HS256_SECRET_FILE".into(),
            dir.join("missing-jwt").display().to_string(),
        );
        env.remove("TINKER_JWT_HS256_SECRET");
        assert!(matches!(load(None, &env), Err(ConfigError::Io(_))));
        let empty_jwt = dir.join("empty-jwt");
        fs::write(&empty_jwt, "\n").expect("empty");
        env.insert(
            "TINKER_JWT_HS256_SECRET_FILE".into(),
            empty_jwt.display().to_string(),
        );
        assert_eq!(load(None, &env), Err(ConfigError::JwtSecret));
        env.remove("TINKER_JWT_HS256_SECRET_FILE");
        env.remove("TINKER_ADMIN_PASSWORD_HASH_FILE");
        env.insert("TINKER_ADMIN_PASSWORD_HASH".into(), hash);
        env.insert("TINKER_JWT_HS256_SECRET".into(), "s".into());
        let cfg = load(None, &env).expect("defaults");
        assert_eq!(cfg.public_listen, "0.0.0.0:8080".parse().expect("p"));
        assert_eq!(cfg.admin_listen, "127.0.0.1:8081".parse().expect("a"));
        env.insert("TINKER_ADMIN_LISTEN".into(), "not-an-addr".into());
        assert!(matches!(load(None, &env), Err(ConfigError::Listen(_))));
    }
}
