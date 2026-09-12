//! Array of integers → sum.

use crate::problem::{Judgement, Problem, ProblemEntry};
use crate::problem_id::ProblemId;
use crate::sampler::Sampler;
use crate::schema::Schema;

/// Catalog row for [`IntSum`].
pub(crate) static ENTRY: ProblemEntry = ProblemEntry::bind::<IntSum>();

/// Language-agnostic sum of a small integer list.
pub struct IntSum;

/// Instance: JSON object `{ "v": [integer, ...] }`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IntList {
    /// Values to sum.
    pub v: Vec<i64>,
}

/// Answer: JSON integer (the sum).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Sum(pub i64);

const MIN_LEN: u64 = 1;
const MAX_LEN: u64 = 16;
const MIN_VAL: i64 = -1000;
const MAX_VAL: i64 = 1000;

impl Problem for IntSum {
    type Instance = IntList;
    type Answer = Sum;

    fn id() -> ProblemId {
        ProblemId::new("int-sum").expect("stem")
    }

    fn description() -> &'static str {
        "Given an array of integers v, return their sum."
    }

    fn instance_schema() -> Schema {
        Schema::object(
            vec![("v".to_owned(), Schema::array(Schema::integer()))],
            vec!["v".to_owned()],
        )
    }

    fn answer_schema() -> Schema {
        Schema::integer()
    }

    fn generate(sampler: &mut dyn Sampler) -> Self::Instance {
        let len = MIN_LEN + sampler.next_below(MAX_LEN - MIN_LEN + 1);
        let mut v = Vec::with_capacity(len as usize);
        for _ in 0..len {
            v.push(sampler.next_i64_inclusive(MIN_VAL, MAX_VAL));
        }
        IntList { v }
    }

    fn reference_solve(instance: &Self::Instance) -> Self::Answer {
        Sum(instance.v.iter().copied().sum())
    }

    fn judge(instance: &Self::Instance, answer: &Self::Answer) -> Judgement {
        if Self::reference_solve(instance) == *answer {
            Judgement::Pass
        } else {
            Judgement::Fail
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verify::{Selector, verify};

    struct Step(u64);

    impl Sampler for Step {
        fn next_u64(&mut self) -> u64 {
            let v = self.0;
            self.0 = self.0.wrapping_add(1);
            v
        }
    }

    #[test]
    fn id_and_schemas() {
        assert_eq!(IntSum::id().as_str(), "int-sum");
        assert_eq!(IntSum::batch_size(), 1);
        assert_eq!(IntSum::visualize(), None);
        let inst = IntSum::instance_schema().to_json();
        assert!(inst.contains("\"v\""));
        assert_eq!(IntSum::answer_schema().to_json(), "{\"type\":\"integer\"}");
    }

    #[test]
    fn wrong_answer_fails_judge() {
        let inst = IntList { v: vec![1, 2, 3] };
        assert_eq!(IntSum::reference_solve(&inst), Sum(6));
        assert_eq!(IntSum::judge(&inst, &Sum(6)), Judgement::Pass);
        assert_eq!(IntSum::judge(&inst, &Sum(0)), Judgement::Fail);
    }

    #[test]
    fn generate_stays_in_bounds() {
        let mut s = Step(0);
        for _ in 0..32 {
            let inst = IntSum::generate(&mut s);
            assert!((MIN_LEN as usize..=MAX_LEN as usize).contains(&inst.v.len()));
            for x in &inst.v {
                assert!((MIN_VAL..=MAX_VAL).contains(x));
            }
            let _ = IntSum::judge(&inst, &IntSum::reference_solve(&inst));
        }
    }

    #[test]
    fn verify_int_sum() {
        let report = verify(&[ENTRY], &Selector::All, 32, &mut Step(7)).expect("ok");
        assert_eq!(report.problems[0].0.as_str(), "int-sum");
    }
}
