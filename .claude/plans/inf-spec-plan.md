# inf spec plan (v2 — Trident-centric)

status: draft for approval
date: 2026-06-07
supersedes: v1 (fork-centric — treated CozoDB as the design; wrong)

## what inf is

INF is proof language #10 of 14 (Infer — Horn clauses, relation/inference).
the declarative TWIN OF RUNE; sibling of Rs/Tri/Bt/Ten, under none of them.
where Rune mutates the cybergraph, INF queries and reasons over it.

CozoDB is to INF what Rust is to Trident: a bootstrap that SHRINKS to zero.
the end-state is not "forked CozoDB" — it is a proof language whose pure
subset is a sub-grammar of Trident, lowering query → IR → Nox patterns →
zheng via the Trident pipeline. do not carry CozoDB's executor semantics into
the language design.

## the Trident contract (drives every decision)

to be a PROOF language, INF-pure must satisfy Trident's three constraints.
each is also where cozo diverges and must be redesigned, not ported:

1. bounded execution — every Horn clause has a static depth bound; recursion
   is bounded fixed-point iteration, no general recursion. cozo TERMINATES
   but is not STATICALLY bounded → add explicit or inferable depth bounds.
2. deterministic over a fixed snapshot — same query + same graph root →
   identical result on every node. kill float (delegate to Tri), kill
   hashmap-order leakage, seed any randomness.
3. statically-known cost — `inf cost query.inf` reports cycles before
   execution (mirrors Trident `--cost`). cozo's planner is runtime-adaptive →
   build a static cost model over query shape + graph statistics.

if all three hold, an INF program lowers through IR → Nox → zheng like any
Trident dialect.

## registers (mirror Rune's pure / reactive / host)

| register | data source | proof | note |
|---|---|---|---|
| pure / static | committed BBG-root snapshot (Nox `look` + Lens) | unconditional zheng | the Trident-compatible core; pure subset = Trident sub-grammar |
| reactive | cybergraph `subscribe` (mutation events) | conditional on event log | analog of Rune `hint`; needs own opcode (`watch`?) |
| live | external / federated / content via radio | witness-based | analog of Rune `host` |
| (bootstrap) | cozo-local | none | NOT a language register — dev/test harness; shrinks away |

the old "modes" (direct/bbg/cybergraph) fold into this: bbg = the static
register's raw committed relations; cybergraph adds (a) derived/normalized
static relations (still provable functions of the root), (b) the reactive
`subscribe` layer, (c) external content. cybergraph is where "control over
what and how is added" lives (the five verbs + validation). direct = the
bootstrap fixture. privacy unchanged: `axons` public, `cyberlinks`
neuron-scoped (own plaintext ∪ keyed-in).

## value model (replaces the fixed-point migration)

INF values are Nox atoms: field / word / hash. numbers are exact field
elements (Goldilocks F_p). NO floats anywhere. φ* is already an 8-byte field
element in BBG — read directly.

arithmetic is DELEGATED to siblings, not reimplemented:
- Tri — field math in WHERE clauses, comparisons, aggregates
- Rs  — byte-level decode of particle content
- Bt  — bitwise tests on tags
- Ten — vector-similarity / kNN (HNSW becomes a Ten concern)

