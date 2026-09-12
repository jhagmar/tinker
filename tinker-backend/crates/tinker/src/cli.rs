//! Hand-rolled CLI: `tinker verify`, help, version, `--color`.

use std::ffi::OsString;
use std::io::Write;
use std::path::Path;

use tinker_catalog::{ProblemId, ProblemIdError, Selector, entries, verify};

use crate::compile::CatalogCompiler;
use crate::sampler::{SplitMix64, VERIFY_SEED};
use crate::version;

const USAGE: &str = "\
Usage: tinker [options] <command>

Commands:
  verify <all|id[,id...]> <n>  Sample n instances per selected problem

Options:
  -h, --help            Show this help
  -V, --version         Show version
      --color <when>    auto, always, or never
";

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
}

enum ParseErr {
    Usage(String),
}

/// Parse args and run. Returns a process exit code (0, 1, or 2).
pub fn run(
    args: &[OsString],
    catalog_dir: &Path,
    no_color: bool,
    compiler: &dyn CatalogCompiler,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
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
    }
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

    fn run_args(
        args: &[&str],
        compiler: &dyn CatalogCompiler,
        no_color: bool,
    ) -> (i32, String, String) {
        let os: Vec<OsString> = args.iter().map(|s| OsString::from(*s)).collect();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run(
            &os,
            Path::new("/unused-catalog"),
            no_color,
            compiler,
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
            &args,
            Path::new("/unused"),
            false,
            &OkCompiler,
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
}
