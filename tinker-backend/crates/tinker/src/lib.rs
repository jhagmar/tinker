//! Host binary for Tinker workshops.

/// Package version from Cargo.toml.
#[must_use]
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Mixtrapi protocol id this host will speak.
#[must_use]
pub fn protocol_id() -> &'static str {
    tinker_protocol::PROTOCOL_ID
}

/// Catalog directory name beside the host workspace.
#[must_use]
pub fn catalog_dir() -> &'static str {
    tinker_catalog::CATALOG_DIR
}

/// Protocol id of the workspace agent copied into language images.
#[must_use]
pub fn agent_protocol_id() -> &'static str {
    tinker_agent::PROTOCOL_ID
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_has_three_numeric_segments() {
        let mut parts = version().split('.');
        for _ in 0..3 {
            let part = parts.next().expect("semver segment");
            assert!(part.chars().all(|c| c.is_ascii_digit()));
        }
        assert_eq!(parts.next(), None);
    }

    #[test]
    fn protocol_matches_agent() {
        assert_eq!(protocol_id(), "mixtrapi/1");
        assert_eq!(agent_protocol_id(), protocol_id());
        assert_eq!(catalog_dir(), "catalog");
    }
}
