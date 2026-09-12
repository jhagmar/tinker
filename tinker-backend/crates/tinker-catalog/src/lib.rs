//! Problem catalog: trait, JSON Schema, in-process verify, and problem modules.

mod problem;
mod problem_id;
mod problems;
mod sampler;
mod schema;
mod verify;

pub use problem::{Judgement, Problem, ProblemEntry};
pub use problem_id::{ProblemId, ProblemIdError};
pub use problems::entries;
pub use sampler::Sampler;
pub use schema::Schema;
pub use verify::{Selector, VerifyError, VerifyReport, verify};

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
