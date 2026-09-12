//! Write `generated/tinker-http.js`.

use std::fs;
use std::io;
use std::path::Path;

/// Write the HTTP JavaScript module to `path`, creating parent directories.
///
/// # Errors
///
/// Returns [`io::Error`] when directories or the file cannot be written.
pub fn write_http_js(path: &Path) -> io::Result<()> {
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(fs::create_dir_all)
        .transpose()?;
    fs::write(path, tinker_protocol::javascript_module())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn scratch(tag: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let dir = env::temp_dir().join(format!(
            "tinker-codegen-{tag}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    #[test]
    fn writes_nested_path() {
        let dir = scratch("ok");
        let path = dir.join("nested").join("tinker-http.js");
        write_http_js(&path).expect("write");
        let text = fs::read_to_string(&path).expect("read");
        assert_eq!(text, tinker_protocol::javascript_module());
        assert!(text.contains("export function apply("));
    }

    #[test]
    fn create_dir_fails_when_parent_is_a_file() {
        let dir = scratch("parent-file");
        let file = dir.join("not-a-dir");
        fs::write(&file, b"x").expect("file");
        write_http_js(&file.join("tinker-http.js")).expect_err("parent file");
    }

    #[test]
    fn write_fails_when_path_is_a_directory() {
        let dir = scratch("is-dir");
        write_http_js(&dir).expect_err("directory");
    }

    #[test]
    fn checked_in_module_matches_generator() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../generated/tinker-http.js");
        let on_disk = fs::read_to_string(&path).unwrap_or_default();
        assert_eq!(
            on_disk,
            tinker_protocol::javascript_module(),
            "run `cargo run -p tinker -- codegen generated/tinker-http.js` from tinker-backend/"
        );
    }
}
