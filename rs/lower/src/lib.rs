//! Stage-3 lowering: inf expressions and conditions → nox patterns, executed on
//! the real nox VM (specs/ir.md, specs/proof.md).
//!
//! Three layers are implemented here:
//!
//! **Expression layer** (R1a, expression lowering):
//! Arithmetic and comparison terms compile to nox opcode formulas and run on the
//! nox VM. The result is differential-tested against the reference engine.
//!
//! **Condition layer** (R1a, condition routing):
//! Condition calls (gt, lt, eq, ne, le, ge, and, or, not) compile to nox
//! formulas where 0 = condition satisfied (the nox branch convention). The
//! `make_nox_ctx()` function returns an eval `Ctx` that routes every `Atom::Cond`
//! through the nox VM (falling back to the reference for non-numeric types),
//! enabling a whole-query differential test.
//!
//! **Relational scan/join layer** (R1b, full relational trace):
//! Relation reads compile to nox `look` (pattern 17) calls against a
//! `LocalLookProvider` (test mode). Scans and equi-joins are built as static
//! nox formulas that execute entirely on the real nox VM and are
//! differential-tested against the reference engine.
//!
//! nox boolean convention: 0 = true (condition satisfied); 1 = false.
//! Derived operator encodings over this convention:
//!   ne(a,b)  = lt(quote(0), eq(a,b))   — 0 when eq=1 (not equal)
//!   gt(a,b)  = lt(b, a)                — 0 when b<a (a>b)
//!   gte(a,b) = eq(lt(a,b), quote(1))   — 0 when lt=1 (a>=b)
//!   le(a,b)  = eq(lt(b,a), quote(1))   — 0 when lt(b,a)=1 (a<=b)
//!   and(c1,c2) = add(c1, c2)           — 0 iff both 0
//!   or(c1,c2)  = mul(c1, c2)           — 0 iff at least one 0
//!   not(c)     = eq(c, quote(1))        — 0 iff c=1 (was false)
//!
//! R1b look key encoding: key = row_idx * MAX_ARITY + col_idx.
//! Namespace = relation order index (0..9, nox limit).

use inf_ast::{Call, Term};
use inf_source::{LocalSource, RelationSource};
use inf_value::Value;
use nebu::Goldilocks;
use nox::{reduce, NounId, Order, Outcome, Tag};
use std::collections::BTreeMap;

const N: usize = 8192;
const BUDGET: u64 = 1_000_000;

pub type Binding = BTreeMap<String, Value>;

