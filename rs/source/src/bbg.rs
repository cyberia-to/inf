//! `BbgSource` — a `RelationSource` over real committed BBG state (R3b).
//!
//! This is the "scan door" of the bbg integration (see
//! `inf/.claude/plans/bbg-integration.md` and `bbg/roadmap/provable-reads.md`):
//! it reads the canonical dimensions as full-column relations straight out of the
//! committed `BbgState`, and reports the committed `GraphStats`. The reads are
//! sound *by construction* (they are the committed snapshot), but they are NOT
//! individually proven — `provable()` stays `false` until bbg's look value
//! semantics are fixed (the "proof door", tracked in `bbg/roadmap/provable-reads.md`).
//!
//! Relation names map 1:1 to bbg dimensions. `axons_out`/`axons_in` are flattened
//! from adjacency lists into edge tuples; `axons` is an alias for `axons_out`.
//! `balances` is read from the in-memory map for completeness but is a private
//! dimension — neuron-scoping is deferred (a later R3b sub-step).

use bbg::state::BbgState;
use inf_value::{Tuple, Value};

use crate::{GraphStats, RelationSource, Schema};

/// A `RelationSource` backed by a committed `BbgState` snapshot.
pub struct BbgSource<'a> {
    pub state: &'a BbgState,
}

impl<'a> BbgSource<'a> {
    pub fn new(state: &'a BbgState) -> Self {
        BbgSource { state }
    }
}

/// Canonical column names per dimension. The first column is always the key.
fn dim_schema(rel: &str) -> Option<Schema> {
    let cols: &[&str] = match rel {
        "particles" => &["id", "energy", "pi_star", "weight", "s_yes", "s_no", "meta_score"],
        "axons_out" | "axons" => &["from", "to"],
        "axons_in" => &["to", "from"],
        "neurons" => &["id", "focus", "karma", "stake"],
        "locations" => &["id", "lat", "lon"],
        "coins" => &["denom", "total_supply"],
        "cards" => &["card", "owner", "particle"],
        "files" => &["particle", "available", "chunk_count"],
        "time" => &["height", "root"],
        "signals" => &["step", "neuron", "link_count", "block_height", "proof_hash"],
        "balances" => &["key", "amount"],
        _ => return None,
    };
    Some(Schema::new(cols))
}

/// u64 record fields become `Value::Word`; 32-byte ids become `Value::Hash`.
#[inline]
fn w(v: u64) -> Value {
    Value::Word(v)
}

impl<'a> RelationSource for BbgSource<'a> {
    fn schema(&self, rel: &str) -> Option<Schema> {
        dim_schema(rel)
    }

