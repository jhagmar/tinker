//! Problem catalog compiled into one fat binary at host start.

/// Directory name for problem sources beside the host workspace.
pub const CATALOG_DIR: &str = "catalog";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_dir_name() {
        assert_eq!(CATALOG_DIR, "catalog");
    }
}