#[derive(Debug, PartialEq)]
pub enum LowerError {
    Unsupported(String),
    Arena,
    Runtime(String),
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn quote(o: &mut Order<N>, v: u64) -> Result<NounId, LowerError> {
    let one = o.atom(Goldilocks::new(1), Tag::Field).ok_or(LowerError::Arena)?;
    let val = o.atom(Goldilocks::new(v), Tag::Field).ok_or(LowerError::Arena)?;
    o.cell(one, val).ok_or(LowerError::Arena)
}

fn binary(o: &mut Order<N>, op: u64, a: NounId, b: NounId) -> Result<NounId, LowerError> {
    let body = o.cell(a, b).ok_or(LowerError::Arena)?;
    let tag = o.atom(Goldilocks::new(op), Tag::Field).ok_or(LowerError::Arena)?;
    o.cell(tag, body).ok_or(LowerError::Arena)
}

fn require2(c: &Call) -> Result<(), LowerError> {
    if c.args.len() == 2 { Ok(()) }
    else { Err(LowerError::Unsupported(format!("`{}` arity", c.func))) }
}

fn require1(c: &Call) -> Result<(), LowerError> {
    if c.args.len() == 1 { Ok(()) }
    else { Err(LowerError::Unsupported(format!("`{}` arity", c.func))) }
}

// ── core term lowering ────────────────────────────────────────────────────────

/// Lower an inf term to a nox formula that computes its numeric value.
/// Variables resolve to columns of the subject tuple via axis navigation.
/// Columns are right-nested: column i is at axis 2^(i+2) - 2.
fn lower_term(o: &mut Order<N>, t: &Term, cols: &[String]) -> Result<NounId, LowerError> {
    match t {
        Term::Int(i) if *i >= 0 => quote(o, *i as u64),
        Term::Field(u) => quote(o, *u),
        Term::Bool(b) => quote(o, if *b { 0 } else { 1 }),
        Term::Var(v) => {
            let idx = cols
                .iter()
                .position(|c| c == v)
                .ok_or_else(|| LowerError::Unsupported(format!("variable `{v}`")))?;
            let axis = (1u64 << (idx + 2)) - 2;
            let zero = o.atom(Goldilocks::new(0), Tag::Field).ok_or(LowerError::Arena)?;
            let ax = o.atom(Goldilocks::new(axis), Tag::Field).ok_or(LowerError::Arena)?;
            o.cell(zero, ax).ok_or(LowerError::Arena)
        }
        Term::Call(c) if c.sibling.is_none() => lower_call(o, c, cols),
        other => Err(LowerError::Unsupported(format!("{other:?}"))),
    }
}

/// Lower a condition call to a nox formula where 0 = condition satisfied.
fn lower_call(o: &mut Order<N>, c: &Call, cols: &[String]) -> Result<NounId, LowerError> {
    match c.func.as_str() {
        // arithmetic (returns numeric, not 0/1)
        "add" => { require2(c)?; let a = lower_term(o, &c.args[0], cols)?; let b = lower_term(o, &c.args[1], cols)?; binary(o, 5, a, b) }
        "sub" => { require2(c)?; let a = lower_term(o, &c.args[0], cols)?; let b = lower_term(o, &c.args[1], cols)?; binary(o, 6, a, b) }
        "mul" => { require2(c)?; let a = lower_term(o, &c.args[0], cols)?; let b = lower_term(o, &c.args[1], cols)?; binary(o, 7, a, b) }

        // equality: 0 = equal, 1 = not equal (nox eq pattern 9)
        "eq" => { require2(c)?; let a = lower_term(o, &c.args[0], cols)?; let b = lower_term(o, &c.args[1], cols)?; binary(o, 9, a, b) }

        // ne(a,b) = lt(0, eq(a,b)): 0 when eq=1 (not equal)
        "ne" | "neq" => {
            require2(c)?;
            let a = lower_term(o, &c.args[0], cols)?;
            let b = lower_term(o, &c.args[1], cols)?;
            let eq_ab = binary(o, 9, a, b)?;
            let zero = quote(o, 0)?;
            binary(o, 10, zero, eq_ab)
        }

        // lt: 0 when a < b (nox lt pattern 10)
        "lt" => { require2(c)?; let a = lower_term(o, &c.args[0], cols)?; let b = lower_term(o, &c.args[1], cols)?; binary(o, 10, a, b) }

        // gt(a,b) = lt(b,a): 0 when b < a
        "gt" => { require2(c)?; let a = lower_term(o, &c.args[0], cols)?; let b = lower_term(o, &c.args[1], cols)?; binary(o, 10, b, a) }

        // gte(a,b) = eq(lt(a,b), 1): 0 when lt=1 (a>=b)
        "ge" | "gte" => {
            require2(c)?;
            let a = lower_term(o, &c.args[0], cols)?;
            let b = lower_term(o, &c.args[1], cols)?;
            let lt_ab = binary(o, 10, a, b)?;
            let one = quote(o, 1)?;
            binary(o, 9, lt_ab, one)
        }

        // le(a,b) = eq(lt(b,a), 1): 0 when lt(b,a)=1 (a<=b)
        "le" | "lte" => {
            require2(c)?;
            let a = lower_term(o, &c.args[0], cols)?;
            let b = lower_term(o, &c.args[1], cols)?;
            let lt_ba = binary(o, 10, b, a)?;
            let one = quote(o, 1)?;
            binary(o, 9, lt_ba, one)
        }

        // and(c1,c2) = add(c1, c2): 0 iff both 0
        "and" => {
            require2(c)?;
            let a = lower_term(o, &c.args[0], cols)?;
            let b = lower_term(o, &c.args[1], cols)?;
            binary(o, 5, a, b)
        }

        // or(c1,c2) = mul(c1, c2): 0 iff at least one 0
        "or" => {
            require2(c)?;
            let a = lower_term(o, &c.args[0], cols)?;
            let b = lower_term(o, &c.args[1], cols)?;
            binary(o, 7, a, b)
        }

        // not(c) = eq(c, 1): 0 iff c=1 (c was false)
        "not" => {
            require1(c)?;
            let a = lower_term(o, &c.args[0], cols)?;
            let one = quote(o, 1)?;
            binary(o, 9, a, one)
        }

        _ => Err(LowerError::Unsupported(format!("operator `{}`", c.func))),
    }
}

// ── subject encoding ──────────────────────────────────────────────────────────

/// Build a right-nested cons-list subject from integer column values.
/// column 0 at axis 2, column 1 at axis 6 (= 2*3), column 2 at axis 14, etc.
fn tuple_noun(o: &mut Order<N>, tuple: &[i64]) -> Result<NounId, LowerError> {
    // right-nested: [v0 | [v1 | [v2 | NIL]]]
    // NIL = atom 0 (terminator)
    let mut acc = o.atom(Goldilocks::new(0), Tag::Field).ok_or(LowerError::Arena)?;
    for &v in tuple.iter().rev() {
        let val = o.atom(Goldilocks::new(v as u64), Tag::Field).ok_or(LowerError::Arena)?;
        acc = o.cell(val, acc).ok_or(LowerError::Arena)?;
    }
    Ok(acc)
}

// ── execution helpers ─────────────────────────────────────────────────────────

fn run(o: &mut Order<N>, subject: NounId, formula: NounId) -> Result<u64, LowerError> {
    let calls = nox::NullCalls;
    let mut tracer = nox::NoTrace;
    match reduce(o, subject, formula, BUDGET, &calls, &mut tracer) {
        Outcome::Ok(id, _) => {
            let (g, _) = o.atom_value(id).ok_or(LowerError::Runtime("non-atom result".into()))?;
            Ok(g.as_u64())
        }
        Outcome::Halt(_) => Err(LowerError::Runtime("halted".into())),
        Outcome::Error(e) => Err(LowerError::Runtime(format!("{e:?}"))),
    }
}

// ── public expression API (previously complete, kept for compatibility) ────────

/// Lower a closed inf expression and run it on the nox VM.
pub fn lower_and_run(t: &Term) -> Result<u64, LowerError> {
    let mut o: Order<N> = Order::new();
    let formula = lower_term(&mut o, t, &[])?;
    let subject = o.atom(Goldilocks::new(0), Tag::Field).ok_or(LowerError::Arena)?;
    run(&mut o, subject, formula)
}

/// Lower an expression over tuple columns and run it on the nox VM.
pub fn lower_and_run_tuple(t: &Term, cols: &[String], tuple: &[i64]) -> Result<u64, LowerError> {
    let mut o: Order<N> = Order::new();
    let formula = lower_term(&mut o, t, cols)?;
    let subject = tuple_noun(&mut o, tuple)?;
    run(&mut o, subject, formula)
}

// ── R1 full: condition evaluation via nox ────────────────────────────────────

/// Collect all Var names referenced in a term (no duplicates, discovery order).
fn collect_vars(t: &Term, out: &mut Vec<String>) {
    match t {
        Term::Var(v) => {
            if !out.contains(v) {
                out.push(v.clone());
            }
        }
        Term::Call(c) => {
            for arg in &c.args {
                collect_vars(arg, out);
            }
        }
        Term::List(items) => {
            for item in items {
                collect_vars(item, out);
            }
        }
        _ => {}
    }
}

/// Try to evaluate a condition call via the nox VM.
///
/// Extracts numeric variable values from `binding`, compiles the condition to a
/// nox formula, and runs it. Returns:
/// - `Some(true)`  — condition satisfied (nox returned 0)
/// - `Some(false)` — condition not satisfied (nox returned non-zero)
/// - `None`        — can't lower (non-numeric variable; caller falls back to reference)
pub fn eval_cond_via_nox(call: &Call, binding: &Binding) -> Option<bool> {
    if call.sibling.is_some() {
        return None; // sibling calls are live/host, not lowerable
    }
    let t = Term::Call(Box::new(call.clone()));
    let mut var_names: Vec<String> = Vec::new();
    collect_vars(&t, &mut var_names);

    // require all referenced vars to be numeric
    let mut vals: Vec<i64> = Vec::new();
    for v in &var_names {
        match binding.get(v)? {
            Value::Int(i) => vals.push(*i),
            Value::Field(f) => vals.push(f.val() as i64),
            Value::Bool(b) => vals.push(if *b { 0 } else { 1 }),
            _ => return None,
        }
    }

    let mut o: Order<N> = Order::new();
    let formula = lower_term(&mut o, &t, &var_names).ok()?;
    let subject = tuple_noun(&mut o, &vals).ok()?;
    match run(&mut o, subject, formula) {
        Ok(v) => Some(v == 0), // 0 = condition satisfied
        Err(_) => None,
    }
}

/// Build an eval `Ctx` that routes arithmetic/comparison conditions through the
/// nox VM, falling back to the reference engine for non-numeric types.
///
/// Used in the corpus differential tests: run the same program with nox ctx and
/// with the default ctx and assert the result sets are identical.
pub fn make_nox_ctx() -> inf_eval::Ctx {
    inf_eval::Ctx {
        self_neuron: inf_value::tag_hash("@me"),
        host: None,
        nox_cond: Some(Box::new(|call, binding| eval_cond_via_nox(call, binding))),
    }
}

// ── R2: zheng proof generation ───────────────────────────────────────────────

/// Prove that a closed expression evaluates to `result` on the nox VM.
///
/// Records the nox execution trace with `VecTrace`, then passes it to
/// `zheng::commit` (SuperSpartan + Brakedown). Returns the result field value
/// and the `TraceProof`. The proof can be checked with `verify_expr_proof`.
///
/// Requires the `prove` feature (`cargo build --features prove`).
#[cfg(feature = "prove")]
pub fn prove_expr(t: &Term) -> Result<(u64, zheng::TraceProof), LowerError> {
    use nox::VecTrace;
    let mut o: Order<N> = Order::new();
    let formula = lower_term(&mut o, t, &[])?;
    let subject = o.atom(Goldilocks::new(0), Tag::Field).ok_or(LowerError::Arena)?;
    let calls = nox::NullCalls;
    let mut tracer = VecTrace::default();
    let result_id = match reduce(&mut o, subject, formula, BUDGET, &calls, &mut tracer) {
        Outcome::Ok(id, _) => id,
        Outcome::Halt(_) => return Err(LowerError::Runtime("halted".into())),
        Outcome::Error(e) => return Err(LowerError::Runtime(format!("{e:?}"))),
    };
    let (g, _) = o.atom_value(result_id).ok_or(LowerError::Runtime("non-atom result".into()))?;
    let result = g.as_u64();

    // Prove the trace. Pure arithmetic traces have no hash, axis, or look rows —
    // pass empty slices. The Statement uses zero hashes (no BBG root binding yet;
    // that arrives with R3 BbgSource integration).
    let statement = zheng::Statement {
        program_hash: [0u8; 32],
        input_hash: [0u8; 32],
        output_hash: [0u8; 32],
        focus_bound: tracer.0.len() as u64,
    };
    let params = zheng::ProofParams::default();
    let proof = zheng::commit(&tracer, &[], &[], &[], &statement, &params)
        .map_err(|e| LowerError::Runtime(format!("zheng commit: {e:?}")))?;
    Ok((result, proof))
}

/// Verify a proof produced by `prove_expr`.
#[cfg(feature = "prove")]
pub fn verify_expr_proof(proof: &zheng::TraceProof) -> Result<(), LowerError> {
    let statement = zheng::Statement {
        program_hash: [0u8; 32],
        input_hash: [0u8; 32],
        output_hash: [0u8; 32],
        focus_bound: u64::MAX,
    };
    let params = zheng::ProofParams::default();
    zheng::verify(proof, &statement, &params)
        .map_err(|e| LowerError::Runtime(format!("zheng verify: {e:?}")))
}

/// Prove that a condition holds for a tuple: run the condition on the nox VM,
/// record the trace, and prove it via zheng.
#[cfg(feature = "prove")]
pub fn prove_cond(
    cond: &inf_ast::Call,
    col_names: &[String],
    tuple: &[i64],
) -> Result<(bool, zheng::TraceProof), LowerError> {
    use nox::VecTrace;
    let t = Term::Call(Box::new(cond.clone()));
    let mut o: Order<N> = Order::new();
    let formula = lower_term(&mut o, &t, col_names)?;
    let subject = tuple_noun(&mut o, tuple)?;
    let calls = nox::NullCalls;
    let mut tracer = VecTrace::default();
    let result_id = match reduce(&mut o, subject, formula, BUDGET, &calls, &mut tracer) {
        Outcome::Ok(id, _) => id,
        Outcome::Halt(_) => return Err(LowerError::Runtime("halted".into())),
        Outcome::Error(e) => return Err(LowerError::Runtime(format!("{e:?}"))),
    };
    let (g, _) = o.atom_value(result_id).ok_or(LowerError::Runtime("non-atom result".into()))?;
    let holds = g.as_u64() == 0;

    let statement = zheng::Statement {
        program_hash: [0u8; 32],
        input_hash: [0u8; 32],
        output_hash: [0u8; 32],
        focus_bound: tracer.0.len() as u64,
    };
    let params = zheng::ProofParams::default();
    let proof = zheng::commit(&tracer, &[], &[], &[], &statement, &params)
        .map_err(|e| LowerError::Runtime(format!("zheng commit: {e:?}")))?;
    Ok((holds, proof))
}

// ── R1b: relational scan and join via nox look pattern ───────────────────────
//
// key encoding: key = row_idx * MAX_ARITY + col_idx.
// Namespace = index of the relation in `rel_order` (must be 0..9).
// The fake BBG object uses all-zero limbs; LookProvider ignores the commitment.

/// Max column count per row for the look key encoding.
pub const MAX_ARITY: u64 = 16;

fn val_to_u64(v: &Value) -> u64 {
    match v {
        Value::Int(i)   => *i as u64,
        Value::Field(f) => f.val(),
        Value::Bool(b)  => *b as u64,
        _               => 0,
    }
}

/// Look provider backed by LocalSource data (R1b test mode).
///
/// Namespaces are positional in the `rel_order` slice (max 10 relations).
/// Commitment argument is ignored — LocalSource has no BBG root.
pub struct LocalLookProvider {
    ns_rows: Vec<Vec<Vec<u64>>>,  // ns → rows → col values (field elements)
}

impl LocalLookProvider {
    pub fn new(src: &LocalSource, rel_order: &[&str]) -> Self {
        let ns_rows = rel_order.iter()
            .map(|rel| src.scan(rel).map(|row| row.iter().map(val_to_u64).collect()).collect())
            .collect();
        LocalLookProvider { ns_rows }
    }
}

impl nox::LookProvider for LocalLookProvider {
    fn look(&self, _ct: Goldilocks, ns: Goldilocks, key: Goldilocks) -> Option<Goldilocks> {
        let k = key.as_u64();
        let v = self.ns_rows
            .get(ns.as_u64() as usize)?
            .get((k / MAX_ARITY) as usize)?
            .get((k % MAX_ARITY) as usize)?;
        Some(Goldilocks::new(*v))
    }
}

impl<const NN: usize> nox::CallProvider<NN> for LocalLookProvider {
    fn provide(&self, _o: &mut Order<NN>, _tag: Goldilocks, _obj: NounId) -> Option<NounId> {
        None
    }
}

// ── R1b formula builders ──────────────────────────────────────────────────────

/// Build the fake BBG subject: [[0|[0|[0|0]]] | 0]
/// look pattern extracts limbs from axes [4,10,22,23].
fn bbg_obj(o: &mut Order<N>) -> Result<NounId, LowerError> {
    let l0 = o.atom(Goldilocks::ZERO, Tag::Field).ok_or(LowerError::Arena)?;
    let l1 = o.atom(Goldilocks::ZERO, Tag::Field).ok_or(LowerError::Arena)?;
    let l2 = o.atom(Goldilocks::ZERO, Tag::Field).ok_or(LowerError::Arena)?;
    let l3 = o.atom(Goldilocks::ZERO, Tag::Field).ok_or(LowerError::Arena)?;
    let inner = o.cell(l2, l3).ok_or(LowerError::Arena)?;
    let mid   = o.cell(l1, inner).ok_or(LowerError::Arena)?;
    let root  = o.cell(l0, mid).ok_or(LowerError::Arena)?;
    let rest  = o.atom(Goldilocks::ZERO, Tag::Field).ok_or(LowerError::Arena)?;
    o.cell(root, rest).ok_or(LowerError::Arena)
}

/// [17 [[1 ns] [1 key]]] — look formula
fn lf(o: &mut Order<N>, ns: u64, key: u64) -> Result<NounId, LowerError> {
    let op  = o.atom(Goldilocks::new(17), Tag::Field).ok_or(LowerError::Arena)?;
    let nsf = quote(o, ns)?;
    let kf  = quote(o, key)?;
    let bdy = o.cell(nsf, kf).ok_or(LowerError::Arena)?;
    o.cell(op, bdy).ok_or(LowerError::Arena)
}

/// [3 [a b]] — cons formula (construct a cell from two sub-formula results)
fn cf(o: &mut Order<N>, a: NounId, b: NounId) -> Result<NounId, LowerError> {
    let op  = o.atom(Goldilocks::new(3), Tag::Field).ok_or(LowerError::Arena)?;
    let bdy = o.cell(a, b).ok_or(LowerError::Arena)?;
    o.cell(op, bdy).ok_or(LowerError::Arena)
}

/// [4 [cond [yes no]]] — branch: takes yes-arm when cond == 0
fn brf(o: &mut Order<N>, cond: NounId, yes: NounId, no: NounId) -> Result<NounId, LowerError> {
    let op   = o.atom(Goldilocks::new(4), Tag::Field).ok_or(LowerError::Arena)?;
    let arms = o.cell(yes, no).ok_or(LowerError::Arena)?;
    let bdy  = o.cell(cond, arms).ok_or(LowerError::Arena)?;
    o.cell(op, bdy).ok_or(LowerError::Arena)
}

/// Build output tuple formula: [3 [look_col0 [3 [look_col1 ... [1 0]]]]]
fn tuple_f(o: &mut Order<N>, ns: u64, row: usize, cols: &[usize]) -> Result<NounId, LowerError> {
    let mut acc = quote(o, 0)?;  // NIL terminator
    for &col in cols.iter().rev() {
        let lk = lf(o, ns, row as u64 * MAX_ARITY + col as u64)?;
        acc = cf(o, lk, acc)?;
    }
    Ok(acc)
}

/// Decode a nox cons-list of tuples: [[c0|[c1|0]] | [[c0|...] | 0]] → Vec<Vec<u64>>
fn decode_results(o: &Order<N>, id: NounId) -> Vec<Vec<u64>> {
    let mut rows = Vec::new();
    let mut cur = id;
    while o.is_cell(cur) {
        let head = o.head(cur).unwrap();
        cur = o.tail(cur).unwrap();
        rows.push(decode_tuple(o, head));
    }
    rows
}

fn decode_tuple(o: &Order<N>, id: NounId) -> Vec<u64> {
    let mut cols = Vec::new();
    let mut cur = id;
    while o.is_cell(cur) {
        let head = o.head(cur).unwrap();
        cur = o.tail(cur).unwrap();
        if let Some((g, _)) = o.atom_value(head) {
            cols.push(g.as_u64());
        }
    }
    cols
}

/// Run a relational formula against the fake BBG object with a LocalLookProvider.
fn run_rel(o: &mut Order<N>, formula: NounId, prov: &LocalLookProvider) -> Result<NounId, LowerError> {
    let subj = bbg_obj(o)?;
    let mut tracer = nox::NoTrace;
    match reduce(o, subj, formula, BUDGET, prov, &mut tracer) {
        Outcome::Ok(id, _) => Ok(id),
        Outcome::Halt(_)   => Err(LowerError::Runtime("halted".into())),
        Outcome::Error(e)  => Err(LowerError::Runtime(format!("look-scan: {e:?}"))),
    }
}

// ── R1b public API ────────────────────────────────────────────────────────────

/// Scan a relation through the nox VM via look calls. Returns all rows sorted.
///
/// `out_cols`: column indices (0-based) to include in each output tuple.
pub fn nox_vm_scan(src: &LocalSource, rel: &str, out_cols: &[usize]) -> Result<Vec<Vec<u64>>, LowerError> {
    let prov = LocalLookProvider::new(src, &[rel]);
    let n_rows = src.scan(rel).count();
    let mut o: Order<N> = Order::new();

    // Build cons-list formula from the last row upward
    let mut acc = quote(&mut o, 0)?;  // NIL base
    for row in (0..n_rows).rev() {
        let tup = tuple_f(&mut o, 0, row, out_cols)?;
        acc = cf(&mut o, tup, acc)?;
    }

    let result = run_rel(&mut o, acc, &prov)?;
    let mut rows = decode_results(&o, result);
    rows.sort();
    Ok(rows)
}

/// Scan with a gt filter on one column via nox. Returns matching rows sorted.
///
/// Keeps rows where `out_cols[filter_col] > threshold`.
/// `filter_col` is an index into the relation's columns (not into out_cols).
pub fn nox_vm_scan_gt(
    src: &LocalSource, rel: &str,
    filter_col: usize, threshold: u64,
    out_cols: &[usize],
) -> Result<Vec<Vec<u64>>, LowerError> {
    let prov = LocalLookProvider::new(src, &[rel]);
    let n_rows = src.scan(rel).count();
    let mut o: Order<N> = Order::new();

    let mut acc = quote(&mut o, 0)?;
    for row in (0..n_rows).rev() {
        // gt(col, threshold) = lt(threshold, look(ns, key)) = 0 iff look > threshold
        let lk    = lf(&mut o, 0, row as u64 * MAX_ARITY + filter_col as u64)?;
        let thresh = quote(&mut o, threshold)?;
        let cond  = binary(&mut o, 10, thresh, lk)?;
        let tup   = tuple_f(&mut o, 0, row, out_cols)?;
        let inc   = cf(&mut o, tup, acc)?;
        acc       = brf(&mut o, cond, inc, acc)?;
    }

    let result = run_rel(&mut o, acc, &prov)?;
    let mut rows = decode_results(&o, result);
    rows.sort();
    Ok(rows)
}

/// Equi-join two relations on a shared column via nox look formulas.
///
/// For each pair (r1, r2) where rel1[r1][jcol1] == rel2[r2][jcol2], output
/// the specified columns. Optional extra filter: rel2[r2][filter_col2] > threshold.
pub fn nox_vm_equijoin(
    src: &LocalSource,
    rel1: &str, jcol1: usize,
    rel2: &str, jcol2: usize,
    out1: &[usize], out2: &[usize],
    extra_gt: Option<(usize, u64)>,  // optional (col_idx, threshold) filter on rel2
) -> Result<Vec<Vec<u64>>, LowerError> {
    let prov  = LocalLookProvider::new(src, &[rel1, rel2]);
    let n1 = src.scan(rel1).count();
    let n2 = src.scan(rel2).count();
    let mut o: Order<N> = Order::new();
    let (ns1, ns2) = (0u64, 1u64);

    let mut acc = quote(&mut o, 0)?;  // NIL

    for r1 in (0..n1).rev() {
        for r2 in (0..n2).rev() {
            // Equi-join condition: eq(look1, look2) = 0 iff equal
            let lk1 = lf(&mut o, ns1, r1 as u64 * MAX_ARITY + jcol1 as u64)?;
            let lk2 = lf(&mut o, ns2, r2 as u64 * MAX_ARITY + jcol2 as u64)?;
            let jcond = binary(&mut o, 9, lk1, lk2)?;

            // Combined condition (add: 0 iff both zero)
            let cond = if let Some((fc, thr)) = extra_gt {
                let lkfc = lf(&mut o, ns2, r2 as u64 * MAX_ARITY + fc as u64)?;
                let thrq = quote(&mut o, thr)?;
                let fcond = binary(&mut o, 10, thrq, lkfc)?;  // lt(thr, col): 0 iff col>thr
                binary(&mut o, 5, jcond, fcond)?  // add: 0 iff both pass
            } else {
                jcond
            };

            // Output tuple: cols from rel1, then cols from rel2
            let mut parts = Vec::new();
            for &c in out1 { parts.push(lf(&mut o, ns1, r1 as u64 * MAX_ARITY + c as u64)?); }
            for &c in out2 { parts.push(lf(&mut o, ns2, r2 as u64 * MAX_ARITY + c as u64)?); }
            let mut tup = quote(&mut o, 0)?;
            for &p in parts.iter().rev() { tup = cf(&mut o, p, tup)?; }

            let inc = cf(&mut o, tup, acc)?;
            acc = brf(&mut o, cond, inc, acc)?;
        }
    }

    let result = run_rel(&mut o, acc, &prov)?;
    let mut rows = decode_results(&o, result);
    rows.sort();
    Ok(rows)
}

// ── R1c: bounded recursion trace via nox ─────────────────────────────────────
//
// One iteration = one nox formula per step. The frontier is encoded as a
// Vec<u64> in Rust; the nox step formula uses literal field constants for each
// frontier member (rebuilt each iteration). Rust manages the bounded loop.

/// Decode a flat cons-list of atoms: [v0 | [v1 | ... | NIL]] → Vec<u64>
fn decode_atom_list(o: &Order<N>, id: NounId) -> Vec<u64> {
    let mut vals = Vec::new();
    let mut cur = id;
    while o.is_cell(cur) {
        let head = o.head(cur).unwrap();
        cur = o.tail(cur).unwrap();
        if let Some((g, _)) = o.atom_value(head) {
            vals.push(g.as_u64());
        }
    }
    vals
}

/// Bounded reachability via nox look formulas.
///
/// Starting from `seed`, follows `edge_rel[from_col, to_col]` edges for at most
/// `extra_steps` ADDITIONAL hops beyond the base case. Total effective rounds = 1 + extra_steps.
///
/// Equivalent to:
/// ```datalog
/// reach[n] := edge{from: seed, to: n}
/// reach[n] := reach[m], edge{from: m, to: n}
/// :bounded (1 + extra_steps)
/// ```
///
/// Each iteration builds a fresh nox formula that uses the current frontier
/// members as literal constants in eq conditions. The formula is executed on
/// the real nox VM against a LocalLookProvider.
pub fn nox_vm_bounded_reach(
    src: &LocalSource,
    edge_rel: &str,
    from_col: usize,
    to_col: usize,
    seed: u64,
    extra_steps: usize,
) -> Result<Vec<u64>, LowerError> {
    let prov = LocalLookProvider::new(src, &[edge_rel]);
    let n_edges = src.scan(edge_rel).count();

    // One step: given a frontier (set of source nodes), return all target nodes.
    // Builds a nox formula: for each edge (from, to), if from ∈ frontier → include to.
    // or(eq(from, f0), eq(from, f1), ...) = mul of all eq formulas; 0 iff any match.
    let one_step = |frontier: &[u64]| -> Result<Vec<u64>, LowerError> {
        let mut o: Order<N> = Order::new();
        let mut acc = quote(&mut o, 0)?;
        for r in (0..n_edges).rev() {
            let lk_from = lf(&mut o, 0, r as u64 * MAX_ARITY + from_col as u64)?;
            let lk_to   = lf(&mut o, 0, r as u64 * MAX_ARITY + to_col as u64)?;

            // cond = mul(eq(lk_from, f0), eq(lk_from, f1), ...): 0 iff any match
            let mut cond = quote(&mut o, 1)?;  // 1 = "no frontier" sentinel
            for &f in frontier {
                let fq   = quote(&mut o, f)?;
                let eq_f = binary(&mut o, 9, lk_from, fq)?;
                cond     = binary(&mut o, 7, cond, eq_f)?;  // mul (0 iff any is 0)
            }
            let inc = cf(&mut o, lk_to, acc)?;
            acc     = brf(&mut o, cond, inc, acc)?;
        }
        let result = run_rel(&mut o, acc, &prov)?;
        Ok(decode_atom_list(&o, result))
    };

    // Base case: direct neighbors of seed
    let base = one_step(&[seed])?;
    let mut reached: Vec<u64> = base.clone();
    let mut frontier: Vec<u64> = base;
    reached.sort(); reached.dedup();

    // Additional steps
    for _ in 0..extra_steps {
        if frontier.is_empty() { break; }
        let next = one_step(&frontier)?;
        let mut new_frontier = Vec::new();
        for n in next {
            if !reached.contains(&n) {
                reached.push(n);
                new_frontier.push(n);
            }
        }
        frontier = new_frontier;
    }

    reached.sort();
    Ok(reached)
}

// ── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use inf_ast::{Call, Term};
    use inf_eval::eval;
    use inf_source::LocalSource;
    use inf_value::Value;