    fn scan<'b>(&'b self, rel: &str) -> Box<dyn Iterator<Item = Tuple> + 'b> {
        let s = self.state;
        match rel {
            "particles" => Box::new(s.particles.iter().map(|(k, v)| {
                vec![
                    Value::hash(*k),
                    w(v.energy),
                    w(v.pi_star),
                    w(v.weight),
                    w(v.s_yes),
                    w(v.s_no),
                    w(v.meta_score),
                ]
            })),
            "axons_out" | "axons" => Box::new(
                s.axons_out
                    .iter()
                    .flat_map(|(k, tos)| tos.iter().map(move |to| vec![Value::hash(*k), Value::hash(*to)])),
            ),
            "axons_in" => Box::new(
                s.axons_in
                    .iter()
                    .flat_map(|(k, froms)| froms.iter().map(move |f| vec![Value::hash(*k), Value::hash(*f)])),
            ),
            "neurons" => Box::new(
                s.neurons
                    .iter()
                    .map(|(k, v)| vec![Value::hash(*k), w(v.focus), w(v.karma), w(v.stake)]),
            ),
            "locations" => Box::new(s.locations.iter().map(|(k, v)| {
                vec![Value::hash(*k), Value::int(v.lat as i64), Value::int(v.lon as i64)]
            })),
            "coins" => Box::new(
                s.coins.iter().map(|(k, v)| vec![Value::hash(*k), w(v.total_supply)]),
            ),
            "cards" => Box::new(s.cards.iter().map(|(k, v)| {
                vec![Value::hash(*k), Value::hash(v.owner), Value::hash(v.particle)]
            })),
            "files" => Box::new(s.files.iter().map(|(k, v)| {
                vec![Value::hash(*k), Value::Bool(v.available), w(v.chunk_count as u64)]
            })),
            "time" => Box::new(s.time.iter().map(|(h, root)| vec![w(*h), Value::hash(*root)])),
            "signals" => Box::new(s.signals.iter().map(|(step, v)| {
                vec![
                    w(*step),
                    Value::hash(v.neuron),
                    w(v.link_count as u64),
                    w(v.block_height),
                    Value::hash(v.proof_hash),
                ]
            })),
            "balances" => Box::new(s.balances.iter().map(|(k, v)| vec![Value::hash(*k), w(*v)])),
            _ => Box::new(std::iter::empty()),
        }
    }

    fn snapshot(&self) -> Option<u64> {
        Some(self.state.height)
    }

    /// `false` for now: the data is committed, but per-read proofs are not yet
    /// sound (the bbg look pattern opens an MLE fingerprint, not the record
    /// cell — see `bbg/roadmap/provable-reads.md`). Flip to `true` once the proof
    /// door is fixed and R2b lands.
    fn provable(&self) -> bool {
        false
    }

    fn graph_stats(&self) -> Option<GraphStats> {
        let s = self.state.statistics();
        Some(GraphStats {
            node_count: s.node_count,
            relation_sizes: s.relation_sizes,
            max_degree: s.max_degree,
            diameter_bound: s.diameter_bound,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bbg::signal::{Cyberlink, Signal};
    use bbg::types::NeuronRecord;

    fn particle(seed: u8) -> [u8; 32] {
        [seed; 32]
    }

    /// A small committed state: one neuron, one cyberlink (→ axons + particles),
    /// one time snapshot.
    fn seeded_state() -> BbgState {
        let mut s = BbgState::new();
        s.neurons.insert(particle(1), NeuronRecord { focus: 50_000, karma: 7, stake: 0 });
        let sig = Signal {
            neuron: particle(1),
            links: vec![Cyberlink {
                from: particle(2),
                to: particle(3),
                token: particle(0),
                amount: 100,
                valence: 1,
            }],
            box_moves: vec![],
            height: 0,
        };
        s.insert(&sig).unwrap();
        s.time.insert(0, particle(99));
        s
    }

    #[test]
    fn schema_maps_dimensions() {
        let st = BbgState::new();
        let src = BbgSource::new(&st);
        assert_eq!(src.schema("neurons").unwrap().columns, vec!["id", "focus", "karma", "stake"]);
        assert_eq!(src.schema("axons").unwrap().columns, vec!["from", "to"]);
        assert!(src.schema("nonsense").is_none());
    }

    #[test]
    fn scan_neurons_returns_committed_rows() {
        let st = seeded_state();
        let src = BbgSource::new(&st);
        let rows: Vec<Tuple> = src.scan("neurons").collect();
        assert_eq!(rows.len(), 1);
        // assert against the committed record, not a literal: inserting the signal
        // debits focus by the staked amount, so focus is 49_900, not 50_000.
        let rec = st.neurons.get(&particle(1)).unwrap();
        assert_eq!(rows[0][0], Value::hash(particle(1)));
        assert_eq!(rows[0][1], Value::Word(rec.focus));
        assert_eq!(rows[0][2], Value::Word(rec.karma));
        assert_eq!(rows[0][3], Value::Word(rec.stake));
    }

    #[test]
    fn scan_axons_flattens_adjacency_into_edges() {
        let st = seeded_state();
        let src = BbgSource::new(&st);
        let out: Vec<Tuple> = src.scan("axons_out").collect();
        // the cyberlink 2→3 produces an out-edge from the axon particle to its target
        assert!(!out.is_empty(), "axons_out should expose committed edges");
        for row in &out {
            assert_eq!(row.len(), 2, "edge tuple is (from, to)");
            assert!(matches!(row[0], Value::Hash(_)));
            assert!(matches!(row[1], Value::Hash(_)));
        }
        // `axons` is an alias for `axons_out`
        let alias: Vec<Tuple> = src.scan("axons").collect();
        assert_eq!(alias.len(), out.len());
    }

    #[test]
    fn scan_time_exposes_height_and_root() {
        let st = seeded_state();
        let src = BbgSource::new(&st);
        let rows: Vec<Tuple> = src.scan("time").collect();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0][0], Value::Word(0)); // height
        assert_eq!(rows[0][1], Value::hash(particle(99))); // root snapshot
    }

    #[test]
    fn graph_stats_come_from_committed_statistics() {
        let st = seeded_state();
        let src = BbgSource::new(&st);
        let stats = src.graph_stats().unwrap();
        let committed = st.statistics();
        assert_eq!(stats.node_count, committed.node_count);
        assert_eq!(stats.relation_sizes, committed.relation_sizes);
        assert_eq!(stats.max_degree, committed.max_degree);
        assert_eq!(stats.diameter_bound, committed.diameter_bound);
    }

    #[test]
    fn snapshot_is_state_height_and_reads_are_not_yet_provable() {
        let st = seeded_state();
        let src = BbgSource::new(&st);
        assert_eq!(src.snapshot(), Some(st.height));
        // honest until the proof door is fixed (bbg/roadmap/provable-reads.md)
        assert!(!src.provable());
    }

    #[test]
    fn unknown_relation_scans_empty() {
        let st = seeded_state();
        let src = BbgSource::new(&st);
        assert!(src.scan("does_not_exist").next().is_none());
    }
}
