# inf — bbg integration plan (R2b + R3b)

status: 2026-06-08, planning. supersedes the "depends on bbg integration"
placeholder in remaining.md. R0, R1 (a/b/c), R2a, R3a, R4, R5 are DONE (86 tests).
this plan covers the two remaining items: R3b (BbgSource over real committed
state) and R2b (full query-level proof with look openings against the BBG root).

## what changed: bbg is usable NOW

investigation (2026-06-08) confirms the blocker is gone:

- **bbg/rs builds standalone, 49 tests pass**, edition 2024, no RUSTC_BOOTSTRAP,
  zero `todo!`/`unimplemented!` in the read path.
- **the R2b proof flow already exists** in bbg + cybergraph:
  - `bbg/rs/src/query.rs`: `BbgLookProvider` (fast `nox::LookProvider`),
    `ProofLookProvider` (records `LookOpening`s during a nox run via `look()`),
    `collect_look_openings(state, trace)` (post-hoc openings from a trace),
    `bbg_query`/`verify_query`/`verify_opening`.
  - `cybergraph/tests/stack_look.rs`: a working end-to-end roundtrip —
    build bbg object → `nox::reduce` a look formula → `take_look_openings()` →
    `zheng::commit(trace, &[], &[], &look_openings, stmt, params)` → `zheng::verify`.
    R2b is *adopting this template*, not inventing it.
- **crate unification holds** — the dependency-graph risk is cleared:
  inf-lower and bbg both resolve `nox` → `~/cyber/nox/rs`, `zheng` → `~/cyber/zheng`,
  `cyb-nebu` → `~/cyber/strata/nebu/rs`. So `Goldilocks`, `LookOpening`,
  `TraceRow`, `reduce` are the *same types* across the boundary. No version skew.

## THE design decision: relational model vs bbg's look model

inf's R1b/R1c lowering uses a **synthetic, local** addressing:
`look(ns = relation_order_idx, key = row*MAX_ARITY + col)`, all-zero-limb subject.
that is a *test fixture* over a flat `LocalSource` table. it does NOT match real bbg.

bbg's real look (`query.rs:127`,`165`,`246`) is **entity-keyed, primary-scalar**:
`look(ns = Dim 0..9, key = entity_id_first_8_bytes_LE) → ONE scalar per record`:

| Dim | primary scalar returned by `look_scalar` |
|---|---|
| Particles | `energy` |
| AxonsOut/In | adjacency-list length |
| Neurons | `focus` |
| Locations | `lat` |
| Coins | `total_supply` |
| Cards | `owner[0]` |
| Files | `available` |
| Time | `root[0]` |
| Signals | `link_count` |

full multi-column records are NOT reachable through the look pattern. they ARE
reachable two other ways:
- `ShardStore::iter(dim) → (key, &[Goldilocks])` — full record, **unproven scan**.
- `bbg_query(dim, key) → QueryProof{ value_bytes }` — full record, **Brakedown-proven**,
  but the nox VM only receives the first scalar (`query_value` reads bytes[..8]).

**consequence — the two layers split cleanly:**
- **data/scan path (R3b)** uses `iter` → full columns, the natural realization of
  "inf relations are VIEWS over BBG polynomial state."
- **proof path (R2b)** uses the look pattern → sound single-scalar, entity-keyed reads.

### framing: inf is the language of sets

inf is datalog — a relation is a SET of tuples, a rule is set-builder notation, eval
is least-fixed-point set construction, mutation is set algebra (signal = set of
cyberlinks). the declarative twin of rune (the language of STEPS).

the whole integration tension is one sentence: **inf wants sets of tuples; bbg stores
entity-keyed records, each with one "headline" scalar.** bbg is a notarized ledger
with two doors:

- **scan door** (`ShardStore::iter`): "give me the whole relation" → every full row,
  all columns, but NOT individually notarized.
- **proof door** (`look`): "certify the value at this one key" → ONE notarized scalar
  (the headline: energy/focus/link_count…), and you must already know the key.

### the only real decision — REVISED 2026-06-08 after deeper investigation

reading data and proving data use different doors:

- **R3b (read) → scan door.** not a judgment call, just the work: inf relations = the
  11 canonical bbg dimensions, full columns decoded from `iter`; `graph_stats()` from
  `state.statistics()`. **unblocked, sound by construction, ships now.**
