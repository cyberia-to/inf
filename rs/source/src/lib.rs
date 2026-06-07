//! The relation source boundary (specs/relations.md).
//!
//! inf's evaluator resolves every base relation through `RelationSource`, so the
//! same engine runs over local, bbg, or cybergraph data unchanged. `LocalSource`
//! is the in-memory implementation used by the bootstrap and the test corpus.

use inf_value::{Relation, Tuple};
use std::collections::HashMap;

/// A relation's column names, in order.
#[derive(Clone, Debug, PartialEq)]
pub struct Schema {
    pub columns: Vec<String>,
}

impl Schema {
    pub fn new(cols: &[&str]) -> Schema {
        Schema { columns: cols.iter().map(|c| c.to_string()).collect() }
    }
    /// Index of a named column.
    pub fn col(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c == name)
    }
    pub fn arity(&self) -> usize {
        self.columns.len()
    }
}

/// The boundary inf reads base relations through.
pub trait RelationSource {
    fn schema(&self, rel: &str) -> Option<Schema>;
    /// All rows of a relation, in sorted order.
    fn scan<'a>(&'a self, rel: &str) -> Box<dyn Iterator<Item = Tuple> + 'a>;
    /// Block height of the committed snapshot; `None` for a local source.
    fn snapshot(&self) -> Option<u64>;
    /// Whether reads can become Lens openings (committed sources only).
    fn provable(&self) -> bool;
    /// Canonical relations are never writable through inf; temp relations are.
    fn writable(&self, _rel: &str) -> bool {
        false
    }
}

/// In-memory relations. Used as the bootstrap "local" source and by tests.
#[derive(Default)]
pub struct LocalSource {
    rels: HashMap<String, (Schema, Relation)>,
    snapshot: Option<u64>,
}

impl LocalSource {
    pub fn new() -> LocalSource {
        LocalSource::default()
    }

    /// Define a relation with named columns and rows. Rows are deduplicated and
    /// sorted by the `Relation` set.
    pub fn add(&mut self, name: &str, columns: &[&str], rows: Vec<Tuple>) {
        let schema = Schema::new(columns);
        let mut rel = Relation::new();
        for r in rows {
            assert_eq!(r.len(), schema.arity(), "row arity mismatch in relation {name}");
            rel.insert(r);
        }
        self.rels.insert(name.to_string(), (schema, rel));
    }

    pub fn with_snapshot(mut self, h: u64) -> LocalSource {
        self.snapshot = Some(h);
        self
    }

    pub fn has(&self, rel: &str) -> bool {
        self.rels.contains_key(rel)
    }

    /// Append a tuple to a relation (used by the reactive driver to apply a
    /// mutation event). The relation must already exist.
    pub fn insert(&mut self, rel: &str, tuple: Tuple) -> Result<(), String> {
        match self.rels.get_mut(rel) {
            Some((schema, r)) => {
                if tuple.len() != schema.arity() {
                    return Err(format!("insert into `{rel}`: arity mismatch"));
                }
                r.insert(tuple);
                Ok(())
            }
            None => Err(format!("insert into unknown relation `{rel}`")),
        }
    }
}

impl RelationSource for LocalSource {
    fn schema(&self, rel: &str) -> Option<Schema> {
        self.rels.get(rel).map(|(s, _)| s.clone())
    }
    fn scan<'a>(&'a self, rel: &str) -> Box<dyn Iterator<Item = Tuple> + 'a> {
        match self.rels.get(rel) {
            Some((_, r)) => Box::new(r.iter().cloned()),
            None => Box::new(std::iter::empty()),
        }
    }
    fn snapshot(&self) -> Option<u64> {
        self.snapshot
    }
    fn provable(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inf_value::Value;

    #[test]
    fn local_source_scan_and_schema() {
        let mut s = LocalSource::new();
        s.add(
            "focus",
            &["particle", "score"],
            vec![
                vec![Value::str("a"), Value::int(10)],
                vec![Value::str("b"), Value::int(3)],
            ],
        );
        assert_eq!(s.schema("focus").unwrap().col("score"), Some(1));
        let rows: Vec<Tuple> = s.scan("focus").collect();
        assert_eq!(rows.len(), 2);
        // sorted: "a" before "b"
        assert_eq!(rows[0][0], Value::str("a"));
        assert!(!s.provable());
        assert!(!s.writable("focus"));
        assert!(s.scan("missing").next().is_none());
    }
}
