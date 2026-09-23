---
tags: inf, audit, zheng, nox, clean-checkout
crystal-type: audit
crystal-domain: cyber
---
# inf-lower R2 proof round-trip vs zheng 0.4.0

date: 2026-09-23 · revision: inf `bf37987`, zheng (local, pre-`zheng#33` pin bump
`a2f8f0b build: zheng 0.4.0 — pin nox 0.3.0, lens 0.2.0`), nox `0.3.0`.
property: [[launch#property registry|launch.md]] row 39, clean-checkout builds.

## clean-checkout build break

`rs/lower/src/lib.rs`'s three `zheng::Statement` literals (`prove_expr`,
`verify_expr_proof`, `prove_cond`) predate zheng 0.4.0's `bbg_root` field
(zheng's BBG state-root binding for pattern-17 look rows). Against zheng
0.4.0, `cargo check --tests --features prove` fails with three
`missing field 'bbg_root'` errors — inf was not in the row-39 sweep's
original 14-repo list because its `prove` feature isn't exercised by a
default build; this is a 15th repo hitting the same pin drift.

fix: all three literals take `bbg_root: [0u8; 32]`, the documented
"no state read" sentinel — correct here because `commit()` is always
called with an empty `look_openings` slice (pure arithmetic and pure
condition traces carry no look rows).

## pre-existing: R2 proof round-trip fails under zheng 0.4.0

With the field fix in place, `cargo test --features prove` still fails:
all three `prove_and_verify_*` tests panic with
`zheng verify: SumcheckFailed { round: 0 }` (or `round: 1` for the
two-operand `mul` case). This reproduces with `bbg_root` fixed at the
same sentinel value on both the `prove_expr`/`prove_cond` and
`verify_expr_proof` sides, so it is not a Statement-mismatch — the
`SpartanVerifier` rejects the universal-instance proof itself.

this is not the `UnsupportedRecursiveOpening` break bbg#24/#25/#12
already document (that one fires on non-empty `axis_openings`/
`look_openings`; inf-lower's traces pass empty slices for both). It
reproduces on the simplest possible traces — a two-step `mul`, a
one-step `lt`, a one-step `gt` — so it looks like a nox 0.3.0 trace
layout / zheng 0.4.0 `universal_ccs()` step-building incompatibility
upstream of anything inf-lower controls, not a bug in this crate's
`prove_expr`/`prove_cond`/`verify_expr_proof` wiring.

not diagnosed further here: reproducing needs stepping through
`build_universal_steps_from_trace` against a live nox 0.3.0
`VecTrace`, which is a zheng/nox-side investigation, not a one-file
inf fix.

## also open, unrelated to the above

`verify_expr_proof(proof: &zheng::TraceProof)` takes no `result` and no
program identity — `prove_expr`/`prove_cond` set `program_hash` and
`output_hash` to `[0u8; 32]` (zheng's sentinel that skips `commit()`'s
own row-hash binding check) rather than binding them to the actual
computation. Once the round-trip above passes again, a caller has no
way to check that a `TraceProof` attests to any *specific* claimed
result — `verify_expr_proof(&proof_of_2_plus_2)` and
`verify_expr_proof(&proof_of_6_times_7)` both return `Ok(())` under the
identical zeroed `Statement`, so nothing here currently lets an
external verifier distinguish which computation a proof is for. Binding
`output_hash` to `zheng::row_hash()` of the trace's last row would let
`commit()`'s existing check enforce it on the prove side, but a remote
verifier still cannot recompute that hash from a bare `u64` result
without the internal `nox::TraceRow` layout (16 columns of VM-internal
state, not just the semantic value) — closing this needs either a
public-output convention in nox's trace format or a different binding
scheme, not a change confined to `inf-lower`. Left as a named gap for
whoever picks up real result-binding for provable reads.

## verified

```
$ cargo check --tests --features prove
    Finished `dev` profile [unoptimized + debuginfo] target(s)
$ cargo test --features prove
test result: ok. 22 passed; 0 failed; 3 ignored; 0 measured; 0 filtered out
```
