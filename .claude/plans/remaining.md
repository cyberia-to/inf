# inf — remaining work

status: 2026-06-07. the core language is built and verified in inf/rs (9 crates,
50 tests, 0 warnings): parse → plan → eval with recursion, aggregation,
negation, mutation derivation, fixed-rule algorithms, a CLI, and a stage-3
expression lowering onto the real nox VM. this plan is everything still missing
against the specs. compliance map: inf/rs/README.md.

ordering: do the quick-wins + oracle first (they harden what exists), then the
headline relational lowering, then proofs; the source/extension/tail tracks can
run in parallel.

## R0 — quick wins and the oracle (harden what exists)

small, high-value, no new external deps.

- cozo differential-oracle harness. wire the vendored rs/cozo as a dev-dep behind
  a `cozo-oracle` feature; run the eval corpus through both cozo and inf-eval and
  assert equality on the integer/bounded subset. closes the plan's M0 promise and
  pins eval's correctness to a battle-tested engine.
- `pub input/output` decls. parse the declaration grammar (grammar.md) into the
  AST; currently skipped. lets a program state its I/O shape.
- temp relations. `:put`/`:rm` into an in-session temp store (currently the
  evaluator only derives the mutation batch); add `_`-prefixed relation writes
  visible to later rules in the same script.
- `cost` completeness (local part). report per-operator breakdown; the committed
  GraphStats part waits on R3 (bbg).

verify: cozo == inf-eval over the corpus; decl + temp tests.
estimate: ~2 sessions.

## R1 — relational nox lowering (stage 3 complete)

the headline. extend inf-lower from expressions to whole queries, executed on the
real nox VM and differential-tested against inf-eval.

- noun encoding. design the nox representation of a tuple (cons of field/hash
  atoms; Hash = 4 field elements) and a relation (cons-list of tuples). document
  it; it is the contract for reads and proofs.
- relation reads → `look` (pattern 17). a LookProvider over the RelationSource:
  resolve (root, namespace=rel, key) to tuple data. decide the multi-field scheme
  (look per column, or a tuple-id indirection) since `look` returns one element.
- joins → `compose`/`axis` over a binding noun; filters/conditions → the existing
  expression lowering under `branch`; project → `cons`.
- bounded recursion → a bounded `compose` loop (the `for _ in 0..MAX` shape);
  aggregation, sort, limit as folds/permutation.
- differential test: nox execution == inf-eval on the full corpus (discovery,
  reachability, aggregation, negation, fixed rules).

verify: every corpus query matches inf-eval when run through nox.
estimate: ~5–8 sessions (a real compiler; the hardest piece).

## R2 — zheng proofs (stage 4)

turn the nox trace into a STARK and make a result certify against a BBG root.

- wire the nox `Tracer` output → zheng proof generation (SuperSpartan + Brakedown
  over Lens).
- relation reads become Lens openings; a multi-row result is a batch opening +
  the derivation circuit (proof.md).
- the operator→constraint mapping (select=range check, join=lookup, sort=perm,
  aggregate=accumulator) from proof.md / bbg/roadmap/verifiable-query.
- mutation: prove the appended batch is exactly the rule's derivation.

verify: proofs check in constant time against a root; size/verify benchmarks.
depends on: R1, the zheng crate API. estimate: ~3–5 sessions.

## R3 — bbg / cybergraph sources + privacy

replace LocalSource with real committed state.

- BbgSource: RelationSource over bbg's ShardStore + committed GraphStats
  (node_count, relation_sizes, max_degree, diameter_bound — bbg/specs/statistics)
  feeding the cost model and recursion bounds.
- CybergraphSource: derived/normalized relations, tru-fresh focus/karma, content.
- privacy: `cyberlinks` neuron-scoping (own plaintext ∪ keyed-in) via the owner
  store; session context carries caller identity + keys (relations.md, the open
  questions); reject writes to canonical relations.
- the full `cost` model: read_cost + committed stats → static cycle ceiling.

verify: queries over a real bbg snapshot; privacy scope tests; cost = committed.
depends on: bbg usability. estimate: ~3–5 sessions.

## R4 — reactive and live registers (extensions)

beyond the pure register (extensions.md).

- reactive: parse `:subscribe sel`; a driver that re-evaluates on matching
  mutation events; event-log-conditional proof.
- live: `Host.fetch(...)` as a witness-bearing call; the proof is conditional on
  the external result.
- enforce the one-way boundary: reactive/live inputs never enter the pure subset.

verify: a subscription re-fires and re-derives; witness-conditional proof shape.
estimate: ~2–4 sessions.

## R5 — builtin and algorithm tail

mechanical breadth against functions.md / algorithms.md.

- functions: vector (→ Ten: vec, cos_dist, l2_dist), json (parse/dump/get),
  regex (match/replace/extract), time (validity, format/parse). transcendentals
  and random stay excluded from the pure subset by design.
- algorithms: DepthFirstSearch (done), LabelPropagation, ClusteringCoefficients,
  KShortestPathYen, ShortestPathAStar, MinimumSpanningForest, RandomWalk
  (committed-seed). fractional ones (Betweenness, Closeness, Louvain) need a
  fixed-point representation like PageRank.

verify: per-function and per-algorithm tests against hand-computed results.
estimate: ~2–3 sessions.

## sequencing

```
R0 (quick wins + oracle)  ──►  R1 (relational nox)  ──►  R2 (zheng)
                               R3 (bbg sources) ───────────┘ (cost + proofs need it)
R4 (reactive/live) and R5 (tail) run in parallel any time after R0.
```

R0 first (cheap, hardens correctness). R1 is the critical path to a real proof
language; R2 needs R1; R3 unblocks the committed cost model and real proofs.
total to a proving, bbg-backed inf: roughly 15–25 sessions.
