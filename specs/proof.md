---
tags: cyber, soft3, inf
crystal-type: spec
crystal-domain: cyber
---
# inf proof

how an [[inf]] result certifies itself against a [[bbg]] root. a query result, or
the set a mutation appends, carries a [[zheng]] proof that it follows from the
committed graph state. a verifier holding only the 32-byte root checks it in
constant time.

## what a proof asserts

```
read:   result        = eval(query, graph_state)
write:  appended_set  = derive(rule, prior_state)
        where graph_state / prior_state is committed by graph_root
```

for a read, the proof binds the returned tuples to the query and the root. for a
mutation, it binds the appended cyberlink batch to the rule and the prior root;
the staked [[signal]] then carries that proof plus the submitting neuron's
signature.

## the construction

a query is a derivation tree over relations. the tree compiles to a constraint
system and proves via zheng (SuperSpartan + Brakedown over [[lens]]):

```
inf rule
  → derivation tree (relational algebra; see ir.md)
  → relation reads become Lens openings against graph_root
  → operators become CCS constraints
  → zheng proof
```

relation reads lower to the [[nox]] `look` pattern and open against the root.
operators become constraints, following the mapping bbg defines
(bbg/specs/query.md, bbg/roadmap/verifiable-query.md):

| operator | constraint |
|---|---|
| select / filter | range check |
| project | polynomial evaluation at a subset |
| join | lookup argument (LogUp) |
| sort | permutation argument |
| aggregate (sum, max, count) | running accumulator |
| ordered aggregate (`running_*`, `:order`) | sort (permutation argument) + running accumulator |
| limit (top-k) | comparison chain + truncation |
| recursion | bounded fold (one layer per round, ≤ the snapshot bound) |

the proof covers both the openings and the computation: it shows every input row
came from the committed graph and the operators were applied correctly. the
ordered aggregate (see [language](language.md), bounded ordered aggregate) is
exactly these two existing rows composed — no new constraint form. this is
the point of the primitive: the same question re-expressed as an inequality
self-join instead proves as a join per compared pair, quadratic in the
partition size rather than linear.

## a multi-row result

a result set is certified by a batch [[lens]] opening over its rows together with
the derivation circuit — one zheng proof, not one per row. a single-point read
(one relation, one key) needs only a single Lens opening (~200 bytes); a complex
query (join, sort, filter, recursion) compiles to the full CCS instance. either
way verification is one decider.

## verification cost

verification is constant — one zheng decider, ~5 μs, proof under ~10 KiB —
regardless of query complexity or graph size. complexity raises prover cost,
which `inf cost` bounds (see [cost](cost.md)); it does not raise verifier cost.

## recursion and completeness

a bounded recursive rule proves as a fold of at most the snapshot bound rounds
(see [language](language.md)). a convergence witness proves the semi-naive delta
is empty at the round the result stabilized, certifying completeness — no
derivable fact omitted — while sizing the produced proof to that round.

an invariant query (`:assert none` / `:assert some`) proves emptiness or
existence over the whole committed graph: the absence of a violating row is
proven, not sampled.

## mutation

a mutation's derivation is proven exactly as a read: the appended set is shown to
be precisely what the rule implies over the prior root — complete, and nothing
beyond it. the commit wraps that proof in a [[signal]], staked and signed once by
the submitting neuron, which cybergraph orders and validates. the proof makes the
bulk change auditable from the root; the signature and stake make it an act of
will (see [language](language.md), mutation).

## privacy

`axons` and other public relations open against the public commitment, so their
proofs are checkable by anyone. `cyberlinks` are neuron-scoped: a proof over your
own links uses openings from your personal owner store; others' individual links
prove only with an opening key you hold. the public path never discloses an
individual cyberlink (see [relations](relations.md)).

## interactive and provable

proof is opt-in. the same rule produces the same result whether run interactively
(no proof emitted) or provably (proof emitted); provability is a cost paid at
submission, not a different answer. the bootstrap runs interactively via CozoDB;
the proof path engages as inf lowers to [[nox]] (see [bootstrap](bootstrap.md)).

## see also

- [ir](ir.md) — the derivation tree that gets compiled
- [cost](cost.md) — prover cost; the bbg statistics interface
- bbg/specs/query.md — Lens-opening proof types and the constraint cost model
- [[zheng]] — the proof system (SuperSpartan + Brakedown)
