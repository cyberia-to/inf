---
tags: cyber, soft3, inf
crystal-type: entity
crystal-domain: cyber
---
# inf — a whitepaper

inf is the language you use to ask the [[cybergraph]] questions and get answers
you can prove. you describe what you want to know; inf returns the result with a
[[zheng]] proof that it follows from the graph's committed state. anyone holding
the 32-byte graph root can verify the answer in microseconds — without the
graph, without trusting the node that computed it. the same set-based language
also updates the graph in bulk, committing mass mutations as provable signals.

this document is about what that buys you: the questions inf answers, why a
proven answer is worth more than an answer, and how the language delivers it. the
short pitch is in the repo README; the canonical definition is in specs/.

## what you can ask

every example below returns a result and a proof against a [[bbg]] root. the
note under each says what the proof certifies — the part you would otherwise
have to take on faith.

### discovery and ranking

```datalog
// the particles a query particle points to, ranked by focus
?[to, score] := axons{from: #photosynthesis, to}, focus{particle: to, score}
:sort -score
:limit 20
```

proof certifies: these are exactly the top twenty by the committed [[focus]]
values, and nothing ranking higher was left out.

### reachability and paths

```datalog
// shortest linkchain between two particles
?[path, cost] <~ ShortestPath(axons{from, to, weight_sum}, #a, #b)
```

```datalog
// is #goal reachable from #seed through links at all?
reachable[p] := axons{from: #seed, to: p}
reachable[p] := reachable{mid}, axons{from: mid, to: p}
?[] := reachable{#goal}
:assert some
```

proof certifies: a path of the returned length exists and none shorter does; or,
for the reachability check, that a connecting chain exists.

### provenance and attribution

```datalog
// what did I assert about #x — my own links, in plaintext
?[to, amount, valence] := cyberlinks{neuron: @me, from: #x, to, amount, valence}
```

```datalog
// how much conviction does the graph as a whole hold on the axon #a → #b
?[weight_sum, link_count] := axons{from: #a, to: #b, weight_sum, link_count}
```

proof certifies: the first answer is your committed links and no others; the
second is the public aggregate over every contributor, with no individual
[[cyberlink]] disclosed. `axons` answer "what does the graph say"; `cyberlinks`
answer "what did I assert" — and only for links you hold the key to.

### aggregation and accounting

```datalog
// top neurons by karma
?[neuron, k] := karma{neuron, k}
:sort -k
:limit 10
```

```datalog
// how many particles each neuron has linked
?[neuron, count(to)] := cyberlinks{neuron, to}
```

proof certifies: the sum, count, or ranking is taken over the complete committed
set — no row silently dropped to change the total.

### structure and motifs

```datalog
// communities of densely interlinked particles
?[particle, community] <~ Communities(axons{from, to, weight_sum})
```

```datalog
// co-citation: pairs many neurons link the same way
?[from, to, n] := cyberlinks{neuron, from, to}, n = count(neuron)
?[from, to, n] := above, gt(n, 3)
```

proof certifies: the communities, bridges, or co-citation counts are the ones
implied by the committed edge set. graph algorithms run as fixed rules and lower
their arithmetic to [[Ten]].

### similarity

```datalog
// particles whose embedding is nearest a query vector
?[particle, d] := embeddings{particle, vec},
                  d = Ten.cos_dist(vec, query)
:sort d
:limit 10
```

proof certifies: these are the nearest particles by the committed embeddings,
under the stated distance.

### history

```datalog
// the focus of a particle as of an earlier block
?[score] := focus{particle: #x, score} @ block_n
```

proof certifies: the value as it was committed at that past root — the time
dimension makes any historical state queryable and provable.

### invariants and policy

```datalog
// graph-wide invariant: no neuron carries negative karma
?[neuron, k] := karma{neuron, k}, lt(k, 0)
:assert none
```

```datalog
// an agent proves it obeyed its own rule: only linked particles above a floor
?[p] := cyberlinks{neuron: @me, to: p}, focus{particle: p, score}, lt(score, FLOOR)
:assert none
```

proof certifies: the property holds across the entire committed graph. an empty
result is proven, not sampled — `:assert none` turns "I found no violation" into
"there is no violation." this is how an agent demonstrates it followed a policy
rather than asking to be believed.

## why a proof changes the answer

a plain query answer is only as trustworthy as the machine that produced it. an
inf answer carries its own evidence, and that changes what you can do with it.

- ask once, verify forever. the proof is portable. a light client with the
  32-byte root checks it in microseconds, never holding the graph. and because
  every proven computation is memoized into the graph as a [[cyberlink]], the
  answer becomes a reusable fact — ask once, and the whole network verifies
  forever.
