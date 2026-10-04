//! Hand-rolled CLI: `tinker verify`, `hash-password`, `orchestrate`, help, version, `--color`.

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};

use tinker_catalog::{ProblemId, ProblemIdError, Selector, entries, verify};

use crate::codegen::write_http_js;
use crate::compile::CatalogCompiler;
use crate::orchestrate::{self, Shutdown};
use crate::sampler::{SplitMix64, VERIFY_SEED};
use crate::version;

const USAGE: &str = "\
Usage: tinker [options] <command>

Commands:
  verify <all|id[,id...]> <n>  Sample n instances per selected problem
  codegen [path]               Write generated/tinker-http.js (or path)
  hash-password                Print an argon2id hash (TTY or --password-file)
  orchestrate                  Bind public and admin listeners

Options:
  -h, --help                   Show this help
  -V, --version                Show version
      --color <when>           auto, always, or never
      --password-file <path>   Password file for hash-password
      --config <path>          TOML config for orchestrate
";

/// Reads a password with echo off (TTY). Tests inject a stub.
pub trait HiddenInput {
    /// Read one password line.
    ///
    /// # Errors
    ///
    /// Returns a message when the prompt fails.
    fn read_password(&mut self) -> Result<String, String>;
}

/// Process arguments for [`run`].
pub struct RunInput<'a> {
    /// Argv including argv0.
    pub args: &'a [OsString],
    /// Catalog directory for `verify`.
    pub catalog_dir: &'a Path,
    /// Whether `NO_COLOR` is set.
    pub no_color: bool,
    /// Catalog compiler.
    pub compiler: &'a dyn CatalogCompiler,
    /// Password prompt.
    pub hidden: &'a mut dyn HiddenInput,
    /// Environment map used by `orchestrate`.
    pub env: &'a HashMap<String, String>,
    /// Orchestrate shutdown mode.
    pub shutdown: Shutdown,
}

/// Selects cargo's `CARGO_TERM_COLOR` for catalog compile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ColorMode {
    /// Color when `NO_COLOR` is unset.
    Auto,
    /// Always set `CARGO_TERM_COLOR=always`.
    Always,
    /// Never color.
    Never,
}

impl ColorMode {
    /// Map to cargo's `CARGO_TERM_COLOR` value.
    #[must_use]
    pub fn cargo_term_color(self, no_color: bool) -> &'static str {
        match self {
            Self::Always => "always",
            Self::Never => "never",
            Self::Auto if no_color => "never",
            Self::Auto => "auto",
        }
    }

    /// Whether color is on for host text (`auto` follows `NO_COLOR`).
    #[must_use]
    pub fn enabled(self, no_color: bool) -> bool {
        self.cargo_term_color(no_color) != "never"
    }
}

enum Action {
    Help,
    Version,
    Verify {
        selector: Selector,
        n: u32,
        color: ColorMode,
    },
    Codegen {
        path: PathBuf,
    },
    HashPassword {
        file: Option<PathBuf>,
    },
    Orchestrate {
        config: Option<PathBuf>,
    },
}

enum ParseErr {
    Usage(String),
}

/// Parse args and run. Returns a process exit code (0, 1, or 2).
pub fn run(input: RunInput<'_>, stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    let RunInput {
        args,
        catalog_dir,
        no_color,
        compiler,
        hidden,
        env,
        shutdown,
    } = input;
    match parse(args) {
        Err(ParseErr::Usage(msg)) => {
            let _ = writeln!(stderr, "{msg}");
            let _ = write!(stderr, "{USAGE}");
            2
        }
        Ok(Action::Help) => {
            let _ = write!(stdout, "{USAGE}");
            0
        }
        Ok(Action::Version) => {
            let _ = writeln!(stdout, "{}", version());
            0
        }
        Ok(Action::Verify { selector, n, color }) => {
            let term_color = color.cargo_term_color(no_color);
            if let Err(e) = compiler.compile(catalog_dir, term_color) {
                let _ = writeln!(stderr, "{e}");
                return 1;
            }
            let mut sampler = SplitMix64::new(VERIFY_SEED);
            match verify(entries(), &selector, n, &mut sampler) {
                Ok(report) => {
                    for (id, count) in report.problems {
                        let _ = writeln!(stdout, "{id}: {count} passed");
                    }
                    0
                }
                Err(e) => {
                    let _ = writeln!(stderr, "{e}");
                    1
                }
            }
        }
        Ok(Action::Codegen { path }) => match write_http_js(&path) {
            Ok(()) => {
                let _ = writeln!(stdout, "wrote {}", path.display());
                0
            }
            Err(e) => {
                let _ = writeln!(stderr, "codegen failed: {e}");
                1
            }
        },
        Ok(Action::HashPassword { file }) => {
            hash_password_cmd(file.as_deref(), hidden, stdout, stderr)
        }
        Ok(Action::Orchestrate { config }) => {
            orchestrate_cmd(config.as_deref(), env, stdout, stderr, shutdown)
        }
    }
}

