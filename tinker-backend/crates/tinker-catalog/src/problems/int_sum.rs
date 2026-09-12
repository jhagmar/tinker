//! Array of integers → sum.

use crate::json::{Json, JsonError};
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

impl IntList {
    /// Encode as `{ "v": [integer, ...] }`.
    #[must_use]
    pub fn to_json(&self) -> Json {
        Json::object(vec![(
            "v".to_owned(),
            Json::Array(self.v.iter().copied().map(Json::int).collect()),
        )])
        .expect("single key")
    }

    /// Decode `{ "v": [integer, ...] }`.
    ///
    /// # Errors
    ///
    /// Returns [`JsonError`] when the value is not this object shape or an element is not an `i64`.
    pub fn from_json(value: &Json) -> Result<Self, JsonError> {
        let Json::Object(pairs) = value else {
            return Err(JsonError::ExpectedObject);
        };
        let mut found = None;
        for (k, val) in pairs {
            if k == "v" {
                found = Some(val);
            } else {
                return Err(JsonError::ExtraField);
            }
        }
        let Some(field) = found else {
            return Err(JsonError::MissingField);
        };
        let Json::Array(items) = field else {
            return Err(JsonError::ExpectedArray);
        };
        let mut v = Vec::with_capacity(items.len());
        for item in items {
            let Json::Int(n) = item else {
                return Err(JsonError::ExpectedInt);
            };
            v.push(n.to_i64()?);
        }
        Ok(Self { v })
    }
}

impl Sum {
    /// Encode as a JSON integer.
    #[must_use]
    pub fn to_json(self) -> Json {
        Json::int(self.0)
    }

    /// Decode a JSON integer.
    ///
    /// # Errors
    ///
    /// Returns [`JsonError`] when the value is not an `i64` integer.
    pub fn from_json(value: &Json) -> Result<Self, JsonError> {
        match value {
            Json::Int(n) => Ok(Self(n.to_i64()?)),
            _ => Err(JsonError::ExpectedInt),
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
    fn json_round_trip_goldens() {
        use crate::json::Json;
        let inst: Json = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../languages/goldens/int-sum-instance.json"
        ))
        .trim()
        .parse()
        .expect("inst");
        let list = IntList::from_json(&inst).expect("list");
        assert_eq!(list.v, vec![1, 2, 3]);
        assert_eq!(
            list.to_json().to_compact_string().expect("enc"),
            inst.to_compact_string().expect("enc")
        );
        let wide: Json = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../languages/goldens/int-sum-instance-wide.json"
        ))
        .trim()
        .parse()
        .expect("wide");
        let list = IntList::from_json(&wide).expect("wide list");
        assert_eq!(list.v, vec![1, 9_007_199_254_740_992]);
        assert_eq!(IntSum::reference_solve(&list), Sum(9_007_199_254_740_993));
        assert_eq!(
            IntSum::reference_solve(&list)
                .to_json()
                .to_compact_string()
                .expect("ans"),
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../languages/goldens/int-sum-answer-wide.json"
            ))
            .trim()
        );
        let ans: Json = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../languages/goldens/int-sum-answer.json"
        ))
        .trim()
        .parse()
        .expect("ans");
        assert_eq!(Sum::from_json(&ans).expect("sum"), Sum(6));
        let empty_arr: Json = "[]".parse().expect("arr");
        assert_eq!(
            IntList::from_json(&empty_arr),
            Err(JsonError::ExpectedObject)
        );
        assert_eq!(
            IntList::from_json(&Json::object(Vec::new()).expect("obj")),
            Err(JsonError::MissingField)
        );
        assert_eq!(
            IntList::from_json(
                &Json::object(vec![
                    ("v".into(), Json::Array(Vec::new())),
                    ("x".into(), Json::Null),
                ])
                .expect("extra")
            ),
            Err(JsonError::ExtraField)
        );
        assert_eq!(
            IntList::from_json(&Json::object(vec![("v".into(), Json::int(1))]).expect("v")),
            Err(JsonError::ExpectedArray)
        );
        assert_eq!(
            IntList::from_json(
                &Json::object(vec![("v".into(), Json::Array(vec![Json::Bool(true)]))]).expect("v")
            ),
            Err(JsonError::ExpectedInt)
        );
        let huge: Json = "{\"$i\":\"999999999999999999999\"}".parse().expect("huge");
        assert_eq!(Sum::from_json(&huge), Err(JsonError::IntRange));
        assert_eq!(Sum::from_json(&Json::Null), Err(JsonError::ExpectedInt));
        assert_eq!(
            IntList::from_json(
                &Json::object(vec![("v".into(), Json::Array(vec![huge]),)]).expect("v")
            ),
            Err(JsonError::IntRange)
        );
    }

    #[test]
    fn verify_int_sum() {
        let report = verify(&[ENTRY], &Selector::All, 32, &mut Step(7)).expect("ok");
        assert_eq!(report.problems[0].0.as_str(), "int-sum");
    }
}
