# inf — remaining work

status: 2026-06-08. the LANGUAGE is feature-complete and verified in inf/rs
(86 tests default + 10 under `--features bbg`, 0 warnings): parse → plan → eval
with recursion, aggregation, negation, mutation derivation, the full function
library, every graph algorithm, reactive + live registers, a CLI, a cozo
differential oracle, full relational nox lowering (scan + equi-join +
bounded-recursion trace via look pattern), committed GraphStats, `BbgSource` over
real committed state (R3b), and zheng proof generation for expression/condition
traces (R2a). DONE: R0, R1, R2a, R3a, R3b-BbgSource, R4, R5.
remaining: R2b (full query proof — inf wiring over bbg's existing look read; bbg/nox need
no architectural change), R3b tail (CybergraphSource + privacy scoping).
compliance map: inf/rs/README.md.

## R0 — quick wins and the oracle  (DONE 2026-06-07)

- cozo differential-oracle: rs/oracle (excluded crate — path-deps gitignored
  cozo; built with `default-features=false, features=["rayon"]` to dodge the
  broken `graph` dep). 5 tests assert inf-eval == CozoDB on filter, join,
  aggregation, transitive closure, negation. KEEP cozo until stage 4 — it's the
  only external semantics oracle and costs nothing (gitignored + workspace-
  excluded); delete `rs/cozo` + `rs/oracle` once R2b/R3b land and the oracle
  reruns green.
- `pub input/output` decls: parsed into the AST (Decl/Type/Dir), Named/Array/
  Record types; `;` added to the lexer.
- temp relations: `_`-prefixed derived relations work as intra-program temps
  (test added). `:put`/`:rm` materialization across statements still waits on
  multi-statement transaction blocks (deferred — needs `{ … }` scripts).
- `cost`: reads / combine / recursion breakdown in the CLI. uses committed
  GraphStats (node_count, diameter_bound, max_degree) when available.

## R1 — relational nox lowering  (DONE)

### R1a — expression + condition lowering  (DONE 2026-06-07)

- arithmetic: add/sub/mul lower to nox opcodes 5/6/7. differential-tested.
- column access: column i at axis 2^(i+2)-2 in the right-nested cons-list
  subject; `lower_and_run_tuple` evaluates over real relation rows.
- comparison conditions: lt/gt/eq/ne/le/ge compile to nox formulas where 0 =
  condition satisfied (nox branch convention). derivations:
    ne(a,b) = lt(0, eq(a,b))   — 0 when eq=1 (not equal)
    gt(a,b) = lt(b, a)         — 0 when b<a (a>b)
    gte(a,b) = eq(lt(a,b), 1)  — 0 when lt=1 (a>=b)
    le(a,b) = eq(lt(b,a), 1)   — 0 when lt(b,a)=1 (a<=b)
    and(c1,c2) = add(c1, c2)   — 0 iff both 0
    or(c1,c2)  = mul(c1, c2)   — 0 iff at least one 0
    not(c)     = eq(c, 1)      — 0 iff c=1 (was false)
- `CondEval` callback in eval `Ctx`: `nox_cond: Option<CondEval>` routes each
  `Atom::Cond` through nox with fallback to reference for non-numeric types.
- `make_nox_ctx()` in inf-lower: builds a Ctx with nox condition evaluation.
- whole-query differential test: scan / join / bounded-recursion / eq filter all
  produce identical result sets via nox and reference engine (5 corpus tests).

### R1b — full relational scan + join via look  (DONE 2026-06-08)

scan and equi-join queries compiled to static nox look formulas, executed on
the real nox VM against a LocalLookProvider, differential-tested vs reference.

- `LocalLookProvider`: implements `LookProvider` + `CallProvider<N>` over
  LocalSource; key = row_idx * MAX_ARITY + col_idx; ns = relation order index;
  commitment ignored (LocalSource has no BBG root).
- `bbg_obj()`: fake BBG subject [[0|[0|[0|0]]] | 0] with all-zero limbs for
  the look pattern's axis-navigation (axes 4, 10, 22, 23).
- `nox_vm_scan()`: builds a cons-list formula of look calls, executes on nox,
  returns rows as Vec<Vec<u64>>. differential-tested against LocalSource scan.
- `nox_vm_scan_gt()`: scan with gt filter via branch (4): `[4 [lt(thresh,col)
  [cons(tuple, rest) rest]]]`. 0-branch = include.
- `nox_vm_equijoin()`: nested scan; join condition = eq(look1, look2); optional
  gt filter combined via add (0 iff both zero). result = cons-list of tuples.
- `decode_results()`: traverse cons-list of cons-tuples → Vec<Vec<u64>>.
- 5 tests: scan_all_rows, scan_single_column, scan_gt_filter,
  equijoin_matches_reference, equijoin_with_filter_matches_reference.
- arena limit: N=8192 (stack-safe); supports up to ~8k formula nodes per query.

### R1c — bounded recursion trace  (DONE 2026-06-08)

`nox_vm_bounded_reach(src, edge_rel, from_col, to_col, seed, extra_steps)`.
- frontier state: Rust `Vec<u64>`, passed as literal constants baked into each
  iteration's formula (avoids noun encode/decode overhead).
- step formula: per-edge look(ns=0, row*MAX_ARITY+col) scan; mul-or condition
  over frontier values (0 iff from-node matches any frontier member); branch
  includes to-node in cons accumulator.
