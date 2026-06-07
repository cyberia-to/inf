# inf/rs — the inf engine

the Rust implementation of [[inf]], written from scratch per
.claude/plans/stage3-engine.md. the core language is complete and tested; the nox
lowering reaches stage 3 for the expression layer. cozo/ is the gitignored
study/oracle fork (excluded from this workspace).

## crates

| crate | dir | what |
|---|---|---|
| inf-value | `value/` | Goldilocks field, value model, set-semantic relations |
| inf-ast | `ast/` | AST + IR types |
| inf-lex | `lex/` | tokenizer |
| inf-parse | `parse/` | parser → AST (pure register) |
| inf-source | `source/` | RelationSource trait + LocalSource |
| inf-plan | `plan/` | stratify, recursion detection, safety → IR |
| inf-eval | `eval/` | reference semi-naive evaluator + fixed-rule algorithms |
| inf-lower | `lower/` | stage-3 lowering of expressions to the real nox VM |
| inf-cli | `cli/` | the `inf` binary (check/plan/cost/run) |

## build and test

```
cargo test                         # core crates (value..cli) — pure stable Rust
RUSTC_BOOTSTRAP=1 cargo test       # whole workspace incl. inf-lower (pulls nox)
cd oracle && RUSTC_BOOTSTRAP=1 cargo test   # differential oracle vs CozoDB (needs rs/cozo)
```

inf-lower depends on the `nox` crate (→ nebu, hemera), which needs
`RUSTC_BOOTSTRAP=1`. The core engine has no external cyber dependencies and
builds/tests on its own. 50 tests pass; zero warnings.

Run a query against the built-in demo graph:

```
cargo run -p inf-cli -- run query.inf
cargo run -p inf-cli -- check query.inf
cargo run -p inf-cli -- cost query.inf
```

## spec compliance

| spec | status | notes |
|---|---|---|
| language.md | complete (pure register) | value model (no float), rules, entry, aggregation, bounded recursion, mutation derivation, `pub input/output` decls, `_`-prefixed temp relations. reactive/live = extensions (planned). |
| grammar.md | complete | rules, conditions, binds, negation, mutation (`:link`/`:unlink`/`:put`/`:rm`), options, fixed rules (`<~`), addresses, infix arithmetic. decls aside. |
| relations.md | partial | LocalSource implemented; bbg/cybergraph sources and privacy scopes are planned (no bbg yet). |
| functions.md | core complete | arithmetic, comparison, logical, `in`, min/max, mod/neg/abs; string (length, concat, case, starts/ends/includes); list (length, first, last, get, concat); coalesce, to_int, to_string; aggregations count/sum/min/max/mean/collect/unique/union. vector/json/regex/time planned; transcendentals/random excluded from the pure subset by design. |
| algorithms.md | representative set | PageRank (fixed-point), DegreeCentrality, ConnectedComponents, StronglyConnectedComponent, BFS, DFS, ShortestPathBFS, ShortestPathDijkstra, TopSort. Betweenness/Closeness/Louvain/LabelProp/ClusteringCoeff/Yen/AStar/MST/RandomWalk planned. `theta` is an integer percent (no float literals). |
| cost.md | partial | `inf cost` reports a static estimate with a reads/combine/recursion breakdown over the IR and source sizes; the full committed-GraphStats model needs bbg. |
| ir.md | partial | IR (strata) implemented; nox lowering implemented for the expression layer (real nox VM); full relational lowering (look-scans, compose-joins, bounded-compose recursion) planned. |
| proof.md | planned | stage 4 (zheng proofs). |
| extensions.md | planned | reactive (`subscribe`) and live. |
| bootstrap.md | stage 2 complete | parse→IR→eval is the working reference engine; stage 3 = expression lowering on real nox; stages 3-remainder and 4 planned. cozo differential oracle wired (rs/oracle). |

## verification

- 50 unit + integration tests across the workspace.
- `eval` corpus covers: join/filter/sort/limit, bounded reachability, full
  closure, aggregation, negation, `:assert`, mutation batch, infix arithmetic,
  and the fixed-rule algorithms.
- `inf-lower` differential tests: nox-VM result == reference engine result for
  arithmetic expressions, on the production `nox` crate.
- the `inf` CLI runs the spec-example queries (discovery, policy, reachability,
  PageRank, mutation) end-to-end with correct results.
- the differential oracle (rs/oracle) asserts inf-eval == CozoDB on filter,
  join, aggregation, transitive closure, and negation.

## what remains

full relational nox lowering + zheng proofs (stage 3 remainder + stage 4), the
bbg/cybergraph sources with privacy, the reactive/live registers, and the long
tail of builtin functions and graph algorithms. see .claude/plans/remaining.md.