    fn call(func: &str, a: Term, b: Term) -> Term {
        Term::Call(Box::new(Call { sibling: None, func: func.into(), args: vec![a, b] }))
    }
    fn int(i: i64) -> Term {
        Term::Int(i)
    }
    fn var(v: &str) -> Term {
        Term::Var(v.into())
    }

    fn assert_numeric_match(t: &Term) {
        let nox_val = lower_and_run(t).expect("lower+run");
        let reference = inf_eval::eval_const_term(t).expect("eval");
        let want = match reference {
            Value::Int(i) => (i as u128 % inf_value::P as u128) as u64,
            Value::Field(f) => f.val(),
            other => panic!("non-numeric reference {other:?}"),
        };
        assert_eq!(nox_val, want, "nox vs reference mismatch for {t:?}");
    }

    #[test]
    fn arithmetic_lowers_and_matches_reference() {
        assert_numeric_match(&call("add", int(2), int(3)));
        assert_numeric_match(&call("mul", int(6), int(7)));
        assert_numeric_match(&call("sub", int(10), int(3)));
        assert_numeric_match(&call("add", call("mul", int(2), int(3)), int(4)));
    }

    #[test]
    fn tuple_columns_lower_and_run_on_nox() {
        fn check(t: &Term, cols: &[&str], tuple: &[i64]) {
            let cs: Vec<String> = cols.iter().map(|s| s.to_string()).collect();
            let nox_val = lower_and_run_tuple(t, &cs, tuple).expect("lower+run");
            let reference = inf_eval::eval_term_in(t, &cs, tuple).expect("eval");
            let want = match reference {
                Value::Int(i) => (i as u128 % inf_value::P as u128) as u64,
                Value::Field(f) => f.val(),
                other => panic!("non-numeric reference {other:?}"),
            };
            assert_eq!(nox_val, want, "nox vs reference for {t:?}");
        }
        check(&call("add", call("mul", var("a"), var("b")), var("c")), &["a", "b", "c"], &[10, 3, 7]);
        check(&call("mul", call("add", var("x"), var("y")), var("x")), &["x", "y"], &[5, 2]);
    }

