---
tags: inf, audit, launch, release
---
# clean-checkout gate — inf

date: 2026-09-24
revision: bf379879b60d724cb34fead91d703e0090acc08e (origin/main)
worktree: fresh checkout only, no owner working tree used

inf was not on the 2026-09-22/23 sweep's list of repos failing the
row-39 clean-checkout gate ([[cyber/launch]]), and had no prior row-39
PR. This measures it directly, same method as tade's and hemera's row-39
audits.

## commands and results

```
$ cargo check --tests --manifest-path rs/Cargo.toml
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.19s

$ cargo test --manifest-path rs/Cargo.toml
test result: ok. 2 passed  (inf-ast)
test result: ok. 6 passed  (inf-cli)
test result: ok. 24 passed (inf-eval)
test result: ok. 4 passed  (inf-lex)
test result: ok. 22 passed (inf-lower)
test result: ok. 11 passed (inf-parse)
test result: ok. 13 passed (inf-plan)
test result: ok. 8 passed  (inf-source)
test result: ok. 9 passed  (inf-value)
```

100 tests, 0 failed, across the nine `rs/` workspace members, resolving
sibling path dependencies (neuron-id, strata, cyber-hemera, cyber-nox)
against their own default-branch checkouts.

`--features prove` does not build clean: `inf-lower` initializes
`zheng::Statement` in three places without the `bbg_root` field zheng
0.4 added. This is the same gap already tracked as
[inf#4](https://github.com/cyberia-to/inf/pull/4) (row 39), not
duplicated here.

## conclusion

inf's default build and test suite pass from a clean checkout of
origin/main against sibling default branches. This closes inf's own
slice of row 39; the row stays open until every repo the sweep found
broken has its fix merged, and inf's `--features prove` gap (inf#4)
resolved.
