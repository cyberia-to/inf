# inf — remaining work

status: 2026-06-07. the LANGUAGE is feature-complete and verified in inf/rs
(60 tests, 0 warnings): parse → plan → eval with recursion, aggregation,
negation, mutation derivation, the full function library, every graph algorithm,
reactive + live registers, a CLI, a cozo differential oracle, and nox lowering of
query expressions over tuple columns. DONE: R0, R4, R5; R1 partial. remaining:
the R1 backend (full relational lowering as one nox trace), R2 (zheng proofs),
R3 (bbg/cybergraph sources) — the proof/storage backend. compliance map:
inf/rs/README.md.

## R0 — quick wins and the oracle  (DONE 2026-06-07)

- cozo differential-oracle: rs/oracle (excluded crate — path-deps gitignored
  cozo; built with `default-features=false, features=["rayon"]` to dodge the
  broken `graph` dep). 5 tests assert inf-eval == CozoDB on filter, join,
  aggregation, transitive closure, negation.
- `pub input/output` decls: parsed into the AST (Decl/Type/Dir), Named/Array/
  Record types; `;` added to the lexer.
- temp relations: `_`-prefixed derived relations work as intra-program temps
  (test added). `:put`/`:rm` materialization across statements still waits on
  multi-statement transaction blocks (deferred — needs `{ … }` scripts).
- `cost`: reads / combine / recursion breakdown in the CLI. committed-GraphStats
  part still waits on R3 (bbg).

## R1 — relational nox lowering (stage 3 complete)  (PARTIAL)

DONE: inf-lower runs query expressions (arithmetic) and expressions over
relation-tuple columns (subject/axis) on the real nox VM, differential-tested vs
inf-eval. REMAINING: whole queries as one nox trace — the items below. this is a
real Nock-style compiler (fold/join/recursion formulas) and the main path to
provability; ~5 sessions.

extend inf-lower from expressions to whole queries, executed on the
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

## R4 — reactive and live registers  (DONE 2026-06-07)

- reactive: `:subscribe rel{..}` parsed; `eval_reactive(ir, source, events, ctx)`
  applies each event and re-evaluates when it matches the subscription. the event
  log is the witness sequence.
- live: `Host.fn(args)` resolved by a `HostFn` provider on `Ctx` as a witness;
  no provider → error. the pure builtins are untouched.
- event-log/witness-conditional PROOFS arrive with R2 (zheng); the registers and
  their execution semantics are in place and tested.

## R5 — builtin and algorithm tail  (DONE 2026-06-07)

- functions: math (pow/signum/…), string (trim/chars/reverse/slice/…), list
  (sorted/prepend/append/union/intersection/difference/…), vector (vec/ip_dist/
  l2_dist — exact integer), regex (matches/replace/extract via `regex`), json
  (is_json/json_get via `serde_json`). excluded by design: transcendentals,
  random, `now`, cos_dist (fixed-point), time/validity (bbg).
- algorithms: ClosenessCentrality, BetweennessCentrality (Brandes),
  LabelPropagation, ClusteringCoefficients, CommunityDetectionLouvain (single-
  level greedy), KShortestPathYen, ShortestPathAStar, MinimumSpanningForest
  (Kruskal), RandomWalk (committed seed) — added to the earlier PageRank/degree/
  components/SCC/BFS/DFS/Dijkstra/TopSort. fractional metrics fixed-point scaled.

## sequencing

```
R0 (quick wins + oracle)  ──►  R1 (relational nox)  ──►  R2 (zheng)
                               R3 (bbg sources) ───────────┘ (cost + proofs need it)
R4 (reactive/live) and R5 (tail) run in parallel any time after R0.
```

R0 first (cheap, hardens correctness). R1 is the critical path to a real proof
language; R2 needs R1; R3 unblocks the committed cost model and real proofs.
total to a proving, bbg-backed inf: roughly 15–25 sessions.
