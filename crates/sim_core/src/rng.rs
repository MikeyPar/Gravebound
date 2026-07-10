use rand_chacha::ChaCha8Rng;
use rand_core::{Rng, SeedableRng};
use thiserror::Error;

const DOMAIN_SEPARATOR: &[u8] = b"gravebound-rng-v1\0";

/// Errors returned by deterministic random helpers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum RngError {
    /// A bounded draw requires a nonzero exclusive upper bound.
    #[error("random upper bound must be greater than zero")]
    ZeroUpperBound,
}

/// Derives an isolated 256-bit stream seed from the release, root seed, and stream label.
///
/// Length prefixes make the encoding unambiguous. Adding a new consumer must use a new named
/// stream instead of changing the draw order of an existing stream.
#[must_use]
pub fn derive_stream_seed(content_version: &str, root_seed: u64, stream_label: &str) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(DOMAIN_SEPARATOR);
    update_length_prefixed(&mut hasher, content_version.as_bytes());
    hasher.update(&root_seed.to_le_bytes());
    update_length_prefixed(&mut hasher, stream_label.as_bytes());
    *hasher.finalize().as_bytes()
}

fn update_length_prefixed(hasher: &mut blake3::Hasher, value: &[u8]) {
    let length = u32::try_from(value.len()).expect("RNG labels and versions must fit in u32");
    hasher.update(&length.to_le_bytes());
    hasher.update(value);
}

/// Deterministic `ChaCha8` stream with platform-independent bounded integer sampling.
#[derive(Debug, Clone)]
pub struct DeterministicRng {
    inner: ChaCha8Rng,
}

impl DeterministicRng {
    /// Constructs a named stream from its canonical seed inputs.
    #[must_use]
    pub fn new(content_version: &str, root_seed: u64, stream_label: &str) -> Self {
        Self {
            inner: ChaCha8Rng::from_seed(derive_stream_seed(
                content_version,
                root_seed,
                stream_label,
            )),
        }
    }

    /// Returns the next raw value. Raw draws are acceptable for hashing and fixture probes.
    pub fn next_u64(&mut self) -> u64 {
        self.inner.next_u64()
    }

    /// Samples uniformly from `0..upper_exclusive` using rejection rather than modulo bias.
    pub fn bounded_u64(&mut self, upper_exclusive: u64) -> Result<u64, RngError> {
        if upper_exclusive == 0 {
            return Err(RngError::ZeroUpperBound);
        }

        let rejection_zone = upper_exclusive.wrapping_neg() % upper_exclusive;
        loop {
            let value = self.next_u64();
            if value >= rejection_zone {
                return Ok(value % upper_exclusive);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_streams_replay_exactly_and_are_isolated() {
        let mut first = DeterministicRng::new("fp.1.0.0", 42, "fixture");
        let mut second = DeterministicRng::new("fp.1.0.0", 42, "fixture");
        let mut other = DeterministicRng::new("fp.1.0.0", 42, "loot");

        assert_eq!(first.next_u64(), second.next_u64());
        assert_ne!(first.next_u64(), other.next_u64());
    }

    #[test]
    fn bounded_draw_rejects_zero_and_stays_in_range() {
        let mut rng = DeterministicRng::new("fp.1.0.0", 7, "fixture");
        assert_eq!(rng.bounded_u64(0), Err(RngError::ZeroUpperBound));
        for _ in 0..1_000 {
            assert!(rng.bounded_u64(17).expect("valid bound") < 17);
        }
    }
}
