//! Shipped catalog problems.

mod int_sum;

use crate::problem::ProblemEntry;

/// Inventory of compiled-in problems. A second problem is one module plus one line here.
#[must_use]
pub fn entries() -> &'static [ProblemEntry] {
    static ENTRIES: &[ProblemEntry] = &[int_sum::ENTRY];
    ENTRIES
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::problem::Problem;
    use crate::sampler::Sampler;
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
    fn inventory_is_int_sum() {
        assert_eq!(entries().len(), 1);
        let entry = &entries()[0];
        assert_eq!(entry.id().as_str(), "int-sum");
        assert_eq!(entry.description(), int_sum::IntSum::description());
        assert_eq!(entry.batch_size(), 1);
        assert_eq!(entry.visualize(), None);
        assert_eq!(
            entry.instance_schema().to_json(),
            int_sum::IntSum::instance_schema().to_json()
        );
        assert_eq!(
            entry.answer_schema().to_json(),
            int_sum::IntSum::answer_schema().to_json()
        );
        assert!(entry.verify_one(&mut Step(0)).is_pass());
        let report = verify(entries(), &Selector::All, 8, &mut Step(1)).expect("ok");
        assert_eq!(report.problems[0].1, 8);
    }
}