- trustless answers. you do not trust the node that ran the query; you check the
  proof. a phone can rely on an answer computed by a datacenter it has never met.
- provable compliance. an agent, a contract, or a market can prove a property of
  its own behavior — "every link I made cleared this karma bar", "this total is
  complete" — instead of being audited after the fact.
- cost known in advance. `inf cost` reports a cycle ceiling before the query
  runs, so you can price and bound a question before asking it.

## the shape of the language

inf is declarative: you state the relationships you want derived, and the engine
finds them. a query is a set of rules. a rule has a head (what it produces) and a
body (the conditions that produce it); rules that name themselves express
recursion, which is how reachability, linkchains, and transitive closure are
written directly rather than unrolled by hand.

this shape is chosen for the cybergraph. a relational language handles tables but
fights recursion; a triple language handles edges but caps traversal. inf
handles recursive traversal, pattern matching, aggregation, and graph algorithms
in one language, with set semantics — each [[cyberlink]] counted once.

values are exact field elements, never floating point, so the same query gives
the same answer on every machine. arithmetic and comparison are not built into
inf; they delegate to the sibling languages that own each algebra: [[Trident|Tri]]
for field math, [[Rs]] for byte decoding of particle content, [[Bt]] for bit
tests on tags, [[Ten]] for vector similarity. inf itself stays pure relation.

## relations are views over the graph

inf does not own storage. the relations you query are views over committed
[[bbg]] state, with canonical schemas in cybergraph/specs/query.md:

- `axons` — the public aggregate weight between two particles, scannable by
  anyone.
- `cyberlinks` — neuron-scoped: a scan returns your own links (in plaintext) and
  links you hold an opening key for; a bare public scan returns nothing.
- `particles`, `neurons`, `focus`, `karma`, `signals` — committed state read at
  the snapshot.

inf reads these at the snapshot and mutates them in bulk (see mutation below).
its own intermediate results live as [[nox]] nouns for the duration of a query,
not in a separate database; canonical mutations route through cybergraph → bbg
as signals, never a direct store write.

## mutation — declarative mass update

inf reads in bulk and writes in bulk. a [[signal]] is a staked, signed batch of
[[cyberlinks]], and inf is set-based — so it is the natural way to build one:
derive the whole set of links a rule implies over the current state, then assert
it (`:link`) or retract it (`:unlink`) as a single signal.

```datalog
// relink every axon under #old to #new, in one signal
stale[from] := axons{from, to: #old}, cyberlinks{neuron: @me, from}
?[from] := stale[from]
:link { neuron: @me, from, to: #new }
```

this is inf's complement to [[rune]]. rune mutates imperatively — one scripted,
reactive action at a time, the agent's nervous system. inf mutates declaratively
— the entire matching set at once, the migration and mass-update tool. forcing a
bulk update through rune's loop is as wrong as forcing reactive agent logic into
datalog.

a mutation has two halves. the derivation is pure and provable: the proof
certifies the committed batch is exactly what the rule implies over the prior
state — complete, nothing extra. provable bulk mutation lets a migration show it
touched precisely the right links. the commit is the effect: the batch becomes a
signal, staked and signed once by the submitting neuron, ordered and validated by
cybergraph. read-only queries stay unconditionally provable; the derivation of an
update joins them in the pure subset, while the commit sits outside it, like
rune's `host` and `hint`. both surfaces write through cybergraph → bbg; neither
touches a store directly.

## the three registers

inf answers three kinds of question, separated by what the proof depends on.

- pure — a question about a committed snapshot at a block. the answer is a
  function of (graph root, query, params) and the proof is unconditional. every
  example above is pure.
- reactive — a standing question that re-answers when the graph changes
  (`subscribe`). the proof is conditional on the event log: given the sequence of
  mutations as witnesses, each re-evaluation is provable.
- live — a question federated across external systems (other chains, oracles,
  classical databases). the external result is a witness; the surrounding
  derivation is provable given it.

the boundary is one-way: Rune-pure ⊃ inf-pure ⊃ [[trident]] grammar. reactive
and live inputs never leak back into the pure subset.

## the proof contract

the pure register is a proof language because it satisfies [[trident]]'s three
constitutional constraints. they are what make every answer above provable.

1. bounded execution — recursion is bounded fixed-point iteration; a query whose
   depth cannot be bounded is rejected before it runs.
2. determinism — same query, same root, identical answer on every node. no
   floats, no hash-map order, no clock, no unseeded randomness.
3. statically-known cost — `inf cost` reports a cycle ceiling before execution.

