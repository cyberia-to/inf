---
tags: cyber, soft3, inf
crystal-type: entity
crystal-domain: cyber
---
# inf — a whitepaper

inf is the declarative query and inference language of the [[cybergraph]] —
proof language #10 of the fourteen, the twin of [[rune]]. this is the long-form
treatment: why it exists, what it guarantees, and how it works. the short pitch
is in the repo README; the canonical definition is in specs/.

## the problem

the [[cybergraph]] is a directed, weighted, authenticated graph of
content-addressed [[particles]] linked by [[cyberlinks]]. asking questions of it
needs four things at once: recursive traversal (follow [[linkchains]] to
arbitrary depth), pattern matching (find [[motifs]]), aggregation (sum [[karma]],
rank by [[focus]]), and graph algorithms (PageRank, shortest path, community
detection).

a relational language handles tables but fights recursion. a triple language
handles edges but caps traversal. datalog handles all four natively — recursive
Horn clauses with set semantics and a terminating fixed-point. that is why inf
is a datalog.

but a query language for cyber must do one more thing no ordinary datalog does:
it must produce a proof. an answer from the graph is worthless if you have to
trust the node that computed it. inf answers come with a STARK that the result
follows from the committed graph state. this is what makes inf a proof language
rather than a query DSL.

| requirement | SQL | SPARQL | datalog | inf |
|---|---|---|---|---|
| recursive queries | CTEs | limited paths | native | native, bounded |
| graph algorithms | external | external | fixed rules | fixed rules → [[Ten]] |
| aggregation | GROUP BY | GROUP BY | inline | inline, semi-lattice in recursion |
| determinism | no | no | yes | yes, enforced |
| proof of result | no | no | no | STARK against a [[bbg]] root |

## place in the fourteen

the cyber stack has fourteen proof languages, each algebraically irreducible.
inf is #10 — Infer, the algebra of Horn clauses. it stands beside [[Rs]] (bytes),
[[Trident|Tri]] (field), [[Bt]] (bits), and [[Ten]] (tensors), under none of
them. above the fourteen sit two execution surfaces: [[rune]] (imperative) and
inf (declarative). rune constructs and mutates; inf queries and reasons. same
stack position, opposite verb.

the relationship to [[trident]] is the same one [[rune]] has: inf is the
declarative twin. rune is Trident grammar plus three dynamic forms; inf is
Trident grammar expressed as Horn clauses plus the same three dynamic registers.
the pure subset of each is a literal sub-grammar of Trident.

## the three registers

inf, like rune, has three execution registers separated by their proof contract.

### pure (static)

a query over a committed graph snapshot at a block. the result is a function of
(graph_root, query, params). every read is a [[nox]] `look` against the root;
every rule application is a constraint. the whole derivation is a STARK,
unconditional. this is the proof language — the Trident-compatible core.

### reactive

a query that subscribes to graph mutations and re-runs when matching state
changes. this is inf's analog of rune's `hint`. the proof becomes conditional on
the event log: given the sequence of mutation events as witnesses, every
re-evaluation is provable. it cannot be pure Trident because the trigger is an
external event.

### live

a query federated across external systems — other chains, oracles, classical
databases. the analog of rune's `host`. the external result is an asserted
witness; the surrounding derivation is provable given that witness. the trust
boundary is the external system.

the boundary is one-way and strict: Rune-pure ⊃ inf-pure ⊃ Trident grammar.
reactive and live inputs never leak back into the pure subset.

## the proof-language contract

to be a proof language and not merely a query tool, inf-pure satisfies
[[trident]]'s three constitutional constraints. each is a design driver and a
place where ordinary datalog had to be narrowed.

1. bounded execution — recursion is bounded fixed-point iteration. no general
   recursion. a rule whose depth cannot be bounded is rejected at compile time.
2. deterministic over a snapshot — the same query and the same graph root
   produce identical results on every node. no floats, no hash-map iteration
   order, no clock, no unseeded randomness.
3. statically-known cost — `inf cost` reports a cycle ceiling before the query
   runs.

hold all three and an inf program lowers query → IR → [[nox]] patterns → STARK
like any Trident dialect.

## bounded recursion over an unbounded graph

recursion is the reason for datalog, and the hardest part of the proof contract.
reachability, linkchains, and transitive closure iterate until they reach a
fixed point, and the number of rounds depends on the graph — its diameter, its
size. a reachability query over a graph of N particles may need up to N rounds.

a STARK circuit, though, has a depth fixed at compile time. these two facts look
like they collide: how do you compile a fixed circuit for a query whose
iteration count grows with the graph? inf resolves it with three layers of
bound.

- capacity — compile time. the circuit compiles to a maximum depth MAX, the
  largest graph the circuit can serve. this is the fixed bound the STARK needs.
- snapshot bound — proof time. the graph root commits to the graph's size
  (node_count) and, where available, its diameter. that committed value is the
  worst-case iteration count for this snapshot. because it is a committed public
  input, the cost is known before the query runs — static — even though it
  scales with the graph. a query whose snapshot bound exceeds MAX is rejected;
  it needs a larger circuit.
- fixpoint — run time. semi-naive evaluation reaches the actual fixed point at
  round K, where K ≤ diameter ≤ node_count. a convergence witness proves that
  round K added nothing, so the produced proof is sized to K, not to MAX.