fn hash_password_cmd(
    file: Option<&Path>,
    hidden: &mut dyn HiddenInput,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    let password = match file {
        Some(path) => match std::fs::read_to_string(path) {
            Ok(s) => s.trim_end_matches(['\n', '\r']).to_owned(),
            Err(e) => {
                let _ = writeln!(stderr, "{e}");
                return 1;
            }
        },
        None => match hidden.read_password() {
            Ok(s) => s,
            Err(e) => {
                let _ = writeln!(stderr, "{e}");
                return 1;
            }
        },
    };
    match orchestrate::hash_password(&password) {
        Ok(h) => {
            let _ = writeln!(stdout, "{h}");
            0
        }
        Err(e) => {
            let _ = writeln!(stderr, "{e}");
            1
        }
    }
}

fn orchestrate_cmd(
    config: Option<&Path>,
    env: &HashMap<String, String>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    shutdown: Shutdown,
) -> i32 {
    let cfg = match orchestrate::load(config, env) {
        Ok(c) => c,
        Err(e) => {
            let _ = writeln!(stderr, "{e}");
            return 1;
        }
    };
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let running = match rt.block_on(orchestrate::bind(cfg)) {
        Ok(r) => r,
        Err(e) => {
            let _ = writeln!(stderr, "{e}");
            return 1;
        }
    };
    let _ = writeln!(stdout, "public {}", running.public);
    let _ = writeln!(stdout, "admin {}", running.admin);
    rt.block_on(async move {
        orchestrate::wait_shutdown(shutdown).await;
        running.shutdown().await;
    });
    0
}

fn parse(args: &[OsString]) -> Result<Action, ParseErr> {
    let mut tokens = Vec::new();
    for (i, a) in args.iter().enumerate() {
        let Some(s) = a.to_str() else {
            return Err(ParseErr::Usage("arguments must be UTF-8".to_owned()));
        };
        if i == 0 {
            continue;
        }
        tokens.push(s);
    }
    let mut color = ColorMode::Auto;
    let mut password_file = None;
    let mut config = None;
    let mut positionals = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let t = tokens[i];
        if t == "-h" || t == "--help" {
            return Ok(Action::Help);
        }
        if t == "-V" || t == "--version" {
            return Ok(Action::Version);
        }
        if t == "--color" {
            i += 1;
            let Some(v) = tokens.get(i) else {
                return Err(ParseErr::Usage("missing value for --color".to_owned()));
            };
            color = parse_color(v)?;
            i += 1;
            continue;
        }
        if let Some(v) = t.strip_prefix("--color=") {
            color = parse_color(v)?;
            i += 1;
            continue;
        }
        if t == "--password-file" {
            i += 1;
            let Some(v) = tokens.get(i) else {
                return Err(ParseErr::Usage(
                    "missing value for --password-file".to_owned(),
                ));
            };
            password_file = Some(*v);
            i += 1;
            continue;
        }
        if let Some(v) = t.strip_prefix("--password-file=") {
            password_file = Some(v);
            i += 1;
            continue;
        }
        if t == "--config" {
            i += 1;
            let Some(v) = tokens.get(i) else {
                return Err(ParseErr::Usage("missing value for --config".to_owned()));
            };
            config = Some(*v);
            i += 1;
            continue;
        }
        if let Some(v) = t.strip_prefix("--config=") {
            config = Some(v);
            i += 1;
            continue;
        }
        if t.starts_with('-') {
            return Err(ParseErr::Usage(format!("unknown option {t}")));
        }
        positionals.push(t);
        i += 1;
    }
    if positionals.is_empty() {
        return Err(ParseErr::Usage("missing command".to_owned()));
    }
    match positionals[0] {
        "verify" => parse_verify(&positionals[1..], color),
        "codegen" => parse_codegen(&positionals[1..]),
        "hash-password" => parse_hash_password(&positionals[1..], password_file),
        "orchestrate" => parse_orchestrate(&positionals[1..], config),
        other => Err(ParseErr::Usage(format!("unknown command {other}"))),
    }
}