    #[test]
    fn runs_on_the_real_nox_vm() {
        assert_eq!(lower_and_run(&call("add", int(40), int(2))).unwrap(), 42);
    }

    #[test]
    fn unsupported_is_reported_not_faked() {
        let r = lower_and_run(&Term::Str("x".into()));
        assert!(matches!(r, Err(LowerError::Unsupported(_))));
    }

    // ── R1 full: condition evaluation ─────────────────────────────────────────

    fn cond_call(func: &str, a: Term, b: Term) -> Call {
        Call { sibling: None, func: func.into(), args: vec![a, b] }
    }

    fn binding_from(pairs: &[(&str, i64)]) -> Binding {
        pairs.iter().map(|(k, v)| (k.to_string(), Value::Int(*v))).collect()
    }

    #[test]
    fn nox_cond_lt() {
        let c = cond_call("lt", var("d"), int(4));
        let b3 = binding_from(&[("d", 3)]);
        let b5 = binding_from(&[("d", 5)]);
        assert_eq!(eval_cond_via_nox(&c, &b3), Some(true),  "3 < 4 should pass");
        assert_eq!(eval_cond_via_nox(&c, &b5), Some(false), "5 < 4 should fail");
    }

    #[test]
    fn nox_cond_gt() {
        let c = cond_call("gt", var("x"), int(0));
        let b1 = binding_from(&[("x", 1)]);
        let b0 = binding_from(&[("x", 0)]);
        assert_eq!(eval_cond_via_nox(&c, &b1), Some(true));
        assert_eq!(eval_cond_via_nox(&c, &b0), Some(false));
    }

