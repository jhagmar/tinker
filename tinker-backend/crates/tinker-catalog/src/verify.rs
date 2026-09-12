//! In-process Monte-Carlo verify.

use core::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::problem::ProblemEntry;
use crate::problem_id::ProblemId;
use crate::sampler::Sampler;

/// Which problems to sample.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Selector {
    /// Every registered problem.
    All,
    /// Explicit stems, in CLI order.
    Ids(Vec<ProblemId>),
}

/// Per-problem sample counts after a successful run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifyReport {
    /// `(id, n)` for each selected problem.
    pub problems: Vec<(ProblemId, u32)>,
}

/// Why [`verify`] failed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VerifyError {
    /// `n` was 0.
    EmptyN,
    /// A requested id is not in the catalog.
    UnknownId(ProblemId),
    /// `judge` returned [`crate::Judgement::Fail`].
    JudgeFailed {
        /// Problem that failed.
        id: ProblemId,
        /// 0-based sample index.
        sample: u32,
    },
    /// Generate, solve, or judge panicked.
    Panicked {
        /// Problem that panicked.
        id: ProblemId,
        /// 0-based sample index.
        sample: u32,
    },
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyN => f.write_str("n must be at least 1"),
            Self::UnknownId(id) => write!(f, "unknown problem {id}"),
            Self::JudgeFailed { id, sample } => {
                write!(f, "judge failed for {id} sample {sample}")
            }
            Self::Panicked { id, sample } => {
                write!(f, "panic in {id} sample {sample}")
            }
        }
    }
}

/// Monte-Carlo-sample `n` instances per selected problem.
///
/// # Errors
///
/// Returns [`VerifyError`] when `n` is 0, an id is missing, judge fails, or a sample panics.
pub fn verify(
    entries: &[ProblemEntry],
    selector: &Selector,
    n: u32,
    sampler: &mut dyn Sampler,
) -> Result<VerifyReport, VerifyError> {
    if n == 0 {
        return Err(VerifyError::EmptyN);
    }
    let selected = select(entries, selector)?;
    let mut problems = Vec::with_capacity(selected.len());
    for entry in selected {
        let id = entry.id();
        for sample in 0..n {
            let judgement = catch_unwind(AssertUnwindSafe(|| entry.verify_one(sampler)));
            match judgement {
                Ok(j) if j.is_pass() => {}
                Ok(_) => {
                    return Err(VerifyError::JudgeFailed { id, sample });
                }
                Err(_) => {
                    return Err(VerifyError::Panicked { id, sample });
                }
            }
        }
        problems.push((id, n));
    }
    Ok(VerifyReport { problems })
}

