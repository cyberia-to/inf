---
tags: cyber, soft3, inf
crystal-type: spec
crystal-domain: cyber
---
# inf cost

the static cost model for [[inf]]. `inf cost query.inf` reports a cycle ceiling
before execution, satisfying [[trident]]'s third constraint. cost is defined as
a contract at the [[bbg]] ↔ inf interface: bbg owns the cost of reads and the
committed size statistics; inf owns the cost of combining reads.

## why cost lives at the interface

an inf query's work is reads plus combination:

- reads — relation scans and point lookups. these are [[bbg]] operations with a
  known cost (a point read is one [[lens]] opening; a namespace scan is a batch
  opening). bbg already defines this in bbg/specs/query.md.
- combination — joins, filters, aggregations, sorts over the read tuples. their
  cost is a function of cardinalities, which are committed graph statistics that
  bbg exposes.
- recursion — bounded iterations times per-iteration cost.

so cost decomposes across the boundary cleanly: bbg answers "what does it cost
to read X, and how big is X (committed)"; inf answers "what does it cost to
combine the reads."

## the bbg side of the contract

bbg provides two things to the cost model. both must be committed in the graph
root so the reported cost is verifiable, not trusted.

```
bbg.statistics() -> GraphStats            // committed in BBG_root via stats_commit
    node_count:     u64                    // particles (graph nodes), exact
    relation_sizes: [u64; 11]              // rows per relation, canonical order, exact
    max_degree:     u64                    // max adjacency-list length (fan-out per hop)
    diameter_bound: u64                    // sound upper bound on graph diameter

bbg.read_cost(op) -> cycles               // bbg/specs/query.md
    point_open                            // one Lens opening
    batch_open(n)                         // namespace / range opening of n rows
    temporal_open                         // opening at a past time dimension
```

`relation_sizes` is indexed in the canonical relation order: particles,
axons_out, axons_in, neurons, locations, coins, cards, files, time, signals,
balances.

bbg commits these in `BBG_root` (`stats_commit = H(node_count ‖ relation_sizes ‖
max_degree ‖ diameter_bound)`); see [[bbg/specs/statistics]]. committed rather
than passed as a side input, the recursion bound and cardinality estimates rest
on proven values, so cost and termination are provable, not trusted.

## the inf side of the contract

inf supplies the query plan and the per-operator cost coefficients.

```
op                cost (cycles)              source of cardinality
─────────────     ────────────────────────  ─────────────────────
scan(rel)         read_cost.batch_open(n)    relation_sizes[rel]
point(rel,key)    read_cost.point_open       1
adjacency(p)      read_cost.batch_open(d)    max_degree (bound on d)
filter            c_filter × n               input cardinality
join(a,b)         c_join × |a| × sel(b)      relation_sizes, selectivity
aggregate         c_aggr × n                 input cardinality
sort              c_sort × n log n           output cardinality
recursion         bound × per_iteration      see below
```

selectivity for a join uses committed sizes and key structure (a keyed lookup
has selectivity 1/relation_sizes[rel]). the coefficients `c_*` are fixed per release
and calibrated against the [[nox]] lowering, the same scoreboard discipline
Trident uses (references ground truth, baselines floor).

## recursion cost

a bounded recursive rule costs `bound × per_iteration`, where the bound comes
from the rule (see [language](language.md), bounded recursion):

- default → the committed `diameter_bound` (bbg commits `node_count − 1` by
  default, or a tighter tru-proven bound installed via `set_diameter_bound`).
  the cost scales with the graph and is static because the bound is committed.
- explicit `:bounded N` → bound is N, capping iterations below the graph size.

`per_iteration` is the cost of one semi-naive step: the reads plus the join that
extends the frontier. the reported ceiling uses the snapshot bound; the
convergence witness (language.md) may make the produced proof cheaper, but
`inf cost` reports the ceiling, not the optimistic value.

## the cost computation

```
inf cost(query, graph_root):
    stats   = bbg.statistics()                    // committed under graph_root
    plan    = inf.plan(query, stats)              // cardinality-annotated logical plan
    reads   = Σ bbg.read_cost(op)   for read ops in plan
    combine = Σ inf.op_cost(op, card) for relational ops in plan
    recur   = Σ bound(rule, stats) × per_iteration(rule)
    return reads + combine + recur                 // cycle ceiling
```

the result is static once `graph_root` is fixed: every term is either a fixed
coefficient or a committed statistic. a query whose cost cannot be bounded this
way — an unbounded recursion with no inferable bound — is rejected before
execution, not costed at runtime.

## relation to proof cost

`inf cost` reports execution cycles. proof size and verification time follow the
[[zheng]] cost model over the lowered [[nox]] trace and are reported by the
proof path (see proof). verification is constant (one decider) regardless
of query complexity; query complexity affects prover cost, which the cycle
ceiling bounds.