the consequence is the right one on every axis: the circuit depth is fixed
(constraint 1 holds), the cost scales with the committed graph size (graph
queries depend on graph size, and the dependence is static because the size is
committed), and the prover pays only for the real depth K.

"static cost" therefore means computable from committed public inputs before
execution. it does not mean constant. a graph query that costs O(graph size) is
admissible, because the graph size is committed.

an explicit `:bounded N` lets a programmer cap iterations below the graph size —
"only three hops" — which shrinks both the cost and the circuit capacity. this
is the deliberate special case. the default bound for a graph query is the graph
itself.

## static cost is the bbg ↔ inf interface

a query's work is reads plus combination plus recursion. each piece lives on one
side of the [[bbg]] boundary.

- bbg owns the reads and the sizes: the cost of a [[lens]] opening, and the
  committed statistics (relation sizes, node_count, diameter). reads are easy
  and their cost is known.
- inf owns the combination: the join, filter, aggregate, and sort coefficients,
  and the bounded iteration count.

`inf cost = reads (bbg) + combine (inf) + recursion (bound × per-iteration)`.
every term is a fixed coefficient or a committed statistic, so the ceiling is
static once the root is fixed. this places one requirement on bbg: it must
commit the statistics in the graph root, or the cost — and the recursion bound
that consumes node_count and diameter — would be trusted rather than proven.
that commitment is being added to bbg. the same committed statistics that bound
recursion also cost it: one interface serves both.

## the value model

inf values are [[nox]] atoms — the three modes of reference: field (an exact
Goldilocks element), word (a 32-bit value), hash (a 256-bit digest, particle
identity). higher shapes are cons-structure over these.

numbers are exact field elements. inf has no floating point — [[focus]], karma,
and weights are field elements as committed in bbg, read directly. arithmetic
and comparison are not reimplemented in inf; they delegate to the sibling
languages: [[Trident|Tri]] for field math in conditions and aggregates, [[Rs]]
for byte decode of particle content, [[Bt]] for bit tests on tags, [[Ten]] for
vector similarity and kNN. inf itself is pure relation. this keeps inf small and
makes every arithmetic step provable through the sibling that owns that algebra.

## relations are views over committed state

inf does not own storage. base relations are views over committed bbg state,
with canonical schemas in cybergraph/specs/query.md. visibility follows the
privacy model:

- `axons` — the public aggregate, scannable by anyone.
- `cyberlinks` — neuron-scoped: a scan returns the caller's own links
  (plaintext) and links the caller holds an opening key for. a bare public scan
  returns nothing. axons answer "what does the graph say"; cyberlinks answer
  "what did I assert."
- `particles`, `neurons`, `signals`, `focus`, `karma` — committed state read at
  the snapshot.

inf never writes canonical relations; mutations flow rune → cybergraph → bbg.
query intermediates are [[nox]] nouns in the noun store, not a separate inf
database. inf owns no persistent state.

## lowering: query to proof

```
inf query
  → parse + stratify + magic-set rewrite      (which relations, in what order)
  → query-plan IR                              (relational algebra; the Trident TIR analog)
  → nox patterns                               (joins/scans → look; recursion → bounded compose)
  → STARK                                      (via the trident → zheng pipeline)
```

relation reads become the `look` pattern (deterministic bbg read). recursion
becomes a bounded `compose` loop. conditions become calls into Tri/Bt. the
result set carries [[lens]] openings against the graph root, so a verifier
checks "every returned row came from the graph committed by this root" in
constant time.

## the bootstrap

inf does not start as a Nox compiler. it starts as CozoDB, the way [[trident]]
started as Rust. CozoDB is vendored in rs/cozo and shrinks to zero across four
stages, each publishing a benchmark (cycles, proof size, verification time)
against a reference floor.

1. queries parse, plan, and execute via CozoDB. results trusted.
2. queries parse via the inf front end into the query-plan IR, execute via
   CozoDB.
3. the query-plan IR lowers to Nox patterns; execution moves to the Nox
   interpreter.
4. native Nox execution with STARK proofs via Trident. CozoDB removed.

the anti-pattern to avoid throughout: treating CozoDB's executor semantics as
the design. CozoDB is the scaffold, not the building. inf-pure is defined by the
Trident contract, not by what cozo happens to do.

## what this enables

an inf answer is a portable proof. a light client with only a 32-byte graph root
can ask "top 100 particles by focus" and verify the answer in microseconds
without holding the graph. an agent can prove it followed a policy — "I only
linked particles above this karma threshold" — rather than asking to be trusted.
and because every proven computation is memoized into the graph as a
[[cyberlink]], the answer becomes a reusable fact: ask once, and the whole
network can verify forever.

## bootstrap reference

the CozoScript surface inf runs on during stages 1-3 is documented here. it
describes present behavior; the canonical language is in specs/.

- [queries](queries.md) — CozoScript syntax: rules, atoms, recursion, aggregation
- [stored relations](stored%20relations.md) — data model: schema, mutations, transactions
- [algorithms](algorithms.md) — fixed rules: PageRank, pathfinding, community detection
- [functions](functions.md) — built-in function reference
- [cybergraph](cybergraph.md) — cybergraph integration and rune interop