- **R2b (prove) → proof door — GATED.** investigating the door (`bbg/rs/src/proof.rs`,
  `query.rs`) found its value semantics is a **placeholder**: `open_dim` opens the
  dimension poly at the *key value* as a point (not a hypercube corner), so the opened
  value is an **MLE fingerprint**, not the entity's column. `ProofLookProvider` returns
  that fingerprint while `BbgLookProvider` returns the real column — the two **disagree**.
  so even "headline" reads (focus>T) are **not soundly provable today** — my earlier
  "80% ships now" was wrong.
  - **decision (user, 2026-06-08):** ship R2b for headline/statistics queries, but FIRST
    fix the proof door — captured as a thought-through proposal in
    **`bbg/roadmap/provable-reads.md`** (NOT `specs/props`; the user wants it analyzed in
    roadmap before any protocol change). R2b sequences *after* that lands.
  - the proposal covers: (1) sound column read = corner opening + position/key binding +
    provider consistency (prerequisite for ANY column proof, headline included); (2) four
    options with a full comparison table (features / pros / cons) — A: per-column
    `ns=dim*16+col` (taxes nox, `ns>9` rejected at `patterns/look.rs:58`); B: record-opening
    (sound, 1 batched opening/entity via `Lens::batch_open` which exists, keeps `ns` 0..9);
    C: bbg-only structured-key stopgap; D: set-level relational-algebra proofs (no per-entity
    look — the [[verifiable-query]] compiler; native fit for a sets language).
    cross-cutting: full 32-byte entity addressing (today's 8-byte key truncates).
  - **recommendation in the proposal (split, not one winner):** always fix soundness first;
    **point reads → B**, **set queries → D**; drop A/C except as stopgaps. key facts driving
    it: the current look has **zero production consumers** (free to redefine) and Brakedown
    **batch_open exists**.

R1b/R1c's `LocalLookProvider` stays as the **local/test** addressing for differential
tests; the bbg path is additive and uses bbg's own providers.

## R3b — BbgSource + CybergraphSource

goal: a `RelationSource` that reads real committed relations + committed `GraphStats`.

1. **inf-source `bbg` feature** (keeps the pure core pure):
   - add optional dep `bbg = { path = "../../../bbg/rs" }` under `[features] bbg = ["dep:bbg"]`.
   - default build stays std-stable, no RUSTC_BOOTSTRAP; `--features bbg` pulls the chain.
2. **`BbgSource<'a> { state: &'a bbg::BbgState }`** impl `RelationSource`:
   - `scan(rel)`: map `rel` → `Dim`; iterate the dimension's `BTreeMap` (or
     `ShardStore::iter`); decode each record (`ParticleRecord`, `NeuronRecord`, …,
     `bbg/rs/src/types.rs`) into a row of inf `Value`s. column order = a fixed schema
     table (one per dimension) living in inf-source.
   - `graph_stats()`: return `state.statistics()` mapped into inf-source's `GraphStats`
     (field names already mirror bbg per R3a — confirm the 11-relation order matches
     `bbg/rs/src/stats.rs:37`).
3. **`CybergraphSource`**: derived/normalized views over `BbgSource` (axons_out/in as
   edge relations, neuron summaries). privacy scoping (`cyberlinks` neuron-keyed) is a
   later sub-step — bbg keeps cyberlinks private; the public dims are unscoped.
4. **tests** (`--features bbg`): build a seeded `BbgState` (reuse bbg's `seeded_state`
   shape), assert `BbgSource::scan` rows == the records inserted; assert `graph_stats()`
   == `state.statistics()`. differential: run the same inf query over `LocalSource`
   (same data) and `BbgSource` → identical result sets.

verification after R3b: `RUSTC_BOOTSTRAP=1 cargo test -p inf-source --features bbg`
plus the existing pure suite stays green without the feature.

## R2b — full query-level proof (look openings vs BBG root)  — GATED on `bbg/roadmap/provable-reads.md`

goal: prove a query's column reads as look openings folded into one STARK, verifying in
~constant time against the BBG root. model on `cybergraph/tests/stack_look.rs`.

**precondition:** bbg's look value semantics must be sound first (corner opening +
position/key binding + `ProofLookProvider == BbgLookProvider`). until then the steps below
produce openings whose *value is an MLE fingerprint, not the column* — verifiable but
meaningless as a data read. do NOT ship R2b as a sound query proof before the proposal lands.
the inf-side plumbing can be written and tested against the door, but its soundness is
exactly the door's soundness.

