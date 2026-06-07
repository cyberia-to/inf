---
tags: cyber, soft3, inf
crystal-type: entity
crystal-domain: cyber
---
# inf — a whitepaper

inf is the language of a provable graph. you ask the [[cybergraph]] a question
and get an answer; you state a change and the graph takes it. both come with a
[[zheng]] proof that the result follows from the graph's committed state, and
anyone holding the 32-byte graph root checks that proof in microseconds — without
the graph, without trusting the node that ran it.

it is the read-write interface to shared, authenticated knowledge, with proof in
place of trust. SQL gave query and update over a database you had to trust; inf
gives query and update over a graph nobody has to trust.

## two operations

inf does two things over the cybergraph, both set-based, both provable.

- ask — derive a set of facts from committed state. a query returns the set and a
  proof that it follows from the graph at a given root.
- shape — commit a derived set of [[cyberlinks]] as one staked signal. a mass
  update changes the graph and proves it changed exactly the set the rule implies.

a [[signal]] is a set of cyberlinks, and inf is set-based, so one language serves
both: state a pattern, and inf finds the whole set — to return it, or to commit
it. the short pitch is in the repo README; the canonical definition is in specs/.

## what you can ask

every example returns a result and a proof against a [[bbg]] root. the note under
each says what the proof certifies — the part you would otherwise take on faith.

### discovery and ranking

```datalog
?[to, score] := axons{from: #photosynthesis, to}, focus{particle: to, score}
:sort -score
:limit 20
```

proof certifies: these are exactly the top twenty by committed [[focus]], with
nothing higher omitted.

### reachability and paths

```datalog
?[path, cost] <~ ShortestPath(axons{from, to, weight_sum}, #a, #b)
```

proof certifies: a path of the returned length exists and none shorter does.

### provenance and attribution

```datalog
// my own links about #x, in plaintext
?[to, amount, valence] := cyberlinks{neuron: @me, from: #x, to, amount, valence}
```

proof certifies: your committed links and no others; `axons` give the public
aggregate without disclosing any individual cyberlink.

### aggregation and accounting

```datalog
?[neuron, count(to)] := cyberlinks{neuron, to}
```

proof certifies: the count is over the complete committed set — no row silently
dropped.

### structure, similarity, history

communities and bridge particles over the edge set; particles nearest a query
vector (via [[Ten]]); the focus of a particle at a past block (`@ block_n`). each
proof certifies the result against the committed — or the historically
committed — root.

### invariants and policy

```datalog
?[neuron, k] := karma{neuron, k}, lt(k, 0)
:assert none
```

proof certifies: the property holds across the entire committed graph. an empty
result is proven, not sampled — `:assert none` turns "I found no violation" into
"there is no violation." this is how an agent demonstrates it followed a policy
rather than asking to be believed.

## what you can change

the cybergraph is append-only (axiom A3): cyberlinks are never edited or deleted,
and every past state stays queryable. so mutation is the mirror of query as an
append — a rule derives the set of links to add or withdraw, and inf commits that
set as one signal. the proof certifies the appended batch is exactly what the rule
implies over the prior state.

### mass assertion

```datalog
// link every high-focus particle not yet under #topic
?[p] := focus{particle: p, score}, gt(score, T), not axons{from: #topic, to: p}
:link { neuron: @me, from: #topic, to: p }
```

proof certifies: exactly the matching particles were linked.

### mass withdrawal

```datalog
// withdraw my links to particles that fell below a focus floor
?[to] := cyberlinks{neuron: @me, to}, focus{particle: to, score}, lt(score, FLOOR)
:unlink { neuron: @me, to }
```

proof certifies: exactly the stale links were withdrawn. `:unlink` appends a
withdrawal — conviction drops and the live aggregate shifts, while the original
links remain in history.

### migration and curation

a migration links a set to a new target and withdraws conviction from the old; a
merge relinks duplicates to a canonical particle. both are declarative rules that
compile to one signal of appends.

```datalog
// link every axon under #old also to #new, in one signal
?[from] := axons{from, to: #old}, cyberlinks{neuron: @me, from}
:link { neuron: @me, from, to: #new }
```

proof certifies: every matching link was added; the change is complete. a
migration no longer asks you to trust that a script ran correctly — it proves it,
and because the graph is append-only the old state stays auditable beside the new.

### mutable views — filesystem and database

move, rename, delete, update — the mutable verbs of a filesystem or a database —
are notions layered on top of the append-only graph. inf realizes their bulk
forms as appends: supersession (the latest assertion under a name wins),
withdrawal, and tombstones. the layer above sees update and delete; underneath,
the graph only ever grows.

