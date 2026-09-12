//! Monte-Carlo sampler port.

/// Byte source for [`crate::Problem::generate`].
pub trait Sampler {
    /// Next uniform `u64`.
    fn next_u64(&mut self) -> u64;

    /// Uniform value in `0..n`.
    ///
    /// # Panics
    ///
    /// Panics when `n` is 0.
    fn next_below(&mut self, n: u64) -> u64 {
        assert!(n > 0, "n > 0");
        let zone = u64::MAX - (u64::MAX % n);
        loop {
            let x = self.next_u64();
            if x < zone {
                return x % n;
            }
        }
    }

    /// Inclusive `i64` range.
    ///
    /// # Panics
    ///
    /// Panics when `low` > `high`.
    fn next_i64_inclusive(&mut self, low: i64, high: i64) -> i64 {
        assert!(low <= high, "low <= high");
        let span = (i128::from(high) - i128::from(low) + 1) as u64;
        let off = self.next_below(span);
        low.checked_add(off as i64).expect("range fits i64")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Seq {
        values: &'static [u64],
        i: usize,
    }

    impl Sampler for Seq {
        fn next_u64(&mut self) -> u64 {
            let v = self.values[self.i % self.values.len()];
            self.i += 1;
            v
        }
    }

    struct Step(u64);

    impl Sampler for Step {
        fn next_u64(&mut self) -> u64 {
            let v = self.0;
            self.0 = self.0.wrapping_add(1);
            v
        }
    }

    #[test]
    fn next_below_stays_in_range() {
        let mut s = Step(0);
        for _ in 0..64 {
            assert!(s.next_below(10) < 10);
        }
        assert_eq!(s.next_below(1), 0);
    }

    #[test]
    fn next_below_rejects_out_of_zone() {
        let mut s = Seq {
            values: &[u64::MAX, 3],
            i: 0,
        };
        assert_eq!(s.next_below(10), 3);
    }

    #[test]
    fn next_i64_inclusive_hits_bounds() {
        let mut s = Step(0);
        let x = s.next_i64_inclusive(-2, 2);
        assert!((-2..=2).contains(&x));
        let y = s.next_i64_inclusive(7, 7);
        assert_eq!(y, 7);
    }

    #[test]
    #[should_panic(expected = "n > 0")]
    fn next_below_zero_panics() {
        let mut s = Step(0);
        let _ = s.next_below(0);
    }

    #[test]
    #[should_panic(expected = "low <= high")]
    fn next_i64_inverted_panics() {
        let mut s = Step(0);
        let _ = s.next_i64_inclusive(3, 1);
    }
}
