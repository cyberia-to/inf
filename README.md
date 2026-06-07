---
tags: cyber, language, soft3
crystal-type: entity
crystal-domain: cyber
alias: Inf, infer, inference language, datalog, CozoScript, cozodb, inf language
---
# inf

ask the [[cybergraph]] anything, and reshape it — every answer and every update
carries a proof.

inf is the query-and-mutation language of the cybergraph — proof language #10 of
14. you write what you want to know, or what you want the graph to become; inf
returns the result, or commits the change, with a [[zheng]] proof that it follows
from the graph's committed state. a light client holding only the 32-byte graph
root checks that proof in microseconds — without storing the graph, without
trusting whoever ran it.

two things, both set-based, both proven.

## ask — provable answers

the cybergraph is a planetary memory — every idea a [[particle]], every claim a
[[cyberlink]], every mind a [[neuron]] — and inf lets you ask it the questions
that matter. what does the graph hold most important here, and how did that shift
as the world learned? which ideas bridge two fields that rarely meet? what is the
shortest chain of reasoning from this claim to that one? who shaped this concept,
and what did they stake on it? which schools of thought are forming, which are
dissolving, and is this whole region of knowledge free of contradiction? these
are questions you could once only answer by trusting whoever ran them — inf
answers them against a 32-byte root, so the answer travels anywhere and convinces
anyone.

```datalog
// discovery — the 20 particles I linked, ranked by focus
?[particle, score] := cyberlinks{neuron: @me, to: particle},
                      focus{particle, score}
:sort -score
:limit 20
```

```datalog
// policy — prove every particle I linked clears a focus floor
?[p] := cyberlinks{neuron: @me, to: p}, focus{particle: p, score}, lt(score, FLOOR)
:assert none            // empty result, proven — I broke no rule
```

## reshape — provable mass updates

a signal is a set of [[cyberlinks]], and inf is set-based — so it derives the
whole set a rule implies over current state and commits it as one staked signal.
the proof certifies the change touched exactly these links and no others.

```datalog
// relink every axon under #old to #new, in one signal
stale[from] := axons{from, to: #old}, cyberlinks{neuron: @me, from}
?[from] := stale[from]
:link { neuron: @me, from, to: #new }
```

more in the [whitepaper](docs/) — paths and shortest linkchains, provenance,
communities and bridge particles, vector-similar particles, historical state at
any past block, graph-wide invariants, and provable bulk migrations.

## why a proof changes everything

- ask once, verify forever — answers are portable proofs, re-checked from the
  root and memoized into the graph as reusable facts.
- trustless — you check the proof, not the node that ran the query.
- provable compliance — an agent proves it followed its policy instead of asking
  to be believed.
- provable migrations — a bulk change proves it touched exactly the right set,
  complete and nothing extra, before it commits.
- cost known up front — `inf cost` reports a cycle ceiling before you run.

## how it works

- declarative and set-based — state the pattern; inf finds the set, for both
  reads and writes. recursive joins, aggregation, and graph algorithms as rules.
- the imperative twin is [[rune]] — rune scripts per-item, reactive effects; inf
  derives and mass-updates whole sets. same graph, two paradigms.
- exact — values are field elements, never floats. arithmetic delegates to
  siblings: [[Trident|Tri]] (field), [[Rs]] (bytes), [[Bt]] (bits),
  [[Ten]] (vectors).
- both reads and writes route through cybergraph → [[bbg]]; canonical mutations
  commit as staked signals, never a direct store write.

| register | acts on | proof |
|---|---|---|
| pure | a committed snapshot | unconditional |
| reactive | graph mutations (`subscribe`) | conditional on the event log |
| live | external systems | witness-based |

## status

bootstrapping on CozoDB (vendored in rs/cozo), which shrinks to zero as inf
self-hosts onto [[nox]].

- the whitepaper — docs/ (what you can ask, what you can change, why it holds)
- the specification — specs/ (language, grammar, cost, …)

cyber license: don't trust. don't fear. don't beg.