a mutation has two halves. the derivation is pure and provable, in the
Trident-pure subset. the commit is the effect: the derived batch becomes a
signal, staked and signed once by the submitting neuron over the whole batch,
then ordered and validated by cybergraph — the way [[rune]]'s `host` and `hint`
sit outside the pure subset. both halves route through cybergraph → [[bbg]]; inf
never writes a store directly.

## proof in place of trust

an answer is only as trustworthy as the machine that produced it; a bulk update
is a wager that the script did what it claimed. inf removes both gambles.

- ask once, verify forever. answers are portable proofs, re-checked from the root
  and memoized into the graph as reusable facts.
- trustless reads. a phone relies on an answer computed by a datacenter it has
  never met, by checking the proof.
- provable conduct. an agent, contract, or market proves a property of its own
  behavior instead of being audited after the fact.
- provable change. a migration or curation proves it touched exactly the set it
  claimed, before it commits.
- cost up front. `inf cost` reports a cycle ceiling before a query or a change
  runs, so either can be priced and bounded in advance.

## rune and inf — two paradigms, one graph

both [[rune]] and inf read and mutate the cybergraph; the split is paradigm, not
direction.

- rune is imperative — scripted, per-item, event-driven, with `host` calls and an
  identity-bearing kernel. it is the agent's nervous system: react to an event,
  decide, act, loop. it builds a signal one action at a time.
- inf is declarative and set-based — derive a whole set from a pattern, to return
  it or commit it. it is the analytical and migration surface: rank, traverse,
  aggregate, and shape in bulk. it builds a signal as the set a rule implies.

they compose: rune logic can invoke an inf derivation for a bulk step; inf
results feed rune decisions. and they are the right tools for opposite shapes of
work — forcing a bulk migration through rune's loop is as wrong as forcing
reactive agent logic into datalog. relative to [[trident]], inf is the
declarative twin that [[rune]] is the imperative twin of: each language's pure
subset is a literal sub-grammar of Trident.

## the shape of the language

inf is declarative: you state the relationships you want, and the engine finds
the set — whether you return it or commit it. a query is a set of rules; a rule
has a head (what it produces) and a body (the conditions that produce it). rules
that name themselves express recursion, which is how reachability, linkchains,
and transitive closure are written directly rather than unrolled by hand.

this shape fits the cybergraph. a relational language handles tables but fights
recursion; a triple language handles edges but caps traversal. inf handles
recursive traversal, pattern matching, aggregation, graph algorithms, and bulk
mutation in one language, with set semantics — each cyberlink counted once.

values are exact field elements, never floating point, so the same query gives
the same answer on every machine. arithmetic and comparison delegate to the
sibling languages that own each algebra: [[Trident|Tri]] for field math, [[Rs]]
for byte decoding, [[Bt]] for bit tests, [[Ten]] for vector similarity. inf
itself stays pure relation.

## relations are views over the graph

inf owns no storage. the relations you read are views over committed [[bbg]]
state, with canonical schemas in cybergraph/specs/query.md:

- `axons` — the public aggregate weight between two particles, scannable by
  anyone.
- `cyberlinks` — neuron-scoped: a scan returns your own links (in plaintext) and
  links you hold an opening key for; a bare public scan returns nothing.
- `particles`, `neurons`, `focus`, `karma`, `signals` — committed state read at
  the snapshot.

reads resolve at the snapshot; mutations commit as signals. both route through
cybergraph → bbg. inf's own intermediate results live as [[nox]] nouns for the
duration of a query, not in a separate database.

## the three registers

inf acts in three registers, separated by what the proof depends on.

- pure — a question or change against a committed snapshot. unconditional proof.
  the queries above are pure; the derivation half of every mutation is pure.
- reactive — a standing rule that re-fires when the graph changes (`subscribe`).
  the proof is conditional on the event log.
- live — federated across external systems; the external result is a witness.

the boundary is one-way: Rune-pure ⊃ inf-pure ⊃ [[trident]] grammar. reactive
and live inputs never leak into the pure subset; a mutation's staked commit sits
just outside it.

## the proof contract

the pure register is a proof language because it satisfies [[trident]]'s three
constitutional constraints — what makes every answer and every update provable.

1. bounded execution — recursion is bounded fixed-point iteration; a rule whose
   depth cannot be bounded is rejected before it runs.
2. determinism — same rule, same root, identical result on every node. no floats,
   no hash-map order, no clock, no unseeded randomness.