fn select<'a>(
    entries: &'a [ProblemEntry],
    selector: &Selector,
) -> Result<Vec<&'a ProblemEntry>, VerifyError> {
    match selector {
        Selector::All => Ok(entries.iter().collect()),
        Selector::Ids(ids) => {
            let mut out = Vec::with_capacity(ids.len());
            for id in ids {
                let Some(entry) = entries.iter().find(|e| e.id() == *id) else {
                    return Err(VerifyError::UnknownId(id.clone()));
                };
                out.push(entry);
            }
            Ok(out)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::problem::{Judgement, Problem, ProblemEntry};
    use crate::schema::Schema;

    struct Step(u64);

    impl Sampler for Step {
        fn next_u64(&mut self) -> u64 {
            let v = self.0;
            self.0 = self.0.wrapping_add(1);
            v
        }
    }

    struct FailP;

    impl Problem for FailP {
        type Instance = ();
        type Answer = ();

        fn id() -> ProblemId {
            ProblemId::new("fail-p").expect("id")
        }

        fn description() -> &'static str {
            "fail"
        }

        fn instance_schema() -> Schema {
            Schema::integer()
        }

        fn answer_schema() -> Schema {
            Schema::integer()
        }

        fn generate(_sampler: &mut dyn Sampler) -> Self::Instance {}

        fn reference_solve(_instance: &Self::Instance) -> Self::Answer {}

        fn judge(_instance: &Self::Instance, _answer: &Self::Answer) -> Judgement {
            Judgement::Fail
        }
    }

    struct PanicP;

    impl Problem for PanicP {
        type Instance = ();
        type Answer = ();

        fn id() -> ProblemId {
            ProblemId::new("panic-p").expect("id")
        }

        fn description() -> &'static str {
            "panic"
        }

        fn instance_schema() -> Schema {
            Schema::integer()
        }

        fn answer_schema() -> Schema {
            Schema::integer()
        }

        fn generate(_sampler: &mut dyn Sampler) -> Self::Instance {
            panic!("boom");
        }

        fn reference_solve(_instance: &Self::Instance) -> Self::Answer {}

        fn judge(_instance: &Self::Instance, _answer: &Self::Answer) -> Judgement {
            Judgement::Pass
        }
    }

    struct LocalP;

    impl Problem for LocalP {
        type Instance = u64;
        type Answer = u64;

        fn id() -> ProblemId {
            ProblemId::new("local-p").expect("id")
        }

        fn description() -> &'static str {
            "local"
        }

        fn instance_schema() -> Schema {
            Schema::integer()
        }

        fn answer_schema() -> Schema {
            Schema::integer()
        }

        fn generate(sampler: &mut dyn Sampler) -> Self::Instance {
            sampler.next_u64()
        }

        fn reference_solve(instance: &Self::Instance) -> Self::Answer {
            *instance
        }

        fn judge(instance: &Self::Instance, answer: &Self::Answer) -> Judgement {
            if instance == answer {
                Judgement::Pass
            } else {
                Judgement::Fail
            }
        }
    }

    #[test]
    fn empty_n() {
        let entries = [ProblemEntry::bind::<LocalP>()];
        let err = verify(&entries, &Selector::All, 0, &mut Step(0)).unwrap_err();
        assert_eq!(err, VerifyError::EmptyN);
        assert_eq!(err.to_string(), "n must be at least 1");
    }

    #[test]
    fn unknown_id() {
        let entries = [ProblemEntry::bind::<LocalP>()];
        let missing = ProblemId::new("missing").expect("id");
        let err = verify(
            &entries,
            &Selector::Ids(vec![missing.clone()]),
            1,
            &mut Step(0),
        )
        .unwrap_err();
        assert_eq!(err, VerifyError::UnknownId(missing));
        assert_eq!(err.to_string(), "unknown problem missing");
    }

    #[test]
    fn judge_failed() {
        let entries = [ProblemEntry::bind::<FailP>()];
        assert_eq!(entries[0].description(), "fail");
        assert_eq!(entries[0].instance_schema(), Schema::integer());
        assert_eq!(entries[0].answer_schema(), Schema::integer());
        let err = verify(&entries, &Selector::All, 1, &mut Step(0)).unwrap_err();
        assert_eq!(
            err,
            VerifyError::JudgeFailed {
                id: ProblemId::new("fail-p").expect("id"),
                sample: 0,
            }
        );
        assert_eq!(err.to_string(), "judge failed for fail-p sample 0");
    }

    #[test]
    fn panicked() {
        let entries = [ProblemEntry::bind::<PanicP>()];
        assert_eq!(entries[0].description(), "panic");
        assert_eq!(entries[0].instance_schema(), Schema::integer());
        assert_eq!(entries[0].answer_schema(), Schema::integer());
        PanicP::reference_solve(&());
        assert_eq!(PanicP::judge(&(), &()), Judgement::Pass);
        let err = verify(&entries, &Selector::All, 1, &mut Step(0)).unwrap_err();
        assert_eq!(
            err,
            VerifyError::Panicked {
                id: ProblemId::new("panic-p").expect("id"),
                sample: 0,
            }
        );
        assert_eq!(err.to_string(), "panic in panic-p sample 0");
    }

    #[test]
    fn local_module_passes() {
        let entries = [ProblemEntry::bind::<LocalP>()];
        assert_eq!(entries[0].description(), "local");
        assert_eq!(entries[0].instance_schema(), Schema::integer());
        assert_eq!(entries[0].answer_schema(), Schema::integer());
        assert_eq!(LocalP::judge(&1, &2), Judgement::Fail);
        let report = verify(&entries, &Selector::All, 8, &mut Step(1)).expect("ok");
        assert_eq!(report.problems.len(), 1);
        assert_eq!(report.problems[0].0.as_str(), "local-p");
        assert_eq!(report.problems[0].1, 8);
    }

    #[test]
    fn selector_ids_order() {
        let entries = [
            ProblemEntry::bind::<LocalP>(),
            ProblemEntry::bind::<FailP>(),
        ];
        let report = verify(
            &entries,
            &Selector::Ids(vec![ProblemId::new("local-p").expect("id")]),
            2,
            &mut Step(0),
        )
        .expect("ok");
        assert_eq!(report.problems[0].0.as_str(), "local-p");
    }
}
