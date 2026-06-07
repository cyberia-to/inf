---
tags: cyber, soft3, inf
crystal-type: spec
crystal-domain: cyber
---
# inf extensions — reactive and live

the pure register (language, proof) is a function of a committed snapshot. two
extensions reach beyond it, mirroring [[rune]]'s `hint` and `host`. each sits
outside the Trident-pure subset and carries its own proof contract. neither leaks
non-determinism back into the pure subset: a rule's derivation stays pure; only
its trigger or its external input lives outside.

## the proof spectrum

```
pure       unconditional proof over the snapshot          (language, proof)
reactive   proof conditional on the event log             (this file)
live       proof conditional on an external witness       (this file)
```

## reactive — subscribe

a reactive rule is a standing query that re-fires when the graph changes. it is
inf's analog of rune's `hint`.

```datalog
// re-rank a topic's particles whenever a new link lands under it
?[p, score] := axons{from: #topic, to: p}, focus{particle: p, score}
:subscribe axons{from: #topic}
:sort -score
```

`:subscribe sel` registers a selector over mutations; each matching mutation
re-evaluates the rule against the new snapshot. the proof is conditional on the
event log: given the sequence of mutation events as witnesses, every
re-evaluation is provable, the way rune's kernel is provable given its events.

a reactive rule may itself emit a mutation — find a pattern, append links — but
the commit is a staked [[signal]] from the subscriber's neuron, exactly as in the
pure case (see [language](language.md), mutation). the trigger is reactive; the
write is still will.

determinism holds relative to the log: the same event sequence on every node
yields the same re-evaluations. the snapshot each firing reads is committed; only
the firing schedule comes from outside.

## live — external

a live rule federates across systems outside the cybergraph — another chain, an
oracle, a classical database. it is inf's analog of rune's `host`.

```datalog
// join graph particles against an external price feed
?[p, px] := axons{from: #market, to: p}, px = Host.fetch(feed, p)
```

`Host.fetch` returns a value the proof treats as an asserted witness: the
surrounding derivation is provable given that witness, and the trust boundary is
the external system itself. the external call carries no [[nox]] trace; what is
proven is "given this external result, the rest of the query was evaluated
correctly."

## the boundary

```
Rune-pure ⊃ inf-pure ⊃ Trident grammar
```

reactive and live inputs never enter the pure subset. an inf derivation invoked
from a dynamic [[rune]] context stays pure in itself; the values it receives from
that context are witnesses, so the composed result is conditional, not pure (see
[interop](interop.md)). a `:subscribe` selector and a `Host.fetch` call have no
form in the pure register; a query that uses neither is Trident-compatible as-is.

## see also

- [language](language.md) — the pure register
- [proof](proof.md) — the unconditional proof the extensions extend
- [interop](interop.md) — how inf composes with rune and the siblings
- [[rune]] — `hint` and `host`, the precedents
