//! The relation source boundary (specs/relations.md).
//!
//! inf's evaluator resolves every base relation through `RelationSource`, so the
//! same engine runs over local, bbg, or cybergraph data unchanged. `LocalSource`
//! is the in-memory implementation used by the bootstrap and the test corpus.
//!
//! `GraphStats` mirrors bbg/specs/statistics.md: the committed graph statistics
//! folded into the BBG root that inf uses for static cost bounds and recursion
//! depth. `LocalSource` computes approximate stats from the local data for
//! bootstrap / testing; the real stats come from the BBG-committed polynomial.

use inf_value::{Relation, Tuple, Value};
use std::collections::HashMap;

#[cfg(feature = "bbg")]
pub mod bbg;
#[cfg(feature = "bbg")]
pub use bbg::BbgSource;

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

// ── committed graph statistics (bbg/specs/statistics.md) ─────────────────────

/// Index of each canonical relation in `relation_sizes` (bbg/specs/statistics.md).
pub mod rel_idx {
    pub const PARTICLES:   usize = 0;
    pub const AXONS_OUT:   usize = 1;
    pub const AXONS_IN:    usize = 2;
    pub const NEURONS:     usize = 3;
    pub const LOCATIONS:   usize = 4;
    pub const COINS:       usize = 5;
    pub const CARDS:       usize = 6;
    pub const FILES:       usize = 7;
    pub const TIME:        usize = 8;
    pub const SIGNALS:     usize = 9;
    pub const BALANCES:    usize = 10;
}

/// The committed graph statistics that `inf cost` uses for static bounds
/// (specs/cost.md, bbg/specs/statistics.md). Committed via `stats_commit =
/// H(...)` folded into BBG_root. `diameter_bound` defaults to node_count−1;
/// `set_diameter_bound` (via tru) can install a tighter proven value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphStats {
    /// Total particle count.
    pub node_count: u64,
    /// Row counts for each canonical relation (11 relations, canonical order).
    pub relation_sizes: [u64; 11],
    /// Maximum out-degree across all particles.
    pub max_degree: u64,
    /// Upper bound on the graph diameter; used as the default recursion depth
    /// bound. Proven tighter by tru via `set_diameter_bound`.
    pub diameter_bound: u64,
}

impl Default for GraphStats {
    fn default() -> Self {
        GraphStats {
            node_count: 0,
            relation_sizes: [0; 11],
            max_degree: 0,
            diameter_bound: 0,
        }
    }
}

impl GraphStats {
    /// Maximum recursion rounds permitted for a query over this graph snapshot.
    /// Defaults to `diameter_bound`; explicit `:bounded N` caps it lower.
    pub fn max_rounds(&self) -> u64 {
        self.diameter_bound.max(1)
    }

    /// Estimated read cost for a full scan of relation `rel_idx`.
    pub fn scan_cost(&self, rel: usize) -> u64 {
        self.relation_sizes.get(rel).copied().unwrap_or(self.node_count)
    }
}

/// Compute approximate GraphStats from a LocalSource by inspecting the
/// cyberlink/axon/neuron relations when present. Used for bootstrap testing
/// before real BBG integration (R3).
///
/// Heuristic: if a `cyberlinks` or `axons` relation exists, count its rows as
/// the link count and find the highest out-degree. Otherwise use the largest
/// relation's row count as node_count.
pub fn compute_stats(src: &LocalSource) -> GraphStats {
    let mut stats = GraphStats::default();

    // node_count from particles/neurons/cyberlinks
    if src.schema("particles").is_some() {
        stats.node_count = src.scan("particles").count() as u64;
        stats.relation_sizes[rel_idx::PARTICLES] = stats.node_count;
    } else if src.schema("neurons").is_some() {
        stats.node_count = src.scan("neurons").count() as u64;
        stats.relation_sizes[rel_idx::NEURONS] = stats.node_count;
    } else {
        // fall back: largest relation size
        stats.node_count = src.all_relations().map(|r| src.scan(r).count() as u64).max().unwrap_or(0);
    }

    // axon counts and max degree
    let link_rel = if src.schema("axons").is_some() { Some("axons") }
                   else if src.schema("cyberlinks").is_some() { Some("cyberlinks") }
                   else { None };

    if let Some(rel) = link_rel {
        let rows: Vec<_> = src.scan(rel).collect();
        stats.relation_sizes[rel_idx::AXONS_OUT] = rows.len() as u64;
        stats.relation_sizes[rel_idx::AXONS_IN]  = rows.len() as u64;

        // count out-degree: first column = from node (as Value)
        let mut degree: HashMap<Vec<u8>, u64> = HashMap::new();
        for row in &rows {
            if let Some(from) = row.first() {
                let key = value_key(from);
                *degree.entry(key).or_insert(0) += 1;
            }
        }
        stats.max_degree = degree.values().copied().max().unwrap_or(0);
    }

    // diameter_bound = node_count − 1 (conservative; tru can tighten)
    stats.diameter_bound = stats.node_count.saturating_sub(1).max(1);

    stats
}

fn value_key(v: &Value) -> Vec<u8> {
    match v {
        Value::Int(i) => i.to_le_bytes().to_vec(),
        Value::Field(f) => f.val().to_le_bytes().to_vec(),
        Value::Word(w) => w.to_le_bytes().to_vec(),
        Value::Hash(h) => h.to_vec(),
        Value::Bool(b) => vec![*b as u8],
        Value::Bytes(b) => b.clone(),
        Value::Null => vec![],
        Value::List(_) => vec![0xff],
    }
}

// ── relation source trait ─────────────────────────────────────────────────────

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
    /// Committed graph statistics for the cost model (specs/cost.md).
    /// Returns `None` for local sources that haven't computed stats yet.
    fn graph_stats(&self) -> Option<GraphStats> {
        None
    }
}