- outer loop: base step (from seed), then extra_steps BFS rounds; dedup via
  `reached` set.
- 2 tests: 1-hop (extra_steps=1, matches ref :bounded 2) and 2-hop (extra_steps=2,
  matches ref :bounded 3). Both pass differentially vs reference eval.

## R2 — zheng proofs  (INITIAL DONE; FULL PENDING)

### R2a — expression/condition proof  (DONE 2026-06-07)

- `prove_expr(t)` → `(result, TraceProof)`: runs the expression with VecTrace,
  calls `zheng::commit` with empty hash/axis/look openings (pure arithmetic).
- `prove_cond(cond, cols, tuple)` → `(holds, TraceProof)`: proves a condition
  evaluation over a specific tuple.
- `verify_expr_proof(proof)`: verifies via `zheng::verify`.
- tests: prove_and_verify_arithmetic_expr, prove_and_verify_condition,
  prove_and_verify_tuple_condition (3 tests under `--features prove`).
- gated: `inf-lower/Cargo.toml [features] prove = ["dep:zheng", "nox/brakedown"]`.

### R2b — full query-level proof  (inf wiring; bbg read primitive is sufficient)

The bbg read primitive (index-addressed cell opening via `look`) is enough — no bbg/nox
architectural change. (A bbg provider *bug* — an off-corner MLE fingerprint, entity-
addressed — was fixed in bbg's `proof.rs`/`query.rs`; verification of that fix is parked
behind the in-flight nox `NounId→Word` refactor.) R2b is inf implementation:
- lower the query to nox `look` reads (index-addressed cells) + composition (key-binding
  = read key cells + `eq`); run with bbg's `ProofLookProvider`.
- `zheng::commit(trace, &[], &[], look_openings, stmt, params)` → `verify`; openings come
  from `ProofLookProvider` / `collect_look_openings` (already exist). Model on
  `cybergraph/tests/stack_look.rs`.
- differential: proven result == inf-eval over BbgSource. verify in ~constant time.

## R3 — bbg / cybergraph sources + privacy  (LOCAL DONE; BBG PENDING)

### R3a — committed GraphStats (local)  (DONE 2026-06-07)

- `GraphStats { node_count, relation_sizes:[u64;11], max_degree, diameter_bound }`
  in inf-source/src/lib.rs (mirrors bbg/specs/statistics.md exactly).
- `rel_idx::*` constants for the canonical 11 relations.
- `LocalSource::ensure_stats()`: computes stats from actual local data.
- `LocalSource::with_stats(GraphStats)`: inject explicit committed stats.
- `RelationSource::graph_stats() → Option<GraphStats>`: default returns None.
- `cost` CLI uses committed stats: recursion defaults to diameter_bound, not 16.
- 7 tests in inf-source.

### R3b — BbgSource (DONE 2026-06-08) + CybergraphSource (PENDING)

`BbgSource` reads real committed state. **DONE:**
- `inf-source/src/bbg.rs` (feature `bbg`): `BbgSource<'a>{ state: &'a bbg::BbgState }`
  impl `RelationSource`. all 11 canonical dimensions → full-column relations
  (particles/neurons/locations/coins/cards/files/time/signals/balances as
  entity records; axons_out/axons_in/`axons` flattened into edge tuples).
  `graph_stats()` maps `state.statistics()` (field-identical to inf GraphStats).
  `snapshot()` = `state.height`. **`provable()` = false** for now — flip to true once
  R2b wires look openings (the bbg read primitive itself is sufficient).
- crate unification verified: bbg + inf resolve nox/zheng/nebu to the same paths.
- 7 source tests + 3 inf-lower differential-parity tests: `eval` over BbgSource ==
  `eval` over a LocalSource mirror (scan / filter / multi-column). +10 under
  `--features bbg`; default suite unchanged (still 86), 0 warnings.

**PENDING (R3b tail):**
- CybergraphSource: derived/normalized relations over BbgSource.
- privacy: `cyberlinks` neuron-scoping (own plaintext ∪ keyed-in); session
  context carries caller identity + keys.
- R2b look openings over BbgSource — inf wiring (bbg's `ProofLookProvider` already
  serves the index-addressed read; no bbg/nox change needed).

## R4 — reactive and live registers  (DONE 2026-06-07)

- reactive: `:subscribe rel{..}` parsed; `eval_reactive(ir, source, events, ctx)`
  applies each event and re-evaluates when it matches the subscription.
- live: `Host.fn(args)` resolved by a `HostFn` provider on `Ctx` as a witness.
- `CondEval` hook added for nox condition routing (see R1a).

## R5 — builtin and algorithm tail  (DONE 2026-06-07)

- functions: math, string, list, vector (exact integer), regex, json.
- algorithms: PageRank, all centralities, components, community detection,
  shortest paths (BFS/Dijkstra/AStar/Yen), MST, TopSort, RandomWalk.

## sequencing

```
R0 (done)  ──►  R1a (done)  ──►  R1b (full trace)  ──►  R2b (full proof)
                R3a (done)  ──►  R3b (BbgSource) ────────────────────────┘
R2a (done) — expression-level proof works today, no bbg needed.
R4, R5 (done)
```

total to a proving, bbg-backed inf: R1b + R2b + R3b ~= 11–18 sessions.
