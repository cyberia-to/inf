//! inf value model — the runtime values inf computes over.
//!
//! values are nox atoms (field / word / hash) plus the structural shapes built
//! from them (bool, bytes, list). there is no floating point — numbers are exact
//! field elements or integers. set semantics come for free from the derived
//! total order: a relation is a `BTreeSet<Tuple>`, deduplicated and sorted.

mod field;
pub use field::{F, P};
pub use neuron_id::NeuronId;

/// A runtime value. The numeric atoms are exact: `Int` for ordinary integers and
/// counts, `Field` for Goldilocks field elements (scores, weights as committed),
/// `Word` for 32-bit-style unsigned values. No floating point exists.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Field(F),
    Word(u64),
    /// 32-byte digest — particle / neuron identity.
    Hash([u8; 32]),
    /// Raw bytes — particle content and text.
    Bytes(Vec<u8>),
    /// Heterogeneous list — nox cons-structure.
    List(Vec<Value>),
}

/// A row: an ordered list of values.
pub type Tuple = Vec<Value>;

/// A relation: a set of tuples. `BTreeSet` gives datalog set semantics
/// (deduplicated) and sorted iteration (needed by semi-naive evaluation).
pub type Relation = std::collections::BTreeSet<Tuple>;

impl Value {
    pub fn int(i: i64) -> Value {
        Value::Int(i)
    }
    pub fn field(x: u64) -> Value {
        Value::Field(F::from_u64(x))
    }
    pub fn hash(h: [u8; 32]) -> Value {
        Value::Hash(h)
    }
    /// Unchanged native subject bytes. The relation's schema supplies domain
    /// meaning; this value establishes neither custody nor authority. Foreign
    /// addresses require explicit domain/address columns, never hashing here.
    pub fn neuron(id: NeuronId) -> Value {
        Value::Hash(id)
    }
    pub fn str(s: &str) -> Value {
        Value::Bytes(s.as_bytes().to_vec())
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// Truthiness for conditions: a non-null, non-false value passes.
    pub fn truthy(&self) -> bool {
        !matches!(self, Value::Null | Value::Bool(false))
    }
}

/// A type-correct, deterministic interface to a particle identity built from a
/// short tag — used by tests and fixtures so `#seed` reads as a stable hash.
pub fn tag_hash(tag: &str) -> [u8; 32] {
    // A small, deterministic, non-cryptographic mapping: enough for fixtures and
    // for the engine, which only ever compares identities for equality/order.
    let mut h = [0u8; 32];
    let b = tag.as_bytes();
    for (i, &c) in b.iter().enumerate() {
        h[i % 32] ^= c.wrapping_add(i as u8);
    }
    h[31] = b.len() as u8;
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_semantics_dedup_and_order() {
        let mut r: Relation = Relation::new();
        r.insert(vec![Value::int(2)]);
        r.insert(vec![Value::int(1)]);
        r.insert(vec![Value::int(2)]); // duplicate
        assert_eq!(r.len(), 2);
        let order: Vec<i64> = r.iter().map(|t| t[0].as_int().unwrap()).collect();
        assert_eq!(order, vec![1, 2]); // sorted
    }

    #[test]
    fn cross_type_total_order_is_stable() {
        // variant order: Null < Bool < Int < Field < Word < Hash < Bytes < List
        let mut v = vec![
            Value::List(vec![Value::int(1)]),
            Value::Int(5),
            Value::Null,
            Value::Bool(true),
            Value::field(7),
        ];
        v.sort();
        assert_eq!(v[0], Value::Null);
        assert_eq!(v[1], Value::Bool(true));
        assert_eq!(v[2], Value::Int(5));
        assert_eq!(v[3], Value::field(7));
        assert!(matches!(v[4], Value::List(_)));
    }

    #[test]
    fn truthiness() {
        assert!(Value::int(0).truthy());
        assert!(Value::Bool(true).truthy());
        assert!(!Value::Bool(false).truthy());
        assert!(!Value::Null.truthy());
    }

    #[test]
    fn tag_hash_is_deterministic_and_distinct() {
        assert_eq!(tag_hash("seed"), tag_hash("seed"));
        assert_ne!(tag_hash("seed"), tag_hash("goal"));
    }
}