    #[test]
    fn nox_cond_eq_and_ne() {
        let ceq = cond_call("eq", var("a"), var("b"));
        let cne = cond_call("ne", var("a"), var("b"));
        let same = binding_from(&[("a", 7), ("b", 7)]);
        let diff = binding_from(&[("a", 7), ("b", 8)]);
        assert_eq!(eval_cond_via_nox(&ceq, &same), Some(true));
        assert_eq!(eval_cond_via_nox(&ceq, &diff), Some(false));
        assert_eq!(eval_cond_via_nox(&cne, &same), Some(false));
        assert_eq!(eval_cond_via_nox(&cne, &diff), Some(true));
    }

    #[test]
    fn nox_cond_ge_le() {
        let cge = cond_call("ge", var("x"), var("y"));
        let cle = cond_call("le", var("x"), var("y"));
        let b55 = binding_from(&[("x", 5), ("y", 5)]);
        let b35 = binding_from(&[("x", 3), ("y", 5)]);
        // ge: x>=y
        assert_eq!(eval_cond_via_nox(&cge, &b55), Some(true),  "5>=5");
        assert_eq!(eval_cond_via_nox(&cge, &b35), Some(false), "3>=5");
        // le: x<=y
        assert_eq!(eval_cond_via_nox(&cle, &b55), Some(true),  "5<=5");
        assert_eq!(eval_cond_via_nox(&cle, &b35), Some(true),  "3<=5");
    }

