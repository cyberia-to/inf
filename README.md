---
tags: cyber, language, soft3
crystal-type: entity
crystal-domain: cyber
alias: Inf, infer, inference language, datalog, CozoScript, cozodb, inf language
---
# inf

ask the [[cybergraph]] a question; get back an answer you can prove.

inf is the query language of the cybergraph — proof language #10 of 14. you
write what you want to know; inf returns the result together with a [[zheng]]
proof that it follows from the graph's committed state. a light client holding
only the 32-byte graph root checks that proof in microseconds — without storing
the graph, and without trusting whoever computed the answer.

## questions inf answers

every answer below comes with a proof against a [[bbg]] root.

```datalog
// discovery — the 20 particles I linked, ranked by focus
?[particle, score] := cyberlinks{neuron: @me, to: particle},
                      focus{particle, score}
:sort -score
:limit 20
```

```datalog
// reachability — can I get from one particle to another through links?
reachable[p] := axons{from: #seed, to: p}
reachable[p] := reachable{mid}, axons{from: mid, to: p}
?[] := reachable{#goal}
:assert some            // the proof shows a path exists
```

```datalog
// policy — prove every particle I linked is above a focus floor
?[p] := cyberlinks{neuron: @me, to: p}, focus{particle: p, score}, lt(score, FLOOR)
:assert none            // empty result, proven — I broke no rule
```

```datalog
// accounting — total conviction the graph holds on an axon
?[weight_sum] := axons{from: #a, to: #b, weight_sum}
```

more in the [whitepaper](docs/) — paths and shortest linkchains, provenance of
your own links, communities and bridge particles, vector-similar particles,
historical state at any past block, and graph-wide invariants.

## why a proven answer is worth more

- ask once, verify forever — the answer is a portable proof. anyone re-checks it
  from the root; proven answers are memoized into the graph as reusable facts.
- trustless — you do not have to trust the node that ran the query.
- agents prove they followed the rules — an agent shows it only linked
  high-karma particles, rather than asking to be believed.
- you know the cost before you run it — `inf cost` reports a cycle ceiling up
  front.

## how it works

- declarative — state what to derive, not how. recursive joins, aggregation,
  and graph algorithms, all as rules.
- exact — values are field elements, never floats. arithmetic delegates to
  siblings: [[Trident|Tri]] (field), [[Rs]] (bytes), [[Bt]] (bits),
  [[Ten]] (vectors).
- the declarative twin of [[rune]] — rune writes the graph, inf reads it.

| register | reads | proof |
|---|---|---|
| pure | committed graph snapshot | unconditional |
| reactive | graph mutations (`subscribe`) | conditional on the event log |
| live | external systems | witness-based |

## status

bootstrapping on CozoDB (vendored in rs/cozo), which shrinks to zero as inf
self-hosts onto [[nox]].

- the whitepaper — docs/ (what you can ask, and why the answers hold)
- the specification — specs/ (language, grammar, cost, …)

cyber license: don't trust. don't fear. don't beg.
