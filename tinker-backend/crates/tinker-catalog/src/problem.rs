//! Problem trait and object-safe catalog entry.

use crate::problem_id::ProblemId;
use crate::sampler::Sampler;
use crate::schema::Schema;

/// Outcome of [`Problem::judge`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Judgement {
    /// The answer matches the instance.
    Pass,
    /// The answer is wrong.
    Fail,
}

impl Judgement {
    /// Whether this is [`Judgement::Pass`].
    #[must_use]
    pub fn is_pass(self) -> bool {
        matches!(self, Self::Pass)
    }
}

/// Language-agnostic problem: generate, reference solve, judge.
pub trait Problem {
    /// Instance type for [`Self::generate`].
    type Instance;
    /// Answer type for [`Self::reference_solve`].
    type Answer;

    /// Catalog stem.
    fn id() -> ProblemId;

    /// Human-readable statement.
    fn description() -> &'static str;

    /// Jobs per batch (default 1).
    fn batch_size() -> u32 {
        1
    }

    /// Optional visualization hint.
    fn visualize() -> Option<&'static str> {
        None
    }

    /// JSON Schema for the instance object.
    fn instance_schema() -> Schema;

    /// JSON Schema for the answer object.
    fn answer_schema() -> Schema;

    /// Sample one instance.
    fn generate(sampler: &mut dyn Sampler) -> Self::Instance;

    /// Canonical solution.
    fn reference_solve(instance: &Self::Instance) -> Self::Answer;

    /// Accept or reject an answer.
    fn judge(instance: &Self::Instance, answer: &Self::Answer) -> Judgement;
}

fn verify_bound<P: Problem>(sampler: &mut dyn Sampler) -> Judgement {
    let instance = P::generate(sampler);
    let answer = P::reference_solve(&instance);
    P::judge(&instance, &answer)
}

/// Object-safe catalog row. A second problem is one module plus one [`crate::entries`] line.
#[derive(Clone, Copy)]
pub struct ProblemEntry {
    id: fn() -> ProblemId,
    description: fn() -> &'static str,
    batch_size: fn() -> u32,
    visualize: fn() -> Option<&'static str>,
    instance_schema: fn() -> Schema,
    answer_schema: fn() -> Schema,
    verify_one: fn(&mut dyn Sampler) -> Judgement,
}

impl ProblemEntry {
    /// Bind a [`Problem`] implementation as a catalog row.
    #[must_use]
    pub const fn bind<P: Problem>() -> Self {
        Self {
            id: P::id,
            description: P::description,
            batch_size: P::batch_size,
            visualize: P::visualize,
            instance_schema: P::instance_schema,
            answer_schema: P::answer_schema,
            verify_one: verify_bound::<P>,
        }
    }

    /// Catalog stem.
    #[must_use]
    pub fn id(&self) -> ProblemId {
        (self.id)()
    }

    /// Human-readable statement.
    #[must_use]
    pub fn description(&self) -> &'static str {
        (self.description)()
    }

    /// Jobs per batch.
    #[must_use]
    pub fn batch_size(&self) -> u32 {
        (self.batch_size)()
    }

    /// Optional visualization hint.
    #[must_use]
    pub fn visualize(&self) -> Option<&'static str> {
        (self.visualize)()
    }

    /// Instance JSON Schema.
    #[must_use]
    pub fn instance_schema(&self) -> Schema {
        (self.instance_schema)()
    }

    /// Answer JSON Schema.
    #[must_use]
    pub fn answer_schema(&self) -> Schema {
        (self.answer_schema)()
    }

    /// One generate / reference-solve / judge sample.
    #[must_use]
    pub fn verify_one(&self, sampler: &mut dyn Sampler) -> Judgement {
        (self.verify_one)(sampler)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::problem_id::ProblemId;
    use crate::schema::Schema;

    struct Dummy;

    struct Unit;

    impl Problem for Dummy {
        type Instance = Unit;
        type Answer = Unit;

        fn id() -> ProblemId {
            ProblemId::new("dummy").expect("id")
        }

        fn description() -> &'static str {
            "dummy"
        }

        fn batch_size() -> u32 {
            4
        }

        fn visualize() -> Option<&'static str> {
            Some("grid")
        }

        fn instance_schema() -> Schema {
            Schema::integer()
        }

        fn answer_schema() -> Schema {
            Schema::integer()
        }

        fn generate(sampler: &mut dyn Sampler) -> Self::Instance {
            let _ = sampler.next_u64();
            Unit
        }

        fn reference_solve(_instance: &Self::Instance) -> Self::Answer {
            Unit
        }

        fn judge(_instance: &Self::Instance, _answer: &Self::Answer) -> Judgement {
            Judgement::Pass
        }
    }

    #[test]
    fn judgement_is_pass() {
        assert!(Judgement::Pass.is_pass());
        assert!(!Judgement::Fail.is_pass());
    }

    #[test]
    fn bind_exposes_defaults_and_schemas() {
        let entry = ProblemEntry::bind::<Dummy>();
        assert_eq!(entry.id().as_str(), "dummy");
        assert_eq!(entry.description(), "dummy");
        assert_eq!(entry.batch_size(), 4);
        assert_eq!(entry.visualize(), Some("grid"));
        assert_eq!(entry.instance_schema(), Schema::integer());
        assert_eq!(entry.answer_schema(), Schema::integer());
        struct Z;
        impl Sampler for Z {
            fn next_u64(&mut self) -> u64 {
                0
            }
        }
        assert!(entry.verify_one(&mut Z).is_pass());
    }
}