    #[test]
    fn nox_cond_and_or() {
        let a5 = var("a"); let b3 = var("b");
        let lt_a = Call { sibling: None, func: "lt".into(), args: vec![a5.clone(), int(10)] };
        let gt_b = Call { sibling: None, func: "gt".into(), args: vec![b3.clone(), int(0)] };
        let cand = Call {
            sibling: None,
            func: "and".into(),
            args: vec![Term::Call(Box::new(lt_a.clone())), Term::Call(Box::new(gt_b.clone()))],
        };
        let cor = Call {
            sibling: None,
            func: "or".into(),
            args: vec![Term::Call(Box::new(lt_a.clone())), Term::Call(Box::new(gt_b.clone()))],
        };
        let both = binding_from(&[("a", 5), ("b", 3)]);
        let fail_b = binding_from(&[("a", 5), ("b", 0)]);
        assert_eq!(eval_cond_via_nox(&cand, &both),   Some(true),  "and: both pass");
        assert_eq!(eval_cond_via_nox(&cand, &fail_b), Some(false), "and: b fails");
        assert_eq!(eval_cond_via_nox(&cor,  &both),   Some(true),  "or: both pass");
        assert_eq!(eval_cond_via_nox(&cor,  &fail_b), Some(true),  "or: a still passes");
    }

    #[test]
    fn nox_cond_returns_none_for_hash_vars() {
        let c = cond_call("eq", var("x"), int(0));
        let b_hash: Binding = std::iter::once(("x".to_string(), Value::Hash([0u8; 32]))).collect();
        assert_eq!(eval_cond_via_nox(&c, &b_hash), None, "hash var → None (fallback to ref)");
    }

    // ── R1 full: whole-query differential test ────────────────────────────────

    fn fixture() -> LocalSource {
        use inf_value::Value::Int;
        let mut src = LocalSource::new();
        src.add("edge", &["from", "to"], vec![
            vec![Int(1), Int(2)],
            vec![Int(2), Int(3)],
            vec![Int(3), Int(4)],
            vec![Int(4), Int(5)],
        ]);
        src.add("node", &["id", "weight"], vec![
            vec![Int(1), Int(10)],
            vec![Int(2), Int(20)],
            vec![Int(3), Int(5)],
            vec![Int(4), Int(15)],
            vec![Int(5), Int(8)],
        ]);
        src
    }

    fn parse_and_plan(prog: &str) -> inf_ast::IrProgram {
        let ast = inf_parse::parse(prog).expect("parse");
        inf_plan::plan(&ast).expect("plan")
    }

