---
tags: cyber, language, soft3
crystal-type: entity
crystal-domain: cyber
alias: Inf, infer, inference language, datalog, CozoScript, cozodb, inf language
---
# inf

the question language of the [[cybergraph]]. inf asks; [[rune]] answers by acting.

inf is proof language #10 of 14 (Infer — Horn clauses). where [[rune]] writes
the graph, inf reads and reasons over it. a query is a set of rules; the answer
returns with a STARK proof that it follows from the committed graph state. ask
once, verify forever.

```datalog
// top particles a neuron linked, ranked by focus
?[particle, score] := cyberlinks{neuron: @me, to: particle},
                      focus{particle, score}
:sort -score
:limit 20
```

## what makes inf a proof language

- declarative — state what to derive, not how. Horn clauses, recursive joins,
  aggregation, graph algorithms.
- provable — the pure register satisfies [[trident]]'s three constraints
  (bounded recursion, determinism, static cost), lowers to [[nox]] patterns, and
  produces a STARK. every result certifies against a [[bbg]] root.
- exact — values are field elements, never floats. arithmetic delegates to
  siblings: [[Trident|Tri]] (field), [[Rs]] (bytes), [[Bt]] (bits),
  [[Ten]] (vectors).
- the declarative twin of [[rune]] — same stack position, opposite verb.

## three registers

| register | reads | proof |
|---|---|---|
| pure | committed graph snapshot | unconditional STARK |
| reactive | graph mutations (`subscribe`) | conditional on the event log |
| live | external systems | witness-based |

## status

bootstrapping on CozoDB (vendored in rs/cozo), which shrinks to zero as inf
self-hosts onto [[nox]].

- the whitepaper — docs/ (why and how, the full design)
- the specification — specs/ (language, grammar, cost, …)

cyber license: don't trust. don't fear. don't beg.
