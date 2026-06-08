# inf/rs — the inf engine

the Rust implementation of [[inf]], written from scratch per
.claude/plans/stage3-engine.md. the core language is complete and tested; the nox
lowering reaches stage 3 for the expression layer. cozo/ is the gitignored
study/oracle fork (excluded from this workspace).

## crates

| crate | dir | what |
|---|---|---|
| inf-value | `value/` | Goldilocks field, value model, set-semantic relations |
| inf-ast | `ast/` | AST + IR types |
| inf-lex | `lex/` | tokenizer |
| inf-parse | `parse/` | parser → AST (pure register) |
| inf-source | `source/` | RelationSource trait + LocalSource |
| inf-plan | `plan/` | stratify, recursion detection, safety → IR |
| inf-eval | `eval/` | reference semi-naive evaluator + fixed-rule algorithms |
| inf-lower | `lower/` | stage-3 lowering of expressions to the real nox VM |
| inf-cli | `cli/` | the `inf` binary (check/plan/cost/run) |

## build and test

```
cargo test                                  # core crates (value..cli) — pure stable Rust
RUSTC_BOOTSTRAP=1 cargo test               # whole workspace incl. inf-lower (pulls nox)
RUSTC_BOOTSTRAP=1 cargo test -p inf-lower --features prove  # R2 zheng proofs
cd oracle && RUSTC_BOOTSTRAP=1 cargo test  # differential oracle vs CozoDB (needs rs/cozo)
```

inf-lower depends on the `nox` crate (→ nebu, hemera), which needs
`RUSTC_BOOTSTRAP=1`. The core engine has no external cyber dependencies and
builds/tests on its own. 86 tests pass in the default suite; zero warnings.
A further 10 tests run under `--features bbg` (R3b `BbgSource`):

```
RUSTC_BOOTSTRAP=1 cargo test -p inf-source -p inf-lower --features bbg
```

Run a query against the built-in demo graph:

```
cargo run -p inf-cli -- run query.inf
cargo run -p inf-cli -- check query.inf
cargo run -p inf-cli -- cost query.inf    # now includes committed GraphStats output
```

## spec compliance

| spec | status | notes |
|---|---|---|
| language.md | complete (pure register) | value model (no float), rules, entry, aggregation, bounded recursion, mutation derivation, `pub input/output` decls, `_`-prefixed temp relations. reactive/live = extensions (done). |
| grammar.md | complete | rules, conditions, binds, negation, mutation (`:link`/`:unlink`/`:put`/`:rm`), options, fixed rules (`<~`), addresses, infix arithmetic. |
| relations.md | R3a + R3b BbgSource | LocalSource + GraphStats (R3a); `BbgSource` over real committed `BbgState` reads all canonical dimensions as full-column relations + committed `GraphStats` (R3b, `--features bbg`). CybergraphSource + cyberlink privacy scoping still pending. |
| functions.md | complete | math (add/sub/mul/div/mod/neg/abs/pow/signum/min/max); comparison + logical + `in`; string (length, concat, case, trim, chars, reverse, slice, starts/ends/includes); list (first, last, get, slice, sorted, reverse, prepend, append, union/intersection/difference); vector (vec, ip_dist, l2_dist — exact integer); regex (matches/replace/extract); json (is_json, json_get); coalesce, to_int, to_string; aggregations count/sum/min/max/mean/collect/unique/union. excluded by design: transcendentals, random, `now` (clock), cos_dist/normalize (need fixed-point) and time/validity (bbg time dimension). |
| algorithms.md | complete | PageRank, DegreeCentrality, ClosenessCentrality, BetweennessCentrality (Brandes), ConnectedComponents, StronglyConnectedComponent, LabelPropagation, ClusteringCoefficients, CommunityDetectionLouvain (single-level greedy), BFS, DFS, ShortestPathBFS, ShortestPathDijkstra, ShortestPathAStar, KShortestPathYen, MinimumSpanningForestKruskal, TopSort, RandomWalk (committed seed). fractional metrics are fixed-point scaled (×10⁶, no float); `theta` is an integer percent. |
| cost.md | mostly complete | `inf cost` reports a static estimate with reads/combine/recursion breakdown; uses committed `GraphStats` (node_count, diameter_bound, max_degree) when computed; recursion bound defaults to `diameter_bound`. |
| ir.md | R1 complete | IR (strata) implemented; nox lowering covers arithmetic, comparisons, boolean connectives, column access, full relational scans + equi-joins + bounded-recursion trace via look (17) pattern (`nox_vm_scan`, `nox_vm_scan_gt`, `nox_vm_equijoin`, `nox_vm_bounded_reach`). LocalLookProvider serves LocalSource data; all differential-tested against reference eval. |
| proof.md | R2 initial | `prove_expr` / `prove_cond` / `verify_expr_proof` via `--features prove`; pure arithmetic + comparison traces proved and verified with zheng (SuperSpartan + Brakedown, empty hash/look openings). Full query-level proof (with look openings against BBG root) pending R3 bbg integration. |
| extensions.md | complete | reactive (`:subscribe` + `eval_reactive` event driver) and live (`Host.fn(..)` witness via a host provider). |
| bootstrap.md | stage 2+3 | parse→IR→eval is the working reference engine; stage 3 = condition lowering on real nox (R1 done); staged proof path (R2 done for expressions). cozo differential oracle wired (rs/oracle). |