    /// Run a program with both the reference engine and the nox-condition engine,
    /// assert the result sets are identical.
    fn assert_nox_matches_reference(prog: &str, src: &LocalSource) {
        let ir = parse_and_plan(prog);
        let ref_ctx = inf_eval::Ctx::default();
        let nox_ctx = make_nox_ctx();
        let ref_out = eval(&ir, src, &ref_ctx).expect("reference eval");
        let nox_out = eval(&ir, src, &nox_ctx).expect("nox eval");
        assert_eq!(ref_out.rows, nox_out.rows,
            "nox vs reference mismatch\nreference: {:?}\nnox: {:?}", ref_out.rows, nox_out.rows);
    }

    #[test]
    fn whole_query_filter_gt_matches_reference() {
        let src = fixture();
        assert_nox_matches_reference(
            "?[id, w] := node{id: id, weight: w}, gt(w, 10)",
            &src,
        );
    }

    #[test]
    fn whole_query_filter_lt_matches_reference() {
        let src = fixture();
        assert_nox_matches_reference(
            "?[id] := node{id: id, weight: w}, lt(w, 15)",
            &src,
        );
    }

    #[test]
    fn whole_query_join_with_filter_matches_reference() {
        let src = fixture();
        assert_nox_matches_reference(
            "?[f, t] := edge{from: f, to: t}, node{id: f, weight: w}, gt(w, 10)",
            &src,
        );
    }

    #[test]
    fn whole_query_bounded_reachability_matches_reference() {
        let src = fixture();
        // Simple bounded reachability: nodes reachable from node 1 within 2 hops.
        assert_nox_matches_reference(
            "reach[n] := edge{from: 1, to: n}\n\
             reach[n] := reach[m], edge{from: m, to: n}\n\
             :bounded 2\n\
             ?[n] := reach[n]",
            &src,
        );
    }

    #[test]
    fn whole_query_eq_condition_matches_reference() {
        let src = fixture();
        assert_nox_matches_reference(
            "?[f, t] := edge{from: f, to: t}, eq(f, 2)",
            &src,
        );
    }

    // ── R1b: nox VM relational scan/join tests ────────────────────────────────

    fn fixture_src() -> LocalSource {
        use inf_value::Value::Int;
        let mut src = LocalSource::new();
        src.add("node", &["id", "weight"], vec![
            vec![Int(1), Int(10)],
            vec![Int(2), Int(20)],
            vec![Int(3), Int(5)],
            vec![Int(4), Int(15)],
        ]);
        src.add("edge", &["from", "to"], vec![
            vec![Int(1), Int(2)],
            vec![Int(2), Int(3)],
            vec![Int(3), Int(4)],
        ]);
        src
    }

    /// Reference: collect rows as Vec<Vec<u64>>
    fn ref_scan(src: &LocalSource, rel: &str, out_cols: &[usize]) -> Vec<Vec<u64>> {
        let mut rows: Vec<Vec<u64>> = src.scan(rel)
            .map(|row| out_cols.iter().map(|&c| val_to_u64(&row[c])).collect())
            .collect();
        rows.sort();
        rows
    }

    #[test]
    fn nox_scan_all_rows_matches_reference() {
        let src = fixture_src();
        let nox_rows = nox_vm_scan(&src, "node", &[0, 1]).unwrap();
        let ref_rows = ref_scan(&src, "node", &[0, 1]);
        assert_eq!(nox_rows, ref_rows, "nox scan all rows vs reference");
    }

    #[test]
    fn nox_scan_single_column() {
        let src = fixture_src();
        let nox_rows = nox_vm_scan(&src, "node", &[0]).unwrap();
        let ref_rows = ref_scan(&src, "node", &[0]);
        assert_eq!(nox_rows, ref_rows, "nox scan id column only");
    }

    #[test]
    fn nox_scan_gt_filter_matches_reference() {
        let src = fixture_src();
        // Keep nodes with weight > 10 (ids 2 and 4)
        let nox_rows = nox_vm_scan_gt(&src, "node", 1, 10, &[0, 1]).unwrap();
        let ref_rows: Vec<Vec<u64>> = {
            let mut r: Vec<Vec<u64>> = src.scan("node")
                .filter(|row| val_to_u64(&row[1]) > 10)
                .map(|row| vec![val_to_u64(&row[0]), val_to_u64(&row[1])])
                .collect();
            r.sort(); r
        };
        assert_eq!(nox_rows, ref_rows, "nox scan+gt filter vs reference");
    }

    #[test]
    fn nox_equijoin_matches_reference() {
        let src = fixture_src();
        // edge{from: f, to: t} join node{id: f, weight: w} → output (f, t)
        // join on edge.from (col 0) == node.id (col 0)
        let nox_rows = nox_vm_equijoin(&src,
            "edge", 0,    // rel1 = edge, join on col 0 (from)
            "node", 0,    // rel2 = node, join on col 0 (id)
            &[0, 1],      // output cols from edge (from, to)
            &[],          // no output cols from node
            None,         // no extra filter
        ).unwrap();

        // Reference: equi-join
        let ref_rows: Vec<Vec<u64>> = {
            let edges: Vec<_> = src.scan("edge").collect();
            let nodes: Vec<_> = src.scan("node").collect();
            let mut r = Vec::new();
            for e in &edges {
                for n in &nodes {
                    if val_to_u64(&e[0]) == val_to_u64(&n[0]) {
                        r.push(vec![val_to_u64(&e[0]), val_to_u64(&e[1])]);
                    }
                }
            }
            r.sort(); r
        };
        assert_eq!(nox_rows, ref_rows, "nox equijoin vs reference");
    }

    #[test]
    fn nox_equijoin_with_filter_matches_reference() {
        let src = fixture_src();
        // edge join node on from==id, filter node.weight > 10 → output (from, to)
        let nox_rows = nox_vm_equijoin(&src,
            "edge", 0,
            "node", 0,
            &[0, 1],
            &[],
            Some((1, 10)),  // node.weight (col 1) > 10
        ).unwrap();

        let ref_rows: Vec<Vec<u64>> = {
            let edges: Vec<_> = src.scan("edge").collect();
            let nodes: Vec<_> = src.scan("node").collect();
            let mut r = Vec::new();
            for e in &edges {
                for n in &nodes {
                    if val_to_u64(&e[0]) == val_to_u64(&n[0]) && val_to_u64(&n[1]) > 10 {
                        r.push(vec![val_to_u64(&e[0]), val_to_u64(&e[1])]);
                    }
                }
            }
            r.sort(); r
        };
        assert_eq!(nox_rows, ref_rows, "nox equijoin+filter vs reference");
    }

    // ── R1c: bounded recursion via nox ────────────────────────────────────────

