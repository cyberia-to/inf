# verify_expr_proof used a Statement that could never match the one proved

2026-09-24 · revision bf37987 (origin/main) · `cargo test --features prove`

## finding

`rs/lower/src/lib.rs`'s three zheng entry points each built a `zheng::Statement`
independently:

- `prove_expr` and `prove_cond` set `focus_bound: tracer.0.len() as u64` — the
  real trace length.
- `verify_expr_proof` set `focus_bound: u64::MAX`.

`zheng::verify` absorbs its `statement` argument byte-for-byte into the same
Fiat-Shamir transcript `zheng::commit` used to generate the proof
(`Transcript::absorb_statement`, `zheng/rs/src/transcript.rs:122-129`, called
from both `commit`'s `run_decide` and `verify` with the caller-supplied
`Statement`). Any field that differs between the proving statement and the
verifying statement desyncs every challenge derived after it, so the proof's
Spartan sumcheck responses no longer match what the verifier recomputes.
`focus_bound` differing (a real trace length vs. `u64::MAX`) guaranteed this
on every call — `verify_expr_proof` could not accept a proof produced by
`prove_expr`/`prove_cond` even for a correct, honestly-generated one.

Open PR inf#4 (row 39, unmerged) hit exactly this symptom — the three
`prove_and_verify_*` tests failing with `SumcheckFailed` — and attributed it
to "a separate, pre-existing nox-0.3.0-trace vs zheng-0.4.0-`universal_ccs`
incompatibility, unrelated to the Statement content," then `#[ignore]`d the
tests. That diagnosis is corrected here: the Statement content is exactly the
cause, and the round trip is not broken upstream.

## fix

`focus_bound: 0` in all three functions. `0` is the documented "no bound"
sentinel already used by `zheng::commit` itself
(`if statement.focus_bound > 0 && trace.len() > statement.focus_bound { ... }`,
`zheng/rs/src/lib.rs:117`) and by zheng's own test suite — not a new
convention. `prove_expr`/`prove_cond`/`verify_expr_proof` now build
byte-identical statements, so the transcripts match. Also added the
`bbg_root: [0u8; 32]` field zheng 0.4.0's `Statement` requires (`Statement`
gained the field after these functions were written; same sentinel value
inf#4 and mudra#13 use for "no state read").

Verified, this revision, this worktree:
```
$ cargo test --features prove -- prove_and_verify
test tests::prove_and_verify_arithmetic_expr ... ok
test tests::prove_and_verify_tuple_condition ... ok
test tests::prove_and_verify_condition ... ok
test result: ok. 3 passed; 0 failed
```
All three were failing (or not compiling, pre-bbg_root) on `origin/main` before this change.

## what is still open

`program_hash`, `input_hash` and `output_hash` stay the zero "no binding"
sentinel in all three functions — `zheng::commit` explicitly skips its own
input/output check when they're zero (`zheng/rs/src/lib.rs:113-127`). So
`verify_expr_proof` now correctly verifies "a valid nox trace was executed
under this Statement," but still cannot check that the trace computed any
*specific* claimed program or result — a proof of `6*7=42` and a proof of
`3*3=9` verify identically today; nothing in the public statement
distinguishes them. Closing that needs a real `program_hash`/`output_hash`
convention wired from the nox trace format (R3 BbgSource integration, per
the existing doc comments) — not attempted here, and not a one-file fix.