fn parse_color(v: &str) -> Result<ColorMode, ParseErr> {
    match v {
        "auto" => Ok(ColorMode::Auto),
        "always" => Ok(ColorMode::Always),
        "never" => Ok(ColorMode::Never),
        other => Err(ParseErr::Usage(format!(
            "invalid --color {other} (expected auto, always, or never)"
        ))),
    }
}

fn parse_verify(rest: &[&str], color: ColorMode) -> Result<Action, ParseErr> {
    if rest.len() < 2 {
        return Err(ParseErr::Usage(
            "verify requires <all|id[,id...]> and <n>".to_owned(),
        ));
    }
    if rest.len() > 2 {
        return Err(ParseErr::Usage("verify takes two arguments".to_owned()));
    }
    let selector = parse_selector(rest[0])?;
    let n = parse_n(rest[1])?;
    Ok(Action::Verify { selector, n, color })
}

fn parse_selector(raw: &str) -> Result<Selector, ParseErr> {
    if raw == "all" {
        return Ok(Selector::All);
    }
    if raw.is_empty() {
        return Err(ParseErr::Usage("problem selector is empty".to_owned()));
    }
    let mut ids = Vec::new();
    for part in raw.split(',') {
        match ProblemId::new(part) {
            Ok(id) => ids.push(id),
            Err(ProblemIdError::Empty) => {
                return Err(ParseErr::Usage("empty problem id in selector".to_owned()));
            }
            Err(ProblemIdError::Invalid) => {
                return Err(ParseErr::Usage(format!("invalid problem id {part}")));
            }
            Err(ProblemIdError::ReservedAll) => {
                return Err(ParseErr::Usage(
                    "all is the full-catalog selector, not a problem id".to_owned(),
                ));
            }
        }
    }
    Ok(Selector::Ids(ids))
}

fn parse_n(raw: &str) -> Result<u32, ParseErr> {
    let n: u32 = raw
        .parse()
        .map_err(|_| ParseErr::Usage(format!("invalid sample count {raw}")))?;
    if n == 0 {
        return Err(ParseErr::Usage("n must be at least 1".to_owned()));
    }
    Ok(n)
}

fn parse_codegen(rest: &[&str]) -> Result<Action, ParseErr> {
    let path = match rest {
        [] => PathBuf::from(tinker_protocol::GENERATED_DIR).join(tinker_protocol::HTTP_JS_FILE),
        [p] => PathBuf::from(*p),
        _ => {
            return Err(ParseErr::Usage("codegen takes at most one path".to_owned()));
        }
    };
    Ok(Action::Codegen { path })
}

fn parse_hash_password(rest: &[&str], file: Option<&str>) -> Result<Action, ParseErr> {
    if !rest.is_empty() {
        return Err(ParseErr::Usage(
            "hash-password takes no positional arguments".to_owned(),
        ));
    }
    Ok(Action::HashPassword {
        file: file.map(PathBuf::from),
    })
}

