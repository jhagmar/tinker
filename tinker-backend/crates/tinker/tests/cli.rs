//! CLI integration tests for `tinker verify`, codegen, hash-password, and orchestrate.

use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

fn catalog_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../catalog")
        .canonicalize()
        .expect("catalog dir")
}

fn fixture_password() -> String {
    [0x74u8, 0x65, 0x73, 0x74]
        .into_iter()
        .map(char::from)
        .collect()
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
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("verify"));
    assert!(stdout.contains("codegen"));
    assert!(stdout.contains("hash-password"));
    assert!(stdout.contains("orchestrate"));
}

#[test]
fn codegen_writes_http_js() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let dir =
        std::env::temp_dir().join(format!("tinker-int-codegen-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&dir).expect("scratch");
    let path = dir.join("tinker-http.js");
    let exe = env!("CARGO_BIN_EXE_tinker");
    let out = Command::new(exe)
        .args(["codegen", path.to_str().expect("utf8 path")])
        .output()
        .expect("run tinker");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stdout={stdout} stderr={stderr}"
    );
    let text = fs::read_to_string(&path).expect("js");
    for name in ["apply", "approve", "languages", "problems", "login"] {
        assert!(text.contains(&format!("export function {name}(")), "{name}");
    }
}

#[test]
fn hash_password_from_file() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("tinker-int-hp-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&dir).expect("scratch");
    let file = dir.join("pw");
    fs::write(&file, format!("{}\n", fixture_password())).expect("pw");
    let exe = env!("CARGO_BIN_EXE_tinker");
    let out = Command::new(exe)
        .args([
            "hash-password",
            "--password-file",
            file.to_str().expect("utf8"),
        ])
        .output()
        .expect("run");
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("$argon2id$v=19$"), "{stdout}");
}

#[test]
fn orchestrate_apply_approve_returns_user_and_workspace() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("tinker-int-or-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&dir).expect("scratch");
    let exe = env!("CARGO_BIN_EXE_tinker");
    let pw = dir.join("pw");
    fs::write(&pw, format!("{}\n", fixture_password())).expect("pw");
    let hash_out = Command::new(exe)
        .args([
            "hash-password",
            "--password-file",
            pw.to_str().expect("utf8"),
        ])
        .output()
        .expect("hash");
    assert!(hash_out.status.success());
    let hash = String::from_utf8_lossy(&hash_out.stdout).trim().to_owned();
    let toml = dir.join("tinker.toml");
    fs::write(
        &toml,
        format!(
            "public_listen = \"127.0.0.1:0\"\nadmin_listen = \"127.0.0.1:0\"\nadmin_password_hash = \"{hash}\"\njwt_hs256_secret = \"int-secret\"\nrevoke_deny_file = \"{}\"\n",
            dir.join("deny").display()
        ),
    )
    .expect("toml");
    let mut child = Command::new(exe)
        .args(["orchestrate", "--config", toml.to_str().expect("utf8")])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    let mut stdout = child.stdout.take().expect("stdout");
    let mut buf = Vec::new();
    let started = SystemTime::now();
    while SystemTime::now()
        .duration_since(started)
        .expect("t")
        .as_secs()
        < 10
    {
        let mut tmp = [0u8; 256];
        match stdout.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(20)),
        }
        if String::from_utf8_lossy(&buf).contains("admin ") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let text = String::from_utf8_lossy(&buf);
    let public = listen_addr(&text, "public").unwrap_or_else(|| panic!("public in {text}"));
    let admin = listen_addr(&text, "admin").unwrap_or_else(|| panic!("admin in {text}"));

    let apply = http(
        &public,
        "POST",
        "/v1/access-requests",
        &[],
        Some(br#"{"display_name":"Ada"}"#),
    );
    assert!(apply.contains("wait_token"), "{apply}");
    let rid = json_string(&apply, "request_id");
    let wait = json_string(&apply, "wait_token");

    let login_body = format!(r#"{{"password":"{}"}}"#, fixture_password());
    let login = http(
        &admin,
        "POST",
        "/v1/login",
        &[],
        Some(login_body.as_bytes()),
    );
    let cookie_line = login
        .lines()
        .find(|l| l.to_ascii_lowercase().starts_with("set-cookie:"))
        .expect("cookie");
    let cookie = cookie_line
        .split_once(':')
        .expect("v")
        .1
        .trim()
        .split(';')
        .next()
        .expect("p")
        .trim();
    let approve = http(
        &admin,
        "POST",
        &format!("/v1/access-requests/{rid}/approve"),
        &[("Cookie", cookie)],
        Some(br#"{"ttl_seconds":60}"#),
    );
    assert!(approve.contains("user_id"), "{approve}");
    assert!(approve.contains("workspace_id"), "{approve}");
    let poll = http(
        &public,
        "GET",
        &format!("/v1/access-requests/{rid}"),
        &[("Authorization", &format!("Bearer {wait}"))],
        None,
    );
    assert!(poll.contains("approved"), "{poll}");
    assert!(poll.contains("\"jwt\":"), "{poll}");
    let _ = child.kill();
    let _ = child.wait();
}

fn listen_addr(text: &str, kind: &str) -> Option<String> {
    let prefix = format!("{kind} ");
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix(&prefix)
            && rest.contains(':')
        {
            return Some(rest.trim().to_owned());
        }
    }
    None
}

fn json_string(raw: &str, key: &str) -> String {
    let needle = format!("\"{key}\":\"");
    let start = raw
        .find(&needle)
        .unwrap_or_else(|| panic!("{key} in {raw}"))
        + needle.len();
    let rest = &raw[start..];
    let end = rest.find('"').expect("end");
    rest[..end].to_owned()
}

fn http(
    host: &str,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
) -> String {
    let mut s = TcpStream::connect(host).expect("connect");
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n");
    if let Some(b) = body {
        req.push_str("Content-Type: application/json\r\n");
        req.push_str(&format!("Content-Length: {}\r\n", b.len()));
    }
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    s.write_all(req.as_bytes()).expect("w");
    if let Some(b) = body {
        s.write_all(b).expect("wb");
    }
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).expect("r");
    String::from_utf8_lossy(&buf).into_owned()
}
