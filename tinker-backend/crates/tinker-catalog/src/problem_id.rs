//! Catalog stem identifier.

use core::fmt;

/// Non-empty UTF-8 catalog stem (`int-sum`).
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ProblemId(String);

/// Why a [`ProblemId`] was rejected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProblemIdError {
    /// Empty after construction.
    Empty,
    /// Contains `/` or a NUL byte.
    Invalid,
    /// The reserved selector word `all`.
    ReservedAll,
}

impl ProblemId {
    /// Parse a catalog stem.
    ///
    /// # Errors
    ///
    /// Returns [`ProblemIdError`] when the stem is empty, reserved, or contains `/` or NUL.
    pub fn new(raw: &str) -> Result<Self, ProblemIdError> {
        if raw.is_empty() {
            return Err(ProblemIdError::Empty);
        }
        if raw == "all" {
            return Err(ProblemIdError::ReservedAll);
        }
        if raw.as_bytes().contains(&b'/') || raw.as_bytes().contains(&0) {
            return Err(ProblemIdError::Invalid);
        }
        Ok(Self(raw.to_owned()))
    }

    /// Stem bytes as `str`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProblemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_stem() {
        let id = ProblemId::new("int-sum").expect("stem");
        assert_eq!(id.as_str(), "int-sum");
        assert_eq!(id.to_string(), "int-sum");
    }

    #[test]
    fn rejects_empty_slash_nul_and_all() {
        assert_eq!(ProblemId::new(""), Err(ProblemIdError::Empty));
        assert_eq!(ProblemId::new("a/b"), Err(ProblemIdError::Invalid));
        assert_eq!(ProblemId::new("a\0b"), Err(ProblemIdError::Invalid));
        assert_eq!(ProblemId::new("all"), Err(ProblemIdError::ReservedAll));
    }
}
