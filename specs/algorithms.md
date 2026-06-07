---
tags: cyber, soft3, inf
crystal-type: spec
crystal-domain: cyber
---
# inf algorithms

graph algorithms available as fixed rules (`<~`) in [[inf]]. a fixed rule takes a
relation of edges and binds algorithm-specific output columns:

```datalog
edges[from, to] := axons{from, to}
?[node, rank] <~ PageRank(edges[], theta: 85)   // damping as an integer percent
```

the algorithm logic is inf's; the arithmetic is not. linear-algebra cores
(matrix-vector products, distances) lower to [[Ten]]; field operations lower to
[[Trident|Tri]]. every algorithm runs bounded, so its cost is static and its
trace provable.

## bounded by construction

[[trident]] constraint 1 forbids run-to-convergence loops with a data-dependent,
unbounded round count. each algorithm below carries a static bound, set the way
recursion is bound (see [language](language.md)):

- iterative algorithms (PageRank, label propagation, random walk) run a fixed
  iteration count, not an epsilon-convergence loop. the count is an explicit
  parameter or the committed `diameter_bound`.
- traversal and pathfinding (BFS, Dijkstra, A*) are bound by `diameter_bound`
  (committed in the graph root, see [[bbg/specs/statistics]]).
- damping, thresholds, and weights are field elements, never floats.

a convergence witness (see [language](language.md)) may size the produced proof
to the round where the result stabilized, while `inf cost` reports the bound.

## centrality

| rule | output | core | notes |
|---|---|---|---|
| `PageRank(edges, theta, iters)` | `[node, rank]` | Ten matvec | `theta` damping is a field element; `iters` fixed. related to [[cyberank]] and [[diffusion]] |
| `DegreeCentrality(edges)` | `[node, degree, in, out]` | counting | one pass |
| `BetweennessCentrality(edges)` | `[node, score]` | Ten | bound by `diameter_bound`; finds bridge particles |
| `ClosenessCentrality(edges)` | `[node, score]` | Ten | bound by `diameter_bound` |

## pathfinding

| rule | output | notes |
|---|---|---|
| `BreadthFirstSearch(edges, src)` | `[node, depth]` | depth bound `diameter_bound` |
| `DepthFirstSearch(edges, src)` | `[node, depth]` | depth bound `diameter_bound` |
| `ShortestPathBFS(edges, a, b)` | `[path]` | unweighted |
| `ShortestPathDijkstra(edges, a, b)` | `[path, cost]` | weighted; weights are field elements |
| `ShortestPathAStar(edges, a, b, heur)` | `[path, cost]` | heuristic-guided |
| `KShortestPathYen(edges, a, b, k)` | `[path, cost]` | k alternative linkchains |

## community

| rule | output | core | notes |
|---|---|---|---|
| `CommunityDetectionLouvain(edges)` | `[node, community]` | Ten | fixed pass count; topic clusters |
| `LabelPropagation(edges)` | `[node, label]` | counting | fixed iteration count |
| `ClusteringCoefficients(edges)` | `[node, coeff]` | counting | neighbor interconnection |

## connectedness

| rule | output | notes |
|---|---|---|
| `ConnectedComponents(edges)` | `[node, component]` | knowledge islands |
| `StronglyConnectedComponent(edges)` | `[node, scc]` | needed for tri-kernel convergence reasoning |
| `MinimumSpanningForestKruskal(edges)` | `[from, to, weight]` | Prim variant takes a root |
| `TopSort(edges)` | `[node, order]` | DAG only; collapse cycles with SCC first |

## random walk

| rule | output | notes |
|---|---|---|
| `RandomWalk(edges, src, steps, times)` | `[node]` | the primitive under [[diffusion]] |

`RandomWalk` needs randomness, which the pure subset excludes (see
[functions](functions.md)). in a provable query its draws come from a committed
seed (Fiat-Shamir over the root), so the walk is reproducible and the trace
provable; without a committed seed it runs only in the bootstrap.

## provability

a fixed rule's output is part of the derivation tree and proves against the graph
root like any other relation (see [proof](proof.md)): the edge reads are [[lens]]
openings, the iteration is a bounded [[nox]] `compose` loop, and the Ten/Tri
cores contribute their own constraints. verification stays constant regardless of
graph size; the bound governs prover cost.

## see also

- [functions](functions.md) — builtins and the determinism rules
- [language](language.md) — bounded recursion and the value model
- [cost](cost.md) — how the bound enters the cost ceiling
- [[cyberank]] — how PageRank generalizes into the tri-kernel
