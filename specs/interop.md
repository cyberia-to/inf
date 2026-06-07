---
tags: cyber, soft3, inf
crystal-type: spec
crystal-domain: cyber
---
# inf interop

[[inf]] is pure relation. it derives sets; it does not implement arithmetic,
byte decoding, bit logic, or linear algebra. those delegate to the sibling
languages that own each algebra. this file defines the calling boundary — into
the siblings, and from [[rune]] into inf.

## the shared currency

every value crossing a boundary is a [[nox]] atom — `field`, `word`, or `hash`
(see [language](language.md)). there is no marshaling: a particle is a `hash`, a
weight is a `field`, a tag is a `word`, in inf and in every sibling alike. a call
is a nox `compose` into the sibling's jet; the result is a nox noun.

## calling the siblings

inside a rule body, conditions and computed terms call the language that owns the
algebra:

| sibling | owns | inf uses it for |
|---|---|---|
| [[Trident|Tri]] | field arithmetic | comparisons, sums, weights, the `mean` quotient |
| [[Rs]] | bytes | decoding particle content, text, json, regex |
| [[Bt]] | bits | tag tests, bitwise masks |
| [[Ten]] | tensors | vector similarity, kNN, the cores of graph algorithms |

```datalog
?[p, d] := embeddings{particle: p, vec},
           d = Ten.cos_dist(vec, query),     // Ten
           Bt.test(tag(p), FLAG),            // Bt
           gt(focus(p), Tri.mul(base, w))    // Tri
```

a call stays in inf-pure only if the sibling operation is deterministic and
provable. the pure operations of Tri/Rs/Bt/Ten qualify; transcendentals and
randomness do not, and are excluded (see [functions](functions.md)).

## being called from rune

inf-pure is a sub-grammar of [[trident]], so a Rune-pure program embeds an inf
derivation as a sub-formula and receives a provable set:

```
=/  hot   inf"?[p] := axons{from: #topic, to: p}, focus{particle: p, s}, gt(s, T)"
```

the boundary is one-way:

```
Rune-pure ⊃ inf-pure ⊃ Trident grammar
```

a Rune-pure caller gets an unconditionally provable result. a Rune-dynamic
context (one using `hint`/`host`/`eval`) may also call inf, but it cannot smuggle
non-determinism back into the pure subset: the inf derivation stays pure, and any
values it takes from the dynamic context are witnesses, making the composed
result conditional (see [extensions](extensions.md)). inf never becomes
non-deterministic by being called; it only ever produces a pure derivation, whose
inputs may or may not be committed.

## relation to rune — two surfaces

inf and rune are the declarative and imperative surfaces over one graph (the
JavaScript-and-SQL split). rune scripts per-item, reactive effects and calls into
hosts; inf derives and appends whole sets. rune logic invokes inf for a bulk
derivation; inf results feed rune decisions. both lower to nox and prove through
the same pipeline (see [proof](proof.md)).

## see also

- [functions](functions.md) — which builtin maps to which sibling
- [language](language.md) — the value model and the pure subset
- [extensions](extensions.md) — reactive and live, the non-pure registers
- [[rune]] — the imperative twin