hold all three and the query lowers query → IR → [[nox]] patterns → [[zheng]]
like any Trident dialect.

## bounded recursion over an unbounded graph

reachability and closure iterate until they reach a fixed point, and the number
of rounds depends on the graph — its diameter, its size. a [[zheng]] circuit,
though, has a depth fixed at compile time. inf reconciles them with three layers
of bound.

- capacity — compile time. the circuit compiles to a maximum depth MAX, the
  largest graph it can serve.
- snapshot bound — proof time. the graph root commits `diameter_bound` (and
  `node_count`, `relation_sizes`), the worst-case round count for this snapshot.
  because it is a committed public input, the cost is known before the query runs
  even though it scales with the graph. a query whose snapshot bound exceeds MAX
  needs a larger circuit.
- fixpoint — run time. evaluation reaches the real fixed point at round K; a
  convergence witness proves round K added nothing, so the proof is sized to K.

"static cost" therefore means computable from committed public inputs before
execution, not constant. a graph query that costs O(graph size) is admissible
because the size is committed. an explicit `:bounded N` caps iterations below the
graph size — "only three hops" — for queries that genuinely need fewer.

## what an answer costs

the cost of a query splits cleanly across the [[bbg]] boundary, which is why it
can be reported up front. bbg owns the cost of reads (a [[lens]] opening) and the
committed sizes (`node_count`, `relation_sizes`, `max_degree`, `diameter_bound`,
in [[bbg/specs/statistics]]). inf owns the cost of combining reads — the join,
filter, aggregate, and sort coefficients, and the bounded iteration count.

```
inf cost = reads (bbg) + combine (inf) + recursion (bound × per-iteration)
```

every term is a fixed coefficient or a committed statistic, so once the root is
fixed the ceiling is static. the same committed statistics that bound recursion
also cost it — one interface serves both.

## from question to proof

```
inf query
  → parse, stratify, plan        which relations, in what order
  → query-plan IR                relational algebra (the Trident TIR analog)
  → nox patterns                 reads → look; recursion → bounded compose
  → zheng                        via the trident → zheng pipeline
```

a relation read becomes the [[nox]] `look` pattern; recursion becomes a bounded
`compose` loop; conditions become calls into Tri or Bt. each returned row carries
a [[lens]] opening against the graph root, so a verifier confirms "every row came
from the graph committed by this root" in constant time, whatever the query did.

## the bootstrap

inf does not start as a Nox compiler. it starts as CozoDB — vendored in rs/cozo,
the way [[trident]] started as Rust — and shrinks to zero across four stages,
each benchmarked (cycles, proof size, verification time) against a reference
floor.

1. queries parse, plan, and execute via CozoDB. results trusted.
2. queries parse via inf's front end into the query-plan IR, execute via CozoDB.
3. the IR lowers to Nox patterns; execution moves to the Nox interpreter.
4. native Nox execution with [[zheng]] proofs via Trident. CozoDB removed.

the discipline throughout: CozoDB is the scaffold, not the design. inf-pure is
defined by the Trident contract, not by what cozo happens to do.

## place in the fourteen

the cyber stack has fourteen proof languages, each algebraically irreducible.
inf is #10 — Infer, the algebra of relations and inference. it stands beside
[[Rs]] (bytes), [[Trident|Tri]] (field), [[Bt]] (bits), and [[Ten]] (tensors),
under none of them. above the fourteen sit two surfaces: [[rune]] (imperative)
and inf (declarative). both read and mutate the same graph; the split is
paradigm, not direction — rune scripts per-item effects, inf derives and
mass-updates sets. the relationship to [[trident]] is the one [[rune]] has — inf
is the declarative twin, its pure subset a literal sub-grammar of Trident.

## what it enables

an inf answer is a portable proof, and that compounds. a light client governs by
checking proofs instead of replaying history. an agent earns trust by proving its
conduct instead of being audited. a market settles on a provable aggregate
instead of a quorum's say-so. and every proven answer, memoized into the graph,
makes the next question cheaper — the more the graph is asked, the more it
already knows. ask once; the planet verifies forever.

## bootstrap reference

the CozoScript surface inf runs on during stages 1-3 is documented here. it
describes present behavior; the canonical language is in specs/.

- [queries](queries.md) — CozoScript syntax: rules, atoms, recursion, aggregation
- [stored relations](stored%20relations.md) — data model: schema, mutations, transactions
- [algorithms](algorithms.md) — fixed rules: PageRank, pathfinding, community detection
- [functions](functions.md) — built-in function reference
- [cybergraph](cybergraph.md) — cybergraph integration and rune interop
```