3. statically-known cost — `inf cost` reports a cycle ceiling before execution.

hold all three and the rule lowers query → IR → [[nox]] patterns → [[zheng]] like
any Trident dialect.

## bounded recursion over an unbounded graph

reachability and closure iterate to a fixed point, and the number of rounds
depends on the graph. a [[zheng]] circuit has a depth fixed at compile time. inf
reconciles them with three layers of bound.

- capacity — compile time. the circuit compiles to a maximum depth MAX.
- snapshot bound — proof time. the graph root commits `diameter_bound` (and
  `node_count`, `relation_sizes`), the worst-case round count for this snapshot.
  a committed public input, so the cost is known before the rule runs even though
  it scales with the graph.
- fixpoint — run time. evaluation reaches the real fixed point at round K; a
  convergence witness proves round K added nothing, sizing the proof to K.

"static cost" means computable from committed public inputs before execution, not
constant. a graph query that costs O(graph size) is admissible because the size
is committed. an explicit `:bounded N` caps iterations below the graph size for
rules that need fewer hops.

## what it costs

cost splits across the [[bbg]] boundary, which is why it can be reported up front.
bbg owns the cost of reads (a [[lens]] opening) and the committed sizes
(`node_count`, `relation_sizes`, `max_degree`, `diameter_bound`, in
[[bbg/specs/statistics]]). inf owns the cost of combining reads.

```
inf cost = reads (bbg) + combine (inf) + recursion (bound × per-iteration)
```

every term is a fixed coefficient or a committed statistic, so once the root is
fixed the ceiling is static. the same statistics that bound recursion also cost
it — one interface serves both.

## from rule to result and signal

```
inf rule
  → parse, stratify, plan        which relations, in what order
  → query-plan IR                relational algebra (the Trident TIR analog)
  → nox patterns                 reads → look; recursion → bounded compose
  → zheng                        via the trident → zheng pipeline
```

a read becomes the [[nox]] `look` pattern; recursion becomes a bounded `compose`
loop; conditions call into Tri or Bt. a query's rows each carry a [[lens]] opening
against the root, so a verifier confirms every row came from the committed graph
in constant time. a mutation's derived set is proven the same way, then committed
as a signal.

## the bootstrap

inf starts as CozoDB — vendored in rs/cozo, the way [[trident]] started as Rust —
and shrinks to zero across four stages, each benchmarked against a reference
floor.

1. rules parse, plan, execute via CozoDB. results trusted.
2. rules parse via inf's front end into the query-plan IR, execute via CozoDB.
3. the IR lowers to Nox patterns; execution moves to the Nox interpreter.
4. native Nox execution with [[zheng]] proofs via Trident. CozoDB removed.

the discipline throughout: CozoDB is the scaffold, not the design. inf-pure is
defined by the Trident contract, not by what cozo happens to do.

## place in the fourteen

the cyber stack has fourteen proof languages, each algebraically irreducible. inf
is #10 — Infer, the algebra of relations and inference. it stands beside [[Rs]]
(bytes), [[Trident|Tri]] (field), [[Bt]] (bits), and [[Ten]] (tensors), under
none of them. above the fourteen sit two surfaces: [[rune]] (imperative) and inf
(declarative). both read and mutate the same graph; the split is paradigm, not
direction.

## what it enables

a graph that anyone can both question and evolve, with proofs replacing trust at
every read and every write.

on the read side: a light client governs by checking proofs instead of replaying
history; an agent earns trust by proving its conduct instead of being audited; a
market settles on a provable aggregate instead of a quorum's say-so; and every
proven answer, memoized into the graph, makes the next question cheaper.

on the write side: knowledge evolves provably. a migration, a dialect-driven
rewrite, a mass curation — each proves it touched exactly the set it claimed,
checkable from the root. shared knowledge can be shaped in bulk without a
custodian everyone has to trust to have run the script correctly.

together they make the cybergraph a living, collectively-maintained body of
knowledge: questioned safely, evolved safely, by anyone, from anywhere. ask once,
change once; the planet verifies forever.

## bootstrap reference

the CozoScript surface inf runs on during stages 1-3 is documented here. it
describes present behavior; the canonical language is in specs/.

- [queries](queries.md) — CozoScript syntax: rules, atoms, recursion, aggregation
- [stored relations](stored%20relations.md) — data model: schema, mutations, transactions
- [algorithms](algorithms.md) — fixed rules: PageRank, pathfinding, community detection
- [functions](functions.md) — built-in function reference
- [cybergraph](cybergraph.md) — cybergraph integration and rune interop
