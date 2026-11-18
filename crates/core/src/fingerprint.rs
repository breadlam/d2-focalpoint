//! Stable 63-bit content fingerprints.
//!
//! Ids derived here are persisted (`build_id`, `loadout_id`), so the encoding
//! must never change silently: any change to what goes into a fingerprint must
//! bump the relevant version constant, which is recorded in the database
//! metadata. xxh3-64 is a fixed, platform-independent algorithm.

use xxhash_rust::xxh3::Xxh3;

/// Incrementally builds a fingerprint from little-endian primitive fields.
#[derive(Default)]
pub struct Fingerprinter(Xxh3);

impl std::fmt::Debug for Fingerprinter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Fingerprinter").finish_non_exhaustive()
    }
}

impl Fingerprinter {
    /// Start a fingerprint for a given encoding `version` and `domain` tag, so
    /// fingerprints of different things never collide by construction.
    #[must_use]
    pub fn new(domain: &[u8], version: u8) -> Self {
        let mut h = Xxh3::new();
        h.update(domain);
        h.update(&[0, version]);
        Self(h)
    }

    pub fn u8(&mut self, v: u8) -> &mut Self {
        self.0.update(&[v]);
        self
    }

    pub fn u16(&mut self, v: u16) -> &mut Self {
        self.0.update(&v.to_le_bytes());
        self
    }

    pub fn u32(&mut self, v: u32) -> &mut Self {
        self.0.update(&v.to_le_bytes());
        self
    }

    pub fn u64(&mut self, v: u64) -> &mut Self {
        self.0.update(&v.to_le_bytes());
        self
    }

    /// Finish as a non-negative `i64` (top bit dropped) so ids are positive
    /// `BIGINT`s in SQL.
    #[must_use]
    pub fn finish(&self) -> i64 {
        (self.0.digest() >> 1) as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_domain_separated() {
        let a = Fingerprinter::new(b"build", 1).u32(7).finish();
        let b = Fingerprinter::new(b"build", 1).u32(7).finish();
        let c = Fingerprinter::new(b"loadout", 1).u32(7).finish();
        let d = Fingerprinter::new(b"build", 2).u32(7).finish();
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
        assert!(a >= 0);
    }

    #[test]
    fn known_value_is_stable() {
        // Guards against accidental encoding changes: if this fails, every
        // persisted id would change. Bump a version instead.
        let v = Fingerprinter::new(b"build", 1)
            .u8(1)
            .u16(2)
            .u32(3)
            .u64(4)
            .finish();
        assert_eq!(v, 4_056_983_643_344_893_724);
    }
}