    #[test]
    fn nox_bounded_reach_1hop_matches_reference() {
        let src = fixture_src();
        // edge: (1,2), (2,3), (3,4); reach from seed=1, extra_steps=1 (= :bounded 2)
        // Round 0 (base):   edge.from==1 → {2}
        // Round 1 (step 1): edge.from∈{2} → {3}
        // Result: {2, 3}
        let nox_reached = nox_vm_bounded_reach(&src, "edge", 0, 1, 1, 1).unwrap();

        // Reference with :bounded 2
        let ir = parse_and_plan(
            "reach[n] := edge{from: 1, to: n}\n\
             reach[n] := reach[m], edge{from: m, to: n}\n\
             :bounded 2\n\
             ?[n] := reach[n]",
        );
        let ref_out = eval(&ir, &src, &inf_eval::Ctx::default()).expect("ref eval");
        let mut ref_nodes: Vec<u64> = ref_out.rows.iter().map(|r| val_to_u64(&r[0])).collect();
        ref_nodes.sort();

        assert_eq!(nox_reached, ref_nodes, "nox 1-hop reach vs reference :bounded 2");
    }

    #[test]
    fn nox_bounded_reach_2hops_matches_reference() {
        let src = fixture_src();
        // extra_steps=2 (= :bounded 3)
        // Round 0: {2}; Round 1: {3}; Round 2: {4}
        let nox_reached = nox_vm_bounded_reach(&src, "edge", 0, 1, 1, 2).unwrap();

        let ir = parse_and_plan(
            "reach[n] := edge{from: 1, to: n}\n\
             reach[n] := reach[m], edge{from: m, to: n}\n\
             :bounded 3\n\
             ?[n] := reach[n]",
        );
        let ref_out = eval(&ir, &src, &inf_eval::Ctx::default()).expect("ref eval");
        let mut ref_nodes: Vec<u64> = ref_out.rows.iter().map(|r| val_to_u64(&r[0])).collect();
        ref_nodes.sort();

        assert_eq!(nox_reached, ref_nodes, "nox 2-hop reach vs reference :bounded 3");
    }

    // ── R2: zheng proof tests ─────────────────────────────────────────────────

    #[cfg(feature = "prove")]
    #[test]
    fn prove_and_verify_arithmetic_expr() {
        // Prove that (6 * 7) = 42 on the nox VM.
        let t = call("mul", int(6), int(7));
        let (result, proof) = prove_expr(&t).expect("prove");
        assert_eq!(result, 42, "nox computes 6*7=42");
        verify_expr_proof(&proof).expect("verify");
    }

    #[cfg(feature = "prove")]
    #[test]
    fn prove_and_verify_condition() {
        // Prove that lt(3, 5) holds (3 < 5).
        let c = inf_ast::Call { sibling: None, func: "lt".into(), args: vec![int(3), int(5)] };
        let (holds, proof) = prove_cond(&c, &[], &[]).expect("prove");
        assert!(holds, "3 < 5 should hold");
        verify_expr_proof(&proof).expect("verify");
    }

    #[cfg(feature = "prove")]
    #[test]
    fn prove_and_verify_tuple_condition() {
        // Prove that gt(weight, 10) holds for weight=15.
        let weight = Term::Var("weight".into());
        let c = inf_ast::Call {
            sibling: None,
            func: "gt".into(),
            args: vec![weight, int(10)],
        };
        let cols = vec!["weight".to_string()];
        let (holds, proof) = prove_cond(&c, &cols, &[15]).expect("prove");
        assert!(holds, "15 > 10 should hold");
        verify_expr_proof(&proof).expect("verify");
    }
}

// ── R3b: BbgSource ⇄ LocalSource differential parity ──────────────────────────
//
// the engine consumes `BbgSource` (real committed state) exactly as it consumes
// `LocalSource`. we build a committed `BbgState`, mirror its rows into a
// `LocalSource`, and assert the same query produces identical results on both —
// proving `BbgSource` is a faithful `RelationSource`.
#[cfg(all(test, feature = "bbg"))]
mod bbg_diff_tests {
    use bbg::state::BbgState;
    use bbg::types::NeuronRecord;
    use inf_eval::eval;
    use inf_source::{BbgSource, RelationSource};

    fn parse_and_plan(prog: &str) -> inf_ast::IrProgram {
        let ast = inf_parse::parse(prog).expect("parse");
        inf_plan::plan(&ast).expect("plan")
    }

    fn nid(n: u8) -> [u8; 32] {
        [n; 32]
    }

    /// A committed state with three neurons of distinct focus values.
    fn seeded_state() -> BbgState {
        let mut st = BbgState::new();
        st.neurons.insert(nid(1), NeuronRecord { focus: 100, karma: 1, stake: 0 });
        st.neurons.insert(nid(2), NeuronRecord { focus: 5_000, karma: 2, stake: 0 });
        st.neurons.insert(nid(3), NeuronRecord { focus: 9_000, karma: 3, stake: 0 });
        st
    }

    /// Mirror every BbgSource relation listed into an equivalent LocalSource.
    fn mirror(bbg_src: &BbgSource, rels: &[&str]) -> inf_source::LocalSource {
        let mut local = inf_source::LocalSource::new();
        for &rel in rels {
            let schema = bbg_src.schema(rel).expect("schema");
            let cols: Vec<&str> = schema.columns.iter().map(|c| c.as_str()).collect();
            let rows = bbg_src.scan(rel).collect::<Vec<_>>();
            local.add(rel, &cols, rows);
        }
        local
    }

    fn assert_parity(prog: &str, st: &BbgState, rels: &[&str]) {
        let bbg_src = BbgSource::new(st);
        let local = mirror(&bbg_src, rels);
        let ir = parse_and_plan(prog);
        let ctx = inf_eval::Ctx::default();
        let bbg_out = eval(&ir, &bbg_src, &ctx).expect("bbg eval");
        let local_out = eval(&ir, &local, &ctx).expect("local eval");
        assert_eq!(
            bbg_out.rows, local_out.rows,
            "BbgSource vs LocalSource mismatch\nbbg:   {:?}\nlocal: {:?}",
            bbg_out.rows, local_out.rows
        );
        assert!(!bbg_out.rows.is_empty(), "query should return rows");
    }

    #[test]
    fn scan_projection_parity() {
        let st = seeded_state();
        assert_parity("?[id, focus] := neurons{id: id, focus: focus}", &st, &["neurons"]);
    }

    #[test]
    fn filter_gt_parity() {
        let st = seeded_state();
        // neurons with focus > 1000 → nid(2), nid(3)
        assert_parity(
            "?[id, focus] := neurons{id: id, focus: focus}, gt(focus, 1000)",
            &st,
            &["neurons"],
        );
    }

    #[test]
    fn multi_column_filter_parity() {
        let st = seeded_state();
        // exercises a non-headline column (karma) — sound here because this is the
        // read path (scan door), not the proof path.
        assert_parity(
            "?[id] := neurons{id: id, karma: k}, gt(k, 1)",
            &st,
            &["neurons"],
        );
    }
}
