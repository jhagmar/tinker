//! SplitMix64 sampler for catalog generate (host adapter).

use tinker_catalog::Sampler;

/// Deterministic SplitMix64 stream. Same seed → same sequence.
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    /// Construct from a seed.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }
}

impl Sampler for SplitMix64 {
    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// Default seed for CLI `verify` (fixed so a run is repeatable).
pub const VERIFY_SEED: u64 = 0x9E37_79B9_7F4A_7C15;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequence_is_deterministic_and_changes() {
        let mut a = SplitMix64::new(VERIFY_SEED);
        let mut b = SplitMix64::new(VERIFY_SEED);
        let x = a.next_u64();
        let y = a.next_u64();
        assert_ne!(x, y);
        assert_eq!(x, b.next_u64());
        assert_eq!(y, b.next_u64());
        assert!(a.next_below(7) < 7);
    }
}
