# query optimization — indexing, reordering, ordered aggregate, semi-naive

status: 2026-08-05, DONE — P0, P1, P2, P3 all landed, full suite green (98
tests + 10 under `--features bbg` + 5 cozo oracle). root-caused against a
real lytics workload (`passage_ids` — an inequality self-join computing a
running count over 1600 events measured at ~485ms). the cause generalizes
to any inf caller, including cybergraph queries over `BbgSource`, so the
fix belongs in inf, not in the caller.

results:
- P0 (hash-indexed joins, `rs/eval/src/lib.rs`): the mtc-shaped scale test
  (2000 events, larger than the reported bottleneck) — 25ms release / 232ms
  debug, plus dropping the redundant second confirmation round every
  non-recursive stratum was paying.
- P1 (planner reordering, `rs/plan/src/lib.rs`): filter pushdown + most-
  bound-first join scheduling, zero coupling risk to nox lowering (verified
  `inf-lower`'s functions take explicit params, never consume `plan()`'s IR
  generically).
- P2 (bounded ordered aggregate, spec + `rs/eval/src/agg.rs` +
  grammar/parser/plan): `running_*` + `:order` — the language-level fix
  replacing the O(n²) self-join idiom with an O(n) primitive that composes
  into proof.md's existing sort + running-accumulator constraint forms
  (O(n) trace instead of O(n²)).
- P3 (semi-naive recursion, `rs/eval/src/lib.rs`): closes a real gap
  between what language.md/ir.md already specified ("semi-naive") and what
  `eval` actually ran (naive full re-derivation every round). 300-hop chain:
  123ms, vs. O(depth²) a naive re-derivation would pay.

each landed as its own commit (`git log --oneline` in this repo).

## diagnosis (verified directly against source, not assumed)

- `rs/eval/src/lib.rs` `read_join`/`unify`: every relation read is a full
  linear scan — `for b in &bindings { for row in &rows { unify(..) } }` —
  O(bindings × relation rows), regardless of whether the join binds an
  already-bound column. no index of any kind.
- `inf_value::Relation = BTreeSet<Tuple>` (`rs/value/src/lib.rs`): ordered
  by the *whole tuple*, not by any join key. `rows_of` flattens it to a
  `Vec` before scanning, so even that ordering is unused.
- `rs/plan/src/lib.rs` `plan()`: stratification + range-restriction safety
  only. zero join reordering, zero filter pushdown, zero use of the
  committed `GraphStats` it already has available at cost-estimate time.
  atom order in a rule body IS join order — whatever the author wrote.
- `agg.rs` aggregation is a single O(n) `BTreeMap` group-by — not a factor.
- `eval_stratum`'s recursive loop is naive: every round re-derives every
  rule over the *full* current relations, not just newly-added (delta)
  tuples. correct but wasteful for real recursion depth.
- `rs/lower/src/lib.rs` (`nox_vm_scan`/`nox_vm_equijoin`/`nox_vm_bounded_reach`)
  takes explicit relation/column parameters, built by hand per differential
  test — it does NOT consume `plan()`'s emitted `Rule.body` order generically.
  confirmed by reading every `pub fn` signature in the crate. this means
  atom reordering in the planner has zero coupling risk to the nox-lowering
  differential harness (R1a/R1b/R1c) — those tests compare final result
  sets, not join execution order.

## why the imperative Rust version never hit this

order and mutable state are free in Rust (sort once, single pass, running
counter = local variable). datalog has neither by design — a running count
must be re-expressed as a fact about *pairs* (`count of arrivals in
(first, ts]`), which is what turns a linear scan into a self-join. the
engine then executes that pairwise form literally, with no indexing to
recover the cost the imperative version got from data-structure choice.

## the four layers, in dependency order

### P0 — hash-indexed joins (engine-only, zero semantic risk)

lazily build a hash index on a relation the first time it is read with a
particular set of bound columns; reuse it for the rest of that read's
lifetime (a stratum's fixed-point pass, or for non-recursive strata, the
one pass). `unify` still runs — only the candidate row set narrows from
"the whole relation" to "the rows matching the bound columns." same rows
unify, same output rows, strictly fewer `unify` calls. no result-set
change, so no spec change and no risk to nox lowering (which never sees
inf-eval's internal join strategy).

### P1 — atom reordering (planner-level, safe by construction)

two independently-safe transforms, applied in `plan()` after safety
checking (safety checking still runs against the *original* body — the
range-restriction proof holds regardless of body order, since it's a
static bound-variable computation):

1. filter pushdown: move each `Cond`/`Not` atom to immediately after the
   last positive atom that binds one of its free variables. always safe —
   a filter's truth value doesn't depend on *when* it's evaluated, only on
   its variables being bound already.
2. most-bound-first among interchangeable `Read`/`Apply` runs: a positive
   join is commutative, so any two `Read`/`Apply` atoms with no `Cond`/`Not`
   between them that depends on one binding the other's output may swap.
   order by (number of already-bound columns in this atom's `binds`,
   descending), a static proxy for selectivity that needs no runtime
   `RelationSource` access — `plan()` only sees the AST. ties keep source
   order (stable sort) so this stays deterministic across runs, which
   matters for `specs/proof.md`'s determinism requirement.

no IR shape change — `Stratum`/`Rule` types stay the same, only `Rule.body`
element order changes.

### P2 — bounded ordered aggregate (language primitive)

the actual gap: `passage_ids`-shaped queries are not really a join problem,
they are a *sequence* problem with no native expression, so authors fall
back to an inequality self-join. adding one ordered construct removes the
need for that idiom:

```
pid[neuron, ts, running_count(arrival)] := events{neuron, ts, arrival} :order ts
```

lowers to: stable sort the bound rows by the `:order` key (per grouping
columns preceding the aggregate), then a single accumulator pass — exactly
the Rust loop this replaces, but declared and provable.

this also matters for proving, not just running: `specs/proof.md`'s
operator table already has both required constraint forms — sort →
permutation argument, aggregate → running accumulator — so this primitive
composes into an O(n) proof trace. the self-join encoding proves the same
fact with O(n²) trace size (one row-pair per comparison). the language
currently forces users into the worse-for-both encoding.

touches: `specs/grammar.md`, `specs/language.md`, `specs/ir.md`,
`specs/proof.md` (spec first, then lex/parse/ast/plan/eval).

### P3 — semi-naive recursion

standard delta evaluation: track `total` and `delta` (this round's new
facts) per recursive relation; each round, join only `delta` against the
rule bodies (not all of `total`), union newly-derived facts into `total`,
set next `delta` = new facts not already in `total`. same fixed point as
the naive loop (monotone positive program, per stratification's own
guarantee) — must produce byte-identical final relations to the current
naive loop on every existing recursion/bounded-reach test.

## sequencing and risk

```
P0 (done first, isolated, zero semantic risk)
P1 (isolated to plan(), zero nox-lowering coupling — confirmed above)
P2 (new spec + new AST/grammar surface — biggest surface, most spec work)
P3 (isolated to eval_stratum's recursive branch)
```

each lands as its own commit, full test suite (`cargo test`, then
`RUSTC_BOOTSTRAP=1 cargo test --features bbg` where applicable) green
before moving to the next.

## non-goals here

- lytics' own ingest-time passage-id materialization (their app-level
  incremental-materialization fix) — out of scope, lives in the lytics
  repo, not inf.
- result caching — masks the pathology, doesn't fix inf.
- R2b (full query-level zheng proof) — unaffected by this work; P2's
  ordered aggregate is designed to compose into that proof model when it
  lands, not to require it now.
