//! App-wide stable hashing.
//!
//! `StableHasher` is SipHash-1-3 with fixed keys (0, 0) -- the same algorithm
//! `std`'s `DefaultHasher` uses, but the `siphasher` crate guarantees the
//! algorithm stays fixed across toolchain upgrades, which `std` does not. Use
//! this anywhere a hash value is user-visible or must reproduce across relux
//! versions (the `mnemonic`/`sha1` BIFs, cause/warning ids, `__RELUX_TEST_ID`,
//! effect-instance identity).

use std::hash::Hash;
use std::hash::Hasher;

use siphasher::sip::SipHasher13;

/// Stable, cross-version 64-bit hasher. Fixed-key SipHash-1-3.
pub struct StableHasher(SipHasher13);

impl StableHasher {
    pub fn new() -> Self {
        Self(SipHasher13::new())
    }
}

impl Default for StableHasher {
    fn default() -> Self {
        Self::new()
    }
}

impl Hasher for StableHasher {
    fn finish(&self) -> u64 {
        self.0.finish()
    }

    fn write(&mut self, bytes: &[u8]) {
        self.0.write(bytes);
    }
}

/// Stable 64-bit hash of any `Hash` value. Same input always yields the same
/// output, across runs and across relux versions.
pub fn stable_hash<T: Hash>(value: &T) -> u64 {
    let mut hasher = StableHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_hash_is_deterministic() {
        assert_eq!(stable_hash(&"empay"), stable_hash(&"empay"));
        assert_eq!(stable_hash(&"empay"), stable_hash(&"empay"));
    }

    #[test]
    fn stable_hash_distinguishes_inputs() {
        assert_ne!(stable_hash(&"alpha"), stable_hash(&"beta"));
    }

    #[test]
    fn empty_string_hashes_without_panic() {
        let _ = stable_hash(&"");
    }
}
