---
tags: cyber, soft3, inf
crystal-type: spec
crystal-domain: cyber
---
# inf bootstrap

[[inf]] starts as CozoDB and shrinks it to zero, the way [[trident]] started as
Rust. CozoDB is vendored in rs/cozo as the bootstrap executor; over four stages
it is replaced by the native [[nox]] path, and removed. CozoDB is the scaffold,
not the design — inf-pure is defined by the Trident contract (see
[language](language.md)), not by what cozo happens to do.

## what carries over, what is dropped

inf keeps cozo's query brain and rewrites the rest.

| from cozo | disposition |
|---|---|
| parser, stratification, magic-set rewrite, semi-naive evaluation | keep — the datalog engine |
| fixed-rule graph algorithms | keep the logic; arithmetic → [[Ten]]/[[Trident|Tri]] (see [algorithms](algorithms.md)) |
| storage engine | drop — [[bbg]] owns state; relations are views (see [relations](relations.md)) |
| float arithmetic | drop — field elements; delegate to Tri (see [functions](functions.md)) |
| run-to-convergence recursion | drop — bounded fixed point (see [language](language.md)) |
| `:put`/`:rm` db writes | drop for canonical — append as signals; keep for temp relations |

the CozoScript surface is the bootstrap reference; the canonical grammar narrows
it (see [grammar](grammar.md), divergence). cozo's own docs and source live in
rs/cozo.

## the four stages

each stage publishes a benchmark against a reference floor — cycles, proof size,
verification time — the same scoreboard discipline Trident uses (references/ as
ground truth, baselines/ as the hand floor).

1. queries parse, plan, and execute via CozoDB. results trusted.
2. queries parse via inf's front end into the query-plan [IR](ir.md); CozoDB
   executes the IR.
3. the IR lowers to nox patterns; execution moves to the nox interpreter.
4. native nox execution with [[zheng]] proofs via Trident. CozoDB removed.

the [IR](ir.md) is the fixed boundary across stages: the rule surface and the IR
do not change as the executor is swapped underneath.

## scoreboard

| stage | executor | trusted | provable |
|---|---|---|---|
| 1 | CozoDB | yes | no |
| 2 | CozoDB over inf IR | yes | no |
| 3 | nox interpreter | yes | the path exists |
| 4 | nox + Trident | — | yes |

a stage lands only when its benchmark holds against the floor and the previous
stage's results match (multi-dimensional verification: reference, executor, and —
from stage 4 — proof must agree).

## see also

- [ir](ir.md) — the stable boundary the executor swaps under
- [grammar](grammar.md) — how the canonical surface narrows CozoScript
- [proof](proof.md) — the proof path that engages at stage 4