fn parse_orchestrate(rest: &[&str], config: Option<&str>) -> Result<Action, ParseErr> {
    if !rest.is_empty() {
        return Err(ParseErr::Usage(
            "orchestrate takes no positional arguments".to_owned(),
        ));
    }
    Ok(Action::Orchestrate {
        config: config.map(PathBuf::from),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile::CompileError;

    struct OkCompiler;

    impl CatalogCompiler for OkCompiler {
        fn compile(&self, _catalog_dir: &Path, _term_color: &str) -> Result<(), CompileError> {
            Ok(())
        }
    }

    struct FailCompiler;

    impl CatalogCompiler for FailCompiler {
        fn compile(&self, _catalog_dir: &Path, _term_color: &str) -> Result<(), CompileError> {
            Err(CompileError::Failed {
                code: Some(101),
                stderr: "nope".into(),
            })
        }
    }

    struct Prompt(&'static str);
    impl HiddenInput for Prompt {
        fn read_password(&mut self) -> Result<String, String> {
            Ok(self.0.to_owned())
        }
    }

    struct FailPrompt;
    impl HiddenInput for FailPrompt {
        fn read_password(&mut self) -> Result<String, String> {
            Err("no tty".into())
        }
    }

    fn run_args(
        args: &[&str],
        compiler: &dyn CatalogCompiler,
        no_color: bool,
    ) -> (i32, String, String) {
        run_args_full(
            args,
            compiler,
            no_color,
            &mut Prompt("secret"),
            &HashMap::new(),
        )
    }

    fn run_args_full(
        args: &[&str],
        compiler: &dyn CatalogCompiler,
        no_color: bool,
        hidden: &mut dyn HiddenInput,
        env: &HashMap<String, String>,
    ) -> (i32, String, String) {
        let os: Vec<OsString> = args.iter().map(|s| OsString::from(*s)).collect();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run(
            RunInput {
                args: &os,
                catalog_dir: Path::new("/unused-catalog"),
                no_color,
                compiler,
                hidden,
                env,
                shutdown: Shutdown::Immediate,
            },
            &mut out,
            &mut err,
        );
        (
            code,
            String::from_utf8(out).expect("utf8"),
            String::from_utf8(err).expect("utf8"),
        )
    }

    #[test]
    fn color_mapping() {
        assert_eq!(ColorMode::Always.cargo_term_color(true), "always");
        assert_eq!(ColorMode::Never.cargo_term_color(false), "never");
        assert_eq!(ColorMode::Auto.cargo_term_color(true), "never");
        assert_eq!(ColorMode::Auto.cargo_term_color(false), "auto");
        assert!(ColorMode::Always.enabled(true));
        assert!(!ColorMode::Never.enabled(false));
        assert!(!ColorMode::Auto.enabled(true));
        assert!(ColorMode::Auto.enabled(false));
    }

    #[test]
    fn help_and_version() {
        let (c, out, err) = run_args(&["tinker", "--help"], &OkCompiler, false);
        assert_eq!(c, 0);
        assert!(out.contains("Usage: tinker"));
        assert!(err.is_empty());
        let (c, out, _) = run_args(&["tinker", "-h"], &OkCompiler, false);
        assert_eq!(c, 0);
        assert!(out.contains("verify"));
        assert!(out.contains("codegen"));
        assert!(out.contains("hash-password"));
        assert!(out.contains("orchestrate"));
        let (c, out, _) = run_args(&["tinker", "--version"], &OkCompiler, false);
        assert_eq!(c, 0);
        assert_eq!(out.trim(), version());
        let (c, out, _) = run_args(&["tinker", "-V"], &OkCompiler, false);
        assert_eq!(c, 0);
        assert_eq!(out.trim(), version());
        let (c, out, _) = run_args(&["tinker", "verify", "all", "1", "-h"], &OkCompiler, false);
        assert_eq!(c, 0);
        assert!(out.contains("Usage:"));
    }

    #[test]
    fn usage_errors() {
        let cases: &[(&[&str], &str)] = &[
            (&["tinker"], "missing command"),
            (&["tinker", "nope"], "unknown command nope"),
            (&["tinker", "verify"], "verify requires"),
            (&["tinker", "verify", "all"], "verify requires"),
            (&["tinker", "verify", "all", "1", "x"], "two arguments"),
            (&["tinker", "verify", "all", "0"], "at least 1"),
            (&["tinker", "verify", "all", "no"], "invalid sample count"),
            (
                &["tinker", "verify", "all", "4294967296"],
                "invalid sample count",
            ),
            (&["tinker", "verify", "", "1"], "selector is empty"),
            (&["tinker", "verify", "a\0b", "1"], "invalid problem id"),
            (&["tinker", "verify", ",int-sum", "1"], "empty problem id"),
            (&["tinker", "verify", "int-sum,", "1"], "empty problem id"),
            (&["tinker", "verify", "a/b", "1"], "invalid problem id"),
            (
                &["tinker", "verify", "all,int-sum", "1"],
                "full-catalog selector",
            ),
            (&["tinker", "--color"], "missing value for --color"),
            (&["tinker", "--color", "rainbow"], "invalid --color"),
            (&["tinker", "--nope"], "unknown option"),
            (
                &["tinker", "--color=rainbow", "verify", "all", "1"],
                "invalid --color",
            ),
            (&["tinker", "codegen", "a", "b"], "at most one path"),
            (&["tinker", "hash-password", "x"], "no positional"),
            (&["tinker", "orchestrate", "x"], "no positional"),
            (
                &["tinker", "--password-file"],
                "missing value for --password-file",
            ),
            (&["tinker", "--config"], "missing value for --config"),
        ];
        for (args, needle) in cases {
            let (c, _, err) = run_args(args, &OkCompiler, false);
            assert_eq!(c, 2, "{args:?}");
            assert!(err.contains(needle), "{args:?} err={err}");
            assert!(err.contains("Usage: tinker"), "{args:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_arg() {
        use std::os::unix::ffi::OsStringExt;
        let args = vec![OsString::from("tinker"), OsString::from_vec(vec![0xff])];
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run(
            RunInput {
                args: &args,
                catalog_dir: Path::new("/unused"),
                no_color: false,
                compiler: &OkCompiler,
                hidden: &mut Prompt("x"),
                env: &HashMap::new(),
                shutdown: Shutdown::Immediate,
            },
            &mut out,
            &mut err,
        );
        assert_eq!(code, 2);
        assert!(String::from_utf8_lossy(&err).contains("UTF-8"));
    }

    #[test]
    fn verify_success_and_unknown_id() {
        let (c, out, err) = run_args(
            &["tinker", "--color", "never", "verify", "all", "4"],
            &OkCompiler,
            true,
        );
        assert_eq!(c, 0, "err={err}");
        assert!(out.contains("int-sum: 4 passed"));
        let (c, out, _) = run_args(
            &["tinker", "--color=always", "verify", "int-sum", "2"],
            &OkCompiler,
            false,
        );
        assert_eq!(c, 0);
        assert!(out.contains("int-sum: 2 passed"));
        let (c, out, _) = run_args(
            &[
                "tinker",
                "--color",
                "auto",
                "verify",
                "int-sum,int-sum",
                "1",
            ],
            &OkCompiler,
            false,
        );
        assert_eq!(c, 0);
        assert_eq!(out.matches("int-sum: 1 passed").count(), 2);
        let (c, _, err) = run_args(&["tinker", "verify", "missing", "1"], &OkCompiler, false);
        assert_eq!(c, 1);
        assert!(err.contains("unknown problem missing"));
    }

    #[test]
    fn compile_failure_is_exit_1() {
        let (c, _, err) = run_args(&["tinker", "verify", "all", "1"], &FailCompiler, false);
        assert_eq!(c, 1);
        assert!(err.contains("catalog compile failed"));
    }

    #[test]
    fn codegen_writes_and_reports_io_errors() {
        use std::fs;
        use std::time::{SystemTime, UNIX_EPOCH};

        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("tinker-cli-codegen-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&root).expect("scratch");
        let file = root.join("tinker-http.js");
        let path = file.to_str().expect("utf8 path");
        let (c, out, err) = run_args(&["tinker", "codegen", path], &FailCompiler, false);
        assert_eq!(c, 0, "err={err}");
        assert!(out.contains("wrote"));
        assert!(
            fs::read_to_string(&file)
                .expect("js")
                .contains("export function login(")
        );

        let dir = root.to_str().expect("utf8 dir");
        let (c, _, err) = run_args(&["tinker", "codegen", dir], &OkCompiler, false);
        assert_eq!(c, 1);
        assert!(err.contains("codegen failed"));
    }

    #[test]
    fn codegen_default_path() {
        use std::fs;

        let path =
            PathBuf::from(tinker_protocol::GENERATED_DIR).join(tinker_protocol::HTTP_JS_FILE);
        let _ = fs::remove_file(&path);
        let (c, out, err) = run_args(&["tinker", "codegen"], &OkCompiler, false);
        assert_eq!(c, 0, "err={err}");
        assert!(out.contains(tinker_protocol::HTTP_JS_FILE));
        assert!(path.is_file());
        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir(tinker_protocol::GENERATED_DIR);
    }

    #[test]
    fn hash_password_prompt_file_and_errors() {
        use std::fs;
        use std::time::{SystemTime, UNIX_EPOCH};

        let (c, out, err) = run_args(&["tinker", "hash-password"], &OkCompiler, false);
        assert_eq!(c, 0, "err={err}");
        assert!(out.contains("$argon2id$v=19$"));
        let (c, _, err) = run_args_full(
            &["tinker", "hash-password"],
            &OkCompiler,
            false,
            &mut FailPrompt,
            &HashMap::new(),
        );
        assert_eq!(c, 1);
        assert!(err.contains("no tty"));
        let (c, _, err) = run_args_full(
            &["tinker", "hash-password"],
            &OkCompiler,
            false,
            &mut Prompt(""),
            &HashMap::new(),
        );
        assert_eq!(c, 1);
        assert!(err.contains("empty"));

        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("t")
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("tinker-cli-hp-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&dir).expect("d");
        let file = dir.join("pw");
        fs::write(&file, "from-file\n").expect("w");
        let path = file.to_str().expect("utf8");
        let (c, out, err) = run_args(
            &["tinker", "--password-file", path, "hash-password"],
            &OkCompiler,
            false,
        );
        assert_eq!(c, 0, "err={err}");
        assert!(out.contains("$argon2id$"));
        let (c, out, _) = run_args(
            &[
                "tinker",
                &format!("--password-file={path}"),
                "hash-password",
            ],
            &OkCompiler,
            false,
        );
        assert_eq!(c, 0);
        assert!(out.contains("$argon2id$"));
        let missing = dir.join("missing");
        let (c, _, err) = run_args(
            &[
                "tinker",
                "--password-file",
                missing.to_str().expect("u"),
                "hash-password",
            ],
            &OkCompiler,
            false,
        );
        assert_eq!(c, 1);
        assert!(!err.is_empty());
    }

    #[test]
    fn orchestrate_refuses_without_secrets_and_binds() {
        use crate::orchestrate::hash_password;
        use std::fs;
        use std::time::{SystemTime, UNIX_EPOCH};

        let (c, _, err) = run_args(&["tinker", "orchestrate"], &OkCompiler, false);
        assert_eq!(c, 1);
        assert!(err.contains("hash") || err.contains("JWT") || err.contains("password"));

        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("t")
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("tinker-cli-or-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&dir).expect("d");
        let hash = hash_password("pw").expect("h");
        let toml = dir.join("tinker.toml");
        fs::write(
            &toml,
            format!(
                "public_listen = \"127.0.0.1:0\"\nadmin_listen = \"127.0.0.1:0\"\nadmin_password_hash = \"{hash}\"\njwt_hs256_secret = \"cli-secret\"\nrevoke_deny_file = \"{}\"\n",
                dir.join("deny").display()
            ),
        )
        .expect("w");
        let (c, out, err) = run_args(
            &[
                "tinker",
                "--config",
                toml.to_str().expect("u"),
                "orchestrate",
            ],
            &OkCompiler,
            false,
        );
        assert_eq!(c, 0, "err={err}");
        assert!(out.contains("public 127.0.0.1:"), "{out}");
        assert!(out.contains("admin 127.0.0.1:"), "{out}");
        assert!(!out.contains("cli-secret"));
        let (c, out, err) = run_args(
            &[
                "tinker",
                &format!("--config={}", toml.display()),
                "orchestrate",
            ],
            &OkCompiler,
            false,
        );
        assert_eq!(c, 0, "err={err} out={out}");

        let mut env = HashMap::new();
        env.insert("TINKER_ADMIN_PASSWORD_HASH".into(), hash);
        env.insert("TINKER_JWT_HS256_SECRET".into(), "env-secret".into());
        env.insert("TINKER_PUBLIC_LISTEN".into(), "127.0.0.1:0".into());
        env.insert("TINKER_ADMIN_LISTEN".into(), "127.0.0.1:0".into());
        env.insert(
            "TINKER_REVOKE_DENY_FILE".into(),
            dir.join("deny2").display().to_string(),
        );
        let (c, out, err) = run_args_full(
            &["tinker", "orchestrate"],
            &OkCompiler,
            false,
            &mut Prompt("x"),
            &env,
        );
        assert_eq!(c, 0, "err={err}");
        assert!(out.contains("public "));

        env.insert("TINKER_PUBLIC_LISTEN".into(), "255.255.255.255:1".into());
        let (c, _, err) = run_args_full(
            &["tinker", "orchestrate"],
            &OkCompiler,
            false,
            &mut Prompt("x"),
            &env,
        );
        assert_eq!(c, 1);
        assert!(!err.is_empty());
    }
}
