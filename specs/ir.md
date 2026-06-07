---
tags: cyber, soft3, inf
crystal-type: spec
crystal-domain: cyber
---
# inf IR

the intermediate representation between an [[inf]] rule and its execution. the IR
is a relational-algebra plan — inf's analog of [[trident]]'s TIR. it is the one
artifact three consumers share: the bootstrap executes it via CozoDB, the proof
path compiles it to constraints (see [proof](proof.md)), and the cost model reads
it (see [cost](cost.md)).

## pipeline

```
inf rule
  → parse            → AST
  → stratify         → layers for negation and aggregation
  → magic-set        → restrict each rule to the relevant subset
  → plan             → relational-algebra IR (this file)
  → lower            → nox patterns
  → execute / prove  → CozoDB (bootstrap) | zheng (end-state)
```

stratification orders rules so negation and aggregation over a recursive relation
compute bottom-up in layers. the magic-set rewrite pushes bound constants (seeds,
query parameters) into rule bodies so a query touches only the reachable subset
instead of materializing whole relations. both are IR-to-IR transforms.

## the IR

a plan is a tree of operators. each carries a cardinality estimate from the
committed `GraphStats` (see [cost](cost.md)), so the same tree serves execution,
proof, and cost.

| operator | meaning |
|---|---|
| `scan(rel)` | read a relation |
| `point(rel, key)` | read one keyed row |
| `filter(pred)` | keep rows satisfying a condition |
| `project(cols)` | select / compute output columns |
| `join(a, b, on)` | combine two inputs on shared variables |
| `aggregate(group, op)` | reduce groups with a head operator |
| `sort(key, dir)` / `limit(n)` | order and truncate |
| `recurse(step, bound)` | bounded fixed-point over a semi-naive step |
| `assert(set)` / `withdraw(set)` | mutation: the derived cyberlink batch |

conditions and arithmetic inside `filter`/`project` are calls into the sibling
languages ([[Trident|Tri]], [[Rs]], [[Bt]], [[Ten]]); the IR holds the call, not
the implementation (see [interop](interop.md)).

## lowering to nox patterns

each operator lowers to the [[nox]] patterns (16 compute + `call` + `look`):

| IR operator | nox lowering |
|---|---|
| `scan` / `point` | `look` (17) + a [[lens]] opening against the root |
| `filter` | `eq` / `lt` + `branch` (4, lazy) |
| `project` | `cons` (3), `axis` (0); arithmetic via Tri jets |
| `join` | nested `look` + `compose` (2) over shared bindings |
| `aggregate` | `compose` fold with `add` / `min` / `max` |
| `sort` / `limit` | comparison chain; permutation for sort |
| `recurse(step, bound)` | `compose` loop of at most `bound` rounds; `branch` ends it |
| `assert` / `withdraw` | assemble the batch noun → [[signal]] |

recursion lowers to a bounded loop (`for _ in 0..MAX`), the same construct
[[trident]] uses, with the committed snapshot bound as the worst-case round count
within MAX (see [language](language.md)). results assemble as nox nouns; reads
carry their Lens openings forward so the proof can bind every row to the root.

## example

```datalog
relevant[p] := axons{from: #seed, to: p}, focus{particle: p, score}, gt(score, T)
?[p, score] := relevant[p], focus{particle: p, score}
:sort -score
:limit 20
```

plan:

```
limit(20)
└ sort(score, desc)
  └ join(relevant, focus on p)
    └ relevant = filter(gt(score, T))
                 └ join(axons{from:#seed}, focus on p)
```

lowering: the two `axons`/`focus` reads become `look` + Lens openings; `gt` is a
Tri call under a `branch`; the join is `compose` over `p`; `sort`+`limit` is a
comparison chain truncated at 20. the bootstrap runs this plan through CozoDB; the
proof path compiles the same plan to CCS.

## stability across the bootstrap

the IR is the fixed boundary while the executor changes underneath (see
[bootstrap](bootstrap.md)): stage 2 runs the IR on CozoDB, stage 3 runs it on the
Nox interpreter, stage 4 proves it through Trident. the rule surface and the IR do
not change as the executor is replaced.

## see also

- [proof](proof.md) — compiling the IR to constraints
- [cost](cost.md) — costing the IR against committed statistics
- [interop](interop.md) — the sibling calls inside `filter`/`project`
- [[nox]] — the 18 patterns the IR lowers to