## verification

- 86 unit + integration tests across the workspace (8 source, 20 eval, 22 lower, 9 cli, …).
- `eval` corpus covers: join/filter/sort/limit, bounded reachability, full
  closure, aggregation, negation, `:assert`, mutation batch, infix arithmetic,
  and all fixed-rule algorithms.
- `inf-lower` R1a: nox-VM result == reference engine result for arithmetic
  expressions, column expressions, and whole-query condition evaluation (gt/lt/
  eq/ne/ge/le/and/or/not) — differential-tested on scan, join, bounded recursion.
- `inf-lower` R1b/R1c: `nox_vm_scan` / `nox_vm_scan_gt` / `nox_vm_equijoin`
  / `nox_vm_bounded_reach` — relational scans, equi-joins, and bounded BFS
  compiled to static nox look formulas, executed on the nox VM via
  `LocalLookProvider`, all differential-tested against reference eval (7 tests).
- `inf-source` R3a: `GraphStats` computation from local data; diameter_bound,
  max_degree, relation_sizes; `graph_stats()` on RelationSource trait.
- `inf-source`/`inf-lower` R3b (`--features bbg`): `BbgSource` reads the canonical
  bbg dimensions (full columns) + committed `GraphStats` from a real `BbgState`;
  differential parity — `eval` over `BbgSource` == `eval` over a `LocalSource`
  mirror, for scan, filter, and multi-column queries (7 source + 3 lower tests).
- `--features prove` (R2): `prove_expr` / `prove_cond` produce zheng
  `TraceProof`s for arithmetic and comparison expressions; `verify_expr_proof`
  checks them end-to-end via the SuperSpartan + Brakedown verifier.
- reactive (`eval_reactive`) and live (`Host` witness) register tests.
- the `inf` CLI runs the spec-example queries (discovery, policy, reachability,
  PageRank, mutation) end-to-end with correct results; `inf cost` reports
  committed GraphStats (node_count, diameter_bound, max_degree).
- the differential oracle (rs/oracle) asserts inf-eval == CozoDB on filter,
  join, aggregation, transitive closure, and negation.

## what remains

- **R3b tail**: `CybergraphSource` (derived/normalized views) + cyberlink privacy
  scoping (neuron-keyed). `BbgSource` (committed reads) is done.
- **bbg proof door** (`bbg/roadmap/provable-reads.md`): the `look` primitive opens an
  MLE fingerprint, not the record cell — sound column reads (corner opening + position
  binding) are a prerequisite for R2b. bbg-side protocol work, security-reviewed.
- **R2b**: once the proof door is sound, prove whole-query results with look openings
  against the BBG root. inf-side plumbing models on `cybergraph/tests/stack_look.rs`.

see `.claude/plans/remaining.md`.
