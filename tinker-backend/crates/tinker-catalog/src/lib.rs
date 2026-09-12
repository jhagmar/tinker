//! Problem catalog: trait, JSON Schema, JSON data model, in-process verify, and problem modules.

mod json;
mod problem;
mod problem_id;
mod problems;
mod sampler;
mod schema;
mod verify;

pub use json::{Json, JsonError, JsonInt, MAX_BYTES, MAX_DEPTH, MAX_SAFE_INT};
pub use problem::{Judgement, Problem, ProblemEntry};
pub use problem_id::{ProblemId, ProblemIdError};
pub use problems::{IntList, IntSum, Sum, entries};
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
