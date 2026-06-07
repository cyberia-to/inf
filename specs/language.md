---
tags: cyber, soft3, inf
crystal-type: spec
crystal-domain: cyber
---
# inf language

the canonical definition of [[inf]] — syntax, types, value model, rules, and
the pure subset that is [[trident]]-equivalent. grammar in [grammar](grammar.md),
cost in [cost](cost.md).

inf is a Horn-clause language over the [[cybergraph]]. a program is a set of
rules; evaluation derives a relation from committed graph state and returns it
with a proof that the result follows from the state.

## the three registers

inf has three execution registers, mirroring [[rune]]'s pure/reactive/host.
this file specifies the pure register. the other two are in
[extensions](extensions.md).

| register | reads | proof | trident |
|---|---|---|---|
| pure (static) | committed snapshot at a graph root | unconditional zheng | the sub-grammar |
| reactive | `subscribe` to mutations | conditional on event log | outside |
| live | external / federated / content | witness-based | outside |

the pure register is the proof language. it is a function of (graph_root,
query, params). it satisfies the three constitutional constraints and lowers to
[[nox]] patterns.

## value model

inf values are [[nox]] atoms — the three modes of reference:

| atom | representation | use |
|---|---|---|
| field | one Goldilocks element, [0, p), p = 2⁶⁴ − 2³² + 1 | exact numbers, weights, scores |
| word | one element holding a 32-bit value | bitwise, indices, counts |
| hash | 4 elements, 256-bit digest | particle identity (cid, neuron) |

higher shapes (tuples, lists) are [[nox]] cons-structure over these atoms.
numbers are exact field elements. inf has no floating point. focus (φ*), karma,
and weights are field elements as committed in [[bbg]]; inf reads them directly.

types name atom shapes and map onto Trident's types:

| inf type | atom | trident |
|---|---|---|
| `Field` | field | `Field` |
| `Word` | word | `U32` |
| `Cid` / `Neuron` | hash | `Digest` |
| `Bool` | field (0/1) | `Bool` |
| `[T; N]` | cons-structure | `[T; N]` |

arithmetic and comparison are delegated to sibling languages, not reimplemented
in inf (see interop):

- [[Trident|Tri]] — field math in conditions and aggregates
- [[Rs]] — byte-level decode of particle content
- [[Bt]] — bitwise tests on tags
- [[Ten]] — vector similarity / kNN

## I/O shape

an inf program declares its interface the way a Trident program does:

```
pub input  graph_root : Cid          // the cybergraph at a block
pub input  params     : { ... }       // query parameters
pub output result      : Relation     // the derived result set
```

the proof certifies `result = eval(query, graph_state)` where graph_state is
committed by graph_root. relation reads lower to the [[nox]] `look` pattern;
each row carries a [[lens]] opening against graph_root (see proof).

## relations

base relations are views over committed [[bbg]] state. their schemas are
canonical in cybergraph/specs/query.md. the visible set and access scope follow
[the query-modes proposal](props/query-modes.md):

- `axons` — public aggregate, scannable by anyone.
- `cyberlinks` — neuron-scoped: a scan returns the caller's own links
  (plaintext) and links the caller holds an opening key for.
- `particles`, `neurons`, `signals`, `focus`, `karma` — committed state,
  read at the snapshot.

inf does not write canonical relations. mutations flow rune → cybergraph →
[[bbg]]. inf rules may build temporary relations that exist only for the
duration of a query.

## rules

a program is a set of rules. each rule has a head and a body. the entry rule
`?` selects the program output.

### inline rules

```
relevant[particle] := axons{from: seed, to: particle},
                      focus{particle, score}, gt(score, threshold)
```

the body is a conjunction of atoms (relation reads and conditions). rules with
the same head name form a disjunction. `gt`, `add`, and other arithmetic atoms
lower to [[Trident|Tri]].

### the entry rule

```
?[particle, score] := relevant[particle], focus{particle, score}
```

exactly one `?` rule per program. its tuples are the result.

### aggregation

operators in the head reduce groups; variables without an operator are grouping
keys.

```
?[neuron, count(particle)] := cyberlinks{neuron, to: particle}
```

semi-lattice operators (`min`, `max`, `union`, `intersection`) are admissible
inside recursive rules because they converge monotonically.

## bounded recursion

recursion is the reason for datalog and the place inf must satisfy constraint 1.
a recursive rule is a bounded fixed-point iteration. inf forbids general
recursion: a rule whose depth cannot be bounded is rejected at compile time.

the iteration count of a graph query depends on the graph — its diameter, its
size. a reachability query over N particles may need up to N rounds. a zheng
circuit, though, has a depth fixed at compile time. inf reconciles these with
three layers of bound.

- capacity (compile time). the circuit compiles to a maximum depth MAX — the
  largest graph it can serve. this is the fixed bound zheng requires.
- snapshot bound (proof time). the graph root commits `diameter_bound` (bbg
  commits `node_count − 1` by default, or a tighter tru-proven bound), alongside
  `node_count` and `relation_sizes` (see [[bbg/specs/statistics]] and
  [cost](cost.md)). the committed bound is the worst-case iteration count for the
  snapshot. because it is a committed public input, the bound is known before the
  query runs — static — even though it scales with the graph. a query whose
  snapshot bound exceeds MAX is rejected; it needs a larger circuit.
- fixpoint (run time). semi-naive evaluation reaches the actual fixed point at
  round K ≤ diameter ≤ node_count.

```
reachable[cid] := cyberlinks{from: seed, to: cid}
reachable[cid] := reachable[mid], cyberlinks{from: mid, to: cid}
```

with no annotation, the bound is the committed graph size — the natural bound
for a graph query. the rule lowers to Trident's bounded loop `for _ in 0..MAX
bounded MAX` over the semi-naive step, with the committed snapshot bound as the
worst-case round count within MAX.

"static cost" therefore means computable from committed public inputs before
execution, not constant. a graph query that costs O(graph size) is admissible
because the size is committed.

### explicit bound — the special case

`:bounded N` caps iterations below the graph size — "only N hops" — which
shrinks both the cost and the circuit capacity.

```
reachable[cid] := cyberlinks{from: seed, to: cid}
reachable[cid] := reachable[mid], cyberlinks{from: mid, to: cid}
:bounded 3
```

this is a deliberate restriction. the default bound for a graph query is the
graph itself; an explicit bound is for queries that genuinely need fewer hops.

### convergence witness

the prover may stop at the actual fixpoint K and prove that round K added
nothing (the semi-naive delta is empty at K). the produced proof is sized to K
while the cost ceiling reported by `inf cost` stays the snapshot bound.
completeness holds: no derivable fact is omitted, because the delta-empty proof
certifies the fixpoint.

## the pure subset is the Trident sub-grammar

every construct above lowers to Trident grammar: rules to functions, conjunction
to composition, bounded recursion to `for … bounded N`, conditions to `bin_op`
calls into Tri, relation reads to `look`. a pure inf program is
Trident-compatible by construction. the reactive and live registers
(`subscribe`, external calls) sit outside this subset and carry their own proof
contracts (see [extensions](extensions.md)). the boundary is one-way:
Rune-pure ⊃ inf-pure ⊃ Trident grammar; reactive and live inputs never enter
the pure subset.