1. **inf-lower: bbg subject builder.** add `bbg_root_obj(o, &state)` producing the real
   4-limb object `[[l0|[l1|[l2|l3]]]|rest]` from `state.root`
   (`goldilocks_from_bytes32`), replacing R1b's all-zero `bbg_obj()` for the proof path.
   axes [4,10,22,23] are read by nox's look pattern (`nox/rs/patterns/look.rs:19`).
2. **inf-lower: entity-keyed look lowering.** add `lf_bbg(o, dim, entity_key_u64)`
   emitting `[17 [[1 dim] [1 key]]]` — the bbg convention (vs R1b's row*ARITY+col).
   the lowered predicate is the committed-scalar filter (e.g. `lt(thresh, look)`).
3. **prove path** (gate `prove` + `bbg`):
   ```
   let prov = bbg::ProofLookProvider::new(&state);
   let mut trace = VecTrace::default();
   reduce(&mut o, subj, formula, BUDGET, &prov, &mut trace);   // Outcome::Ok
   let look_openings = prov.take_look_openings();              // or collect_look_openings(&state,&trace.0)
   let proof = zheng::commit(&trace, &[], &[], &look_openings, &stmt, &params)?;
   zheng::verify(&proof, &stmt, &params)?;
   ```
   `stmt` carries program/input/output hashes (define an inf `Statement` builder; the
   output hash binds the query result). hash_aux/axis_openings stay empty (no Poseidon /
   axis reads in the scalar-predicate class).
4. **value check.** assert the look value bbg returns == the committed scalar inf-eval
   read for the same entity (differential: eval result == proven result).
5. **tests** (`--features prove,bbg`): seed `BbgState`; prove+verify a focus>T neuron
   filter and an energy>T particle filter; assert (a) `verify` passes, (b) the proven
   row set == inf-eval over `BbgSource`, (c) a tampered `stmt`/value fails verify.

verification after R2b:
`RUSTC_BOOTSTRAP=1 cargo test -p inf-lower --features prove,bbg`.

honest scope marker (log in README + remaining.md): R2b is **gated on
`bbg/roadmap/provable-reads.md`** — the proof door currently opens an MLE fingerprint, not
the record column, so NO column proof (headline or otherwise) is sound until the door is
fixed. do NOT claim provable queries until then; R2a (pure computation) is the proof ceiling.

## cozo deletion — recommendation: NOT yet

the question "can we delete cozodb?" — engineering answer: keep it through R2b+R3b,
then delete at stage 4. rationale:

- it costs nothing in the main build: `rs/cozo` is gitignored and the workspace
  *excludes* both `cozo` and `oracle`; CI without the clone runs the pure suite.
- it is the **only external semantics oracle** (`inf-oracle` asserts inf-eval ==
  CozoDB on filter/join/aggregation/closure/negation). R3b/R2b will touch
  source + eval-adjacent semantics — losing the differential net right before that
  work is the wrong time.
- the architecture (memory: stage plan) keeps cozo as oracle *throughout stages 1–3*
  and deletes it at **stage 4** (inf-pure becomes a Trident sub-grammar, proofs
  self-validate). we are at stage 3.
- deleting is trivial and reversible (re-clonable, 8.2M) — no urgency.

**delete condition:** once R2b + R3b land AND the `inf-oracle` suite has run green
against the post-integration eval, drop `rs/cozo` + `rs/oracle` and the `cozo-oracle`
gating in one chore commit. until then, keep.

## sequencing & estimate  (revised 2026-06-08)

```
R3b BbgSource ──► [bbg: provable-reads — fix door soundness + addressing] ──► R2b query proofs
   ~2 sessions        (bbg-side; security review; roadmap proposal first)      ~2 sessions after
```

- **R3b first** — fully unblocked; ships real data reads now. R2b also needs a `BbgState` +
  `BbgSource` to read against, so this is the prerequisite either way.
- **bbg proof-door work next** — gated on `bbg/roadmap/provable-reads.md` being reviewed and
  a direction chosen (sound corner opening + position binding, then addressing A/B/C). this
  is bbg-side protocol work with security review, not inf work.
- **R2b last** — once the door is sound, wire inf trace → `zheng::commit` with real look
  openings → `verify`; differential-test eval result == proven result.

per-step rule (project CLAUDE.md): zero warnings, full suite green, differential
parity (eval == proven) before marking any sub-step done.