INF itself is pure relation/Horn-clause. graph algorithms (pagerank,
dijkstra) become field-arithmetic iterations with STATIC iteration counts
(constraint #1), delegating the arithmetic to Tri/Ten. the "float problem"
dissolves: field elements + Tri, exact and provable.

## storage (resolves the BBG/temp-store debate)

end-state: INF owns ZERO storage.
- query intermediates are Nox nouns in the noun store (honeycrisp unimem) —
  allocation is `cons`, no heap.
- canonical reads are Nox `look` + Lens openings against a BBG root.

bootstrap stages 1-3: keep cozo's mem temp store — it is throwaway, deleted
at stage 4. do NOT extend BBG ephemeral for INF's scratch; INF's scratch
ceases to exist. BBG ordered-ephemeral (old option B) is justified only by
other consumers (tru, …), never by INF.

## bootstrap migration (4 stages, scoreboard discipline)

mirror Trident self-hosting. each stage publishes a benchmark: cycles, proof
size, verification time. references/ = ground truth, baselines/ = hand floor.

1. queries parse + plan + execute via CozoDB. results trusted.
2. parse via INF lexer/parser → query-plan IR → execute via CozoDB.
3. query-plan IR → Nox patterns (joins/scans/bounded fixpoints as Nox loops)
   → execute on the Nox interpreter.
4. native Nox execution + zheng via Trident. CozoDB removed.

## spec set (mirror trident/ and rune/ specs/ — INF is a sibling language)

write to inf/specs/ (matches the actual trident & rune repos; the relation
brief said reference/, but those repos use specs/ — flagged).

1. specs/language.md — syntax, types, query operators; the pure subset that
   is Trident-equivalent; value model (field/word/hash, no float).
2. specs/grammar.md — formal grammar; mark which productions are shared with
   Trident's grammar.
3. specs/cost.md — static cost model: cycles from query shape + graph stats.
   NEW; Trident constraint #3.
4. specs/proof.md — proof contract: result certifies against a BBG root;
   Lens opening for a multi-row result set; derivation-trace → zheng.
5. specs/ir.md — query-plan IR + Nox-pattern lowering (the TIR analog).
   merges v1's ir + circuit.
6. specs/extensions.md — reactive (`subscribe`) + live; their proof contracts
   (event-log-relative, witness-based). mirror rune hint/host structure.
7. specs/interop.md — calling Tri/Rs/Bt/Ten; being called from Rune-pure; the
   one-way boundary (Rune-pure ⊃ INF-pure ⊃ Trident grammar; no
   non-determinism smuggled back).
8. specs/relations.md — relation model: views over committed bbg state,
   visibility/privacy scopes, snapshot read, append-only mutation surface, temp
   relations, mutable fs/db views on top. (canonical rewrite of docs/stored
   relations + part of docs/cybergraph.)
9. specs/functions.md — builtin reference; field-element semantics, arithmetic
   delegated to Tri/Rs/Bt/Ten. (canonical rewrite of docs/functions.)
10. specs/algorithms.md — fixed-rule catalog (PageRank, Dijkstra, Louvain, …);
   lowers arithmetic to Ten; bounded. (canonical rewrite of docs/algorithms.)
11. specs/bootstrap.md — the 4-stage CozoDB shrink + benchmark scoreboard;
   summarizes the cozo surface (CozoScript syntax, :put/%if) rather than
   duplicating it as graph pages.
12. specs/props/query-modes.md — recast as data-source ↔ register mapping
   (already drafted; privacy resolved).

reference belongs in specs/, not docs/ — queries/functions/algorithms/stored-
relations are reference (canonical), not explanation. the five cozo pages in
docs/ are superseded as their specs/ counterparts land: queries → language +
grammar; stored relations → relations; functions → functions; algorithms →
algorithms; cybergraph → relations + interop. docs/ keeps the whitepaper
(README) plus future tutorials/guides. cozo-specifics live in rs/cozo +
bootstrap.md.

dropped from v1: arithmetic.md (→ Tri), vector.md (→ Ten/interop + algorithms),
transactions.md (bootstrap-only → bootstrap.md).

## read-first before writing specs (brief's list, paths corrected)

- trident/specs/{language,grammar,ir}.md — what Trident accepts + its IR
- rune/specs/README.md — DONE (pure/reactive/host precedent)
- nox/README.md + nox/patterns/ — the patterns INF lowers to (16 compute +
  call + look)
- bbg/ + lens/ — the `look`/Lens commitment+opening primitives
- cyb/root/languages.md — DONE (the 14, INF = #10)

## open decisions (Trident-forced)

1. bounded-recursion surface: explicit depth parameter (cozo-style
   `chain[p, depth], depth < N`) vs static inference from the rule shape.
2. static cost with recursion: worst-case bound needs a graph-size parameter;
   how is it surfaced to `inf cost`.
3. naming: specs/ (match trident/rune) vs reference/ (brief). lean specs/.
4. how much of cozo's Imperative/Sys layers survive the bootstrap.

## write order

overview/language + grammar first (1,2) → cost + proof (3,4) →
ir/Nox-lowering (5) → extensions + interop (6,7) → bootstrap (8). recast
modes doc (9) alongside language.