/// In-memory relations. Used as the bootstrap "local" source and by tests.
#[derive(Default)]
pub struct LocalSource {
    rels: HashMap<String, (Schema, Relation)>,
    snapshot: Option<u64>,
    /// Cached committed stats (set explicitly or computed lazily).
    stats: Option<GraphStats>,
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

    /// Names of all relations in this source.
    pub fn all_relations(&self) -> impl Iterator<Item = &str> {
        self.rels.keys().map(|s| s.as_str())
    }

    /// Compute (and cache) GraphStats from the local relations.
    /// Re-use cached value if already computed.
    pub fn ensure_stats(&mut self) -> &GraphStats {
        if self.stats.is_none() {
            // We can't call compute_stats(self) because it borrows self.
            // Re-implement inline to avoid the borrow issue.
            let mut stats = GraphStats::default();

            // node_count from the largest recognisable entity relation
            let particle_count = self.rels.get("particles").map(|(_, r)| r.len() as u64);
            let neuron_count   = self.rels.get("neurons").map(|(_, r)| r.len() as u64);
            stats.node_count = particle_count
                .or(neuron_count)
                .unwrap_or_else(|| self.rels.values().map(|(_, r)| r.len() as u64).max().unwrap_or(0));

            if let Some((_, r)) = self.rels.get("particles") {
                stats.relation_sizes[rel_idx::PARTICLES] = r.len() as u64;
            }
            if let Some((_, r)) = self.rels.get("neurons") {
                stats.relation_sizes[rel_idx::NEURONS] = r.len() as u64;
            }

            // axon/cyberlink counts + max out-degree
            let link_rows: Option<Vec<_>> = self.rels.get("axons")
                .or_else(|| self.rels.get("cyberlinks"))
                .map(|(_, r)| r.iter().cloned().collect());

            if let Some(rows) = link_rows {
                stats.relation_sizes[rel_idx::AXONS_OUT] = rows.len() as u64;
                stats.relation_sizes[rel_idx::AXONS_IN]  = rows.len() as u64;
                let mut degree: HashMap<Vec<u8>, u64> = HashMap::new();
                for row in &rows {
                    if let Some(from) = row.first() {
                        *degree.entry(value_key(from)).or_insert(0) += 1;
                    }
                }
                stats.max_degree = degree.values().copied().max().unwrap_or(0);
            }

            stats.diameter_bound = stats.node_count.saturating_sub(1).max(1);
            self.stats = Some(stats);
        }
        self.stats.as_ref().unwrap()
    }

    /// Set explicit committed stats (e.g., read from a BBG root).
    pub fn with_stats(mut self, stats: GraphStats) -> Self {
        self.stats = Some(stats);
        self
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
    fn graph_stats(&self) -> Option<GraphStats> {
        self.stats.clone()
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
        assert_eq!(rows[0][0], Value::str("a"));
        assert!(!s.provable());
        assert!(!s.writable("focus"));
        assert!(s.scan("missing").next().is_none());
    }

    // ── R3: GraphStats tests ──────────────────────────────────────────────────

    #[test]
    fn graph_stats_computes_from_local_data() {
        let mut s = LocalSource::new();
        s.add("particles", &["id"], vec![
            vec![Value::int(1)],
            vec![Value::int(2)],
            vec![Value::int(3)],
        ]);
        s.add("axons", &["from", "to"], vec![
            vec![Value::int(1), Value::int(2)],
            vec![Value::int(1), Value::int(3)],
            vec![Value::int(2), Value::int(3)],
        ]);
        let stats = s.ensure_stats().clone();
        assert_eq!(stats.node_count, 3, "particle count");
        assert_eq!(stats.relation_sizes[rel_idx::PARTICLES], 3);
        assert_eq!(stats.relation_sizes[rel_idx::AXONS_OUT], 3);
        assert_eq!(stats.max_degree, 2, "node 1 has 2 out-edges");
        assert_eq!(stats.diameter_bound, 2, "node_count - 1 = 2");
    }

    #[test]
    fn graph_stats_max_rounds_is_diameter() {
        let stats = GraphStats { node_count: 100, diameter_bound: 99, ..Default::default() };
        assert_eq!(stats.max_rounds(), 99);
    }

    #[test]
    fn graph_stats_default_is_zero() {
        let s = GraphStats::default();
        assert_eq!(s.node_count, 0);
        assert_eq!(s.diameter_bound, 0);
        assert_eq!(s.max_rounds(), 1, "clamps to 1 minimum");
    }

    #[test]
    fn local_source_graph_stats_none_before_compute() {
        let s = LocalSource::new();
        assert_eq!(s.graph_stats(), None, "no stats before ensure_stats");
    }

    #[test]
    fn local_source_graph_stats_some_after_compute() {
        let mut s = LocalSource::new();
        s.add("particles", &["id"], vec![vec![Value::int(1)]]);
        s.ensure_stats();
        assert!(s.graph_stats().is_some());
    }

    #[test]
    fn with_stats_overrides_computed() {
        let custom = GraphStats {
            node_count: 999,
            diameter_bound: 42,
            ..Default::default()
        };
        let s = LocalSource::new().with_stats(custom.clone());
        assert_eq!(s.graph_stats(), Some(custom));
    }

    #[test]
    fn compute_stats_without_canonical_rels_falls_back_to_largest() {
        let mut s = LocalSource::new();
        s.add("edge", &["from", "to"], vec![
            vec![Value::int(1), Value::int(2)],
            vec![Value::int(3), Value::int(4)],
        ]);
        let stats = s.ensure_stats().clone();
        // no particles/neurons: falls back to largest relation (2 rows)
        assert_eq!(stats.node_count, 2);
    }
}
