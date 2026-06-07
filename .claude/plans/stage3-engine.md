# stage 3 engine — implementation plan

status: draft for approval
date: 2026-06-07
goal: reach bootstrap stage 3 — an inf query lowers to [[nox]] patterns and runs
on the nox interpreter, matching a reference, for the pure register.

## decision: write from scratch, cozo as oracle

not fork-and-strip. inf's engine is written fresh directly in inf/rs, alongside
the vendored rs/cozo, which is kept only as a differential oracle.

why not adapt cozo-core in place:

- the end state deletes cozo (stage 4). investment in adapting cozo internals is
  throwaway; fresh code is not.
- the spec's core demands are deep, not surface — they touch cozo's value model,
  eval loop, and storage layer at once: field-element values (cozo's DataValue is
  float-laden), bounded recursion (cozo runs to convergence), IR → nox lowering
  (cozo has no nox), a RelationSource boundary (cozo owns storage), append-only
  signal mutation (cozo does db writes). a fresh engine aligned to nox from day
  one is cleaner than fighting cozo's assumptions.
- the algorithms we keep (semi-naive evaluation, magic-set rewriting,
  stratification, the graph algos) are well-understood; cozo gives us a correct
  reference to port and differential-test against, without code entanglement.

this matches the stack pattern (nika, the warriors): full Rust reimplementation,
the upstream kept as a reference, not a dependency.

## reframing the staging

bootstrap.md lists stage 2 as "CozoDB over inf IR." from-scratch refines this:
cozo is the differential ORACLE throughout, never an executor we route IR into.
two reference implementations converge (cyber multi-dimensional verification):

- cozo — external oracle, checked on the integer/bounded overlapping subset.
- eval — our Rust evaluator, the canonical-semantics reference.
- lower — IR lowered to nox, run on the nox interpreter. stage 3. must match
  eval bit-for-bit; cross-checked against cozo where semantics overlap.

(small update to bootstrap.md's stage-2 row to follow once approved.)

## crate layout (rs/)

a cargo workspace at inf/rs, mirroring rune's crate map. directories are plain —
no inf- prefix (we are already in the inf repo). rs/cozo stays gitignored and is
excluded from the workspace (it carries its own).

```
inf/rs/                 cargo workspace (excludes cozo/)
  Cargo.toml
  value/                DataValue over nox atoms (field/word/hash); memcmp; temp store
  ast/                  AST (rules, atoms, terms) + IR (relational-algebra plan)
  lex/                  tokenizer for the inf surface
  parse/                parser → AST (the pure register, grammar.md)
  plan/                 AST → IR: stratify, magic-set, compile to RA (ported from cozo)
  source/               RelationSource trait + LocalSource (in-mem); BbgSource later
  eval/                 semi-naive evaluator over IR + RelationSource (the reference)
  lower/                IR → nox-pattern lowering; runs on the nox interpreter
  cli/                  the `inf` binary: run / plan / cost / check
  cozo/                 (gitignored) — oracle, behind a feature flag
```

package names may carry an `inf-` prefix for crate identity; the directories stay
plain. dependencies: nox/rs (interpreter, for `lower`), honeycrisp (unimem temp
store, when the arena lands), strata/Tri for field ops. cozo oracle tests are
gated behind a `cozo-oracle` feature requiring the local clone, so CI without
cozo runs the `eval` differential suite only.

## what to port vs build fresh

| concern | source | how |
|---|---|---|
| lexer / parser | fresh | inf grammar (grammar.md), not CozoScript; cozo's pest grammar as a reference |
| stratification | port cozo query/stratify.rs | re-implement over our IR |
| magic-set rewrite | port cozo query/magic.rs | re-implement over our IR |
| relational-algebra plan | port cozo query/ra.rs + compile.rs | our IR types |
| semi-naive eval | port cozo query/eval.rs | bounded; over RelationSource, not StoreTx |
| graph algorithms | port cozo fixed_rule/algos/* | arithmetic → Ten/Tri; bounded iteration counts |
| value model | fresh | field/word/hash atoms, no float (`value`) |
| storage | fresh | RelationSource + temp store; no cozo storage layer (`source`) |
| nox lowering | fresh | the novel work (`lower`) |

## milestones

a thin vertical slice reaches stage 3 first; breadth follows.

M0 — scaffold. workspace, crates, CI, cozo behind the `cozo-oracle` feature. a
query corpus harness that runs a query through {cozo, eval, lower} and
diffs results.

M1 — value + source + temp store. `value` (atoms, memcmp), `source` with an
in-mem LocalSource, a temp store (std BTree arena first; unimem arena later).

M2 — front-end. `lex` + `parse` for the pure register → AST. parse every example
in the specs; snapshot-test the AST.

M3 — planner. `plan`: stratify → magic-set → IR. port from cozo, target our IR.

M4 — `eval` (the reference). semi-naive evaluation over IR + LocalSource: joins,
filters (Tri/Bt calls), projection, aggregation, bounded recursion, the
fixed-rule algos. differential-test vs cozo on the integer/bounded corpus. at
this milestone inf runs end-to-end interpretively.

M5 — `lower` (STAGE 3). lower IR → nox patterns (reads → look against the source,
join → compose, condition → Tri/Bt under branch, recursion → bounded compose,
sort/limit → comparison+truncate). run on the nox interpreter. differential-test
vs `eval` (must match) and cozo (overlap). stage 3 lands.

### the vertical slice (drives M0–M5)

```datalog
?[to, score] := axons{from: #seed, to}, focus{particle: to, score}, gt(score, T)
:sort -score
:limit 20
```

two-relation join + a Tri condition + sort + limit, no recursion, over a
LocalSource fixture. once this runs on the nox interpreter and matches eval,
the path exists; then broaden to bounded recursion (reachability), aggregation
(count/sum), a fixed rule (PageRank), and mutation derivation (`:link` → the
cyberlink batch a signal would carry).

## verification — the scoreboard

per cyber/engineering and bootstrap.md: each milestone publishes correctness and,
from M5, cost. references/ holds the ground-truth corpus (query + expected rows);
the harness asserts cozo == eval == lower on every corpus entry within the
shared semantics. a milestone lands only when its diff is clean.

## risks

- semi-naive + magic-set + stratification are subtle. mitigation: port faithfully
  from cozo and differential-test every step against it.
- nox `look` over a non-bbg source. mitigation: M5 uses a look-handler over the
  LocalSource; bbg wiring is a later source, not a stage-3 blocker.
- field-element vs cozo float divergence narrows the shared corpus. mitigation:
  eval is the primary reference; cozo is a secondary check on integer queries.

## sequencing (sessions, 3h each)

M0 ~0.5 · M1 ~1 · M2 ~1.5 · M3 ~2 · M4 ~3 · M5 ~3 → roughly 9–11 sessions to
stage 3. M4 (`eval`) is the heaviest and the highest-value: it makes inf real and
is the reference stage 3 must match.
