//! The Goldilocks field F_p, p = 2^64 − 2^32 + 1. The canonical representation
//! is a `u64` in `[0, P)`, so the derived `Ord` is a stable total order on field
//! elements (their canonical residues). Arithmetic is reference-correct (reduces
//! through `u128`), not micro-optimized — the proof path owns performance.

/// The Goldilocks prime.
pub const P: u64 = 0xFFFF_FFFF_0000_0001;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct F(u64);

impl F {
    pub fn from_u64(x: u64) -> F {
        F((x as u128 % P as u128) as u64)
    }
    pub fn val(self) -> u64 {
        self.0
    }
    pub fn zero() -> F {
        F(0)
    }
    pub fn one() -> F {
        F(1)
    }
    pub fn add(self, o: F) -> F {
        F(((self.0 as u128 + o.0 as u128) % P as u128) as u64)
    }
    pub fn sub(self, o: F) -> F {
        F(((self.0 as u128 + P as u128 - o.0 as u128) % P as u128) as u64)
    }
    pub fn mul(self, o: F) -> F {
        F(((self.0 as u128 * o.0 as u128) % P as u128) as u64)
    }
    pub fn neg(self) -> F {
        if self.0 == 0 {
            F(0)
        } else {
            F(P - self.0)
        }
    }
    pub fn pow(self, mut e: u64) -> F {
        let mut base = self;
        let mut acc = F::one();
        while e > 0 {
            if e & 1 == 1 {
                acc = acc.mul(base);
            }
            base = base.mul(base);
            e >>= 1;
        }
        acc
    }
    /// Multiplicative inverse via Fermat (a^(p-2)). `None` for zero.
    pub fn inv(self) -> Option<F> {
        if self.0 == 0 {
            None
        } else {
            Some(self.pow(P - 2))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reduction_is_canonical() {
        assert_eq!(F::from_u64(P).val(), 0);
        assert_eq!(F::from_u64(P + 5).val(), 5);
    }

    #[test]
    fn add_sub_roundtrip() {
        let a = F::from_u64(123456789);
        let b = F::from_u64(987654321);
        assert_eq!(a.add(b).sub(b), a);
        assert_eq!(a.add(a.neg()), F::zero());
    }

    #[test]
    fn mul_is_commutative_and_associative() {
        let a = F::from_u64(7);
        let b = F::from_u64(P - 3);
        let c = F::from_u64(1 << 40);
        assert_eq!(a.mul(b), b.mul(a));
        assert_eq!(a.mul(b).mul(c), a.mul(b.mul(c)));
    }

    #[test]
    fn inverse_is_correct() {
        for x in [1u64, 2, 3, 7, P - 1, 1 << 32, 0xDEAD_BEEF] {
            let f = F::from_u64(x);
            let inv = f.inv().unwrap();
            assert_eq!(f.mul(inv), F::one(), "inv failed for {x}");
        }
        assert_eq!(F::zero().inv(), None);
    }

    #[test]
    fn order_matches_canonical_residue() {
        assert!(F::from_u64(3) < F::from_u64(4));
        assert!(F::from_u64(0) < F::from_u64(P - 1));
    }
}
