//! The reference evaluator: a semi-naive datalog engine over the IR and a
//! `RelationSource` (specs/ir.md, specs/language.md — language.md's bounded
//! recursion section specifies semi-naive explicitly). Each stratum is
//! evaluated bottom-up; a non-recursive stratum needs one pass, a recursive
//! stratum iterates to a bounded fixed point by joining only newly-derived
//! (delta) facts each round rather than re-deriving from the full
//! accumulated relation (`.claude/plans/query-optimization.md` P3). This is
//! the canonical-semantics reference the nox lowering must match.

mod agg;
mod expr;
mod fixed;
mod fixed_more;
mod funcs;

pub use expr::Ctx;

use inf_ast::*;
use inf_source::RelationSource;
use inf_value::{Relation, Tuple, Value};
use std::collections::{BTreeMap, HashMap, HashSet};

use expr::{eval_call, eval_term, Binding};

const HARD_CAP: usize = 100_000;

#[derive(Clone, Debug, PartialEq)]
pub struct EvalError {
    pub msg: String,
}

fn err<T>(msg: impl Into<String>) -> Result<T, EvalError> {
    Err(EvalError { msg: msg.into() })
}

#[derive(Clone, Debug, PartialEq)]
pub enum MutOp {
    Link,
    Unlink,
    Put(String),
    Rm(String),
}

/// The result of evaluating a program: a result set, or — when the program ends
/// in a mutation — the derived cyberlink batch a signal would carry.
#[derive(Clone, Debug, PartialEq)]
pub struct Output {
    pub columns: Vec<String>,
    pub rows: Vec<Tuple>,
    pub mutation: Option<MutOp>,
}

/// Per-relation derived state: column names + the tuple set.
type Env = BTreeMap<String, (Vec<String>, Relation)>;

fn head_cols(h: &Head) -> Vec<String> {
    h.args
        .iter()
        .map(|a| match a {
            HeadArg::Var(v) => v.clone(),
            HeadArg::Aggr { var, .. } => var.clone(),
        })
        .collect()
}

fn head_rel(r: &Rule) -> String {
    r.head.name.clone().unwrap_or_else(|| ENTRY.to_string())
}

/// Evaluate a closed term (no free variables) with the default context. The nox
/// lowering uses this as the reference value in its differential tests.
pub fn eval_const_term(t: &Term) -> Result<Value, EvalError> {
    expr::eval_term(t, &Default::default(), &Ctx::default()).map_err(|m| EvalError { msg: m })
}

/// Evaluate a term with integer columns bound by name — the reference value for
/// the nox lowering's relational differential tests.
pub fn eval_term_in(t: &Term, cols: &[String], ints: &[i64]) -> Result<Value, EvalError> {
    let mut b = expr::Binding::new();
    for (c, v) in cols.iter().zip(ints) {
        b.insert(c.clone(), Value::Int(*v));
    }
    expr::eval_term(t, &b, &Ctx::default()).map_err(|m| EvalError { msg: m })
}

/// A mutation event for the reactive register: a tuple appended to a relation.
#[derive(Clone, Debug)]
pub struct Event {
    pub rel: String,
    pub tuple: Tuple,
}

/// Reactive evaluation (specs/extensions.md). Apply each event to the source;
/// when an event matches the `:subscribe` relation (or there is no selector),
/// re-evaluate and emit a result. The event log is the witness sequence the
/// proof would be conditional on.
pub fn eval_reactive(
    ir: &IrProgram,
    mut source: inf_source::LocalSource,
    events: &[Event],
    ctx: &Ctx,
) -> Result<Vec<Output>, EvalError> {
    let mut outs = Vec::new();
    for ev in events {
        source.insert(&ev.rel, ev.tuple.clone()).map_err(|m| EvalError { msg: m })?;
        let trigger = match &ir.subscribe {
            None => true,
            Some((rel, _)) => &ev.rel == rel,
        };
        if trigger {
            outs.push(eval(ir, &source, ctx)?);
        }
    }
    Ok(outs)
}

pub fn eval(ir: &IrProgram, src: &dyn RelationSource, ctx: &Ctx) -> Result<Output, EvalError> {
    let mut env: Env = Env::new();

    for stratum in &ir.strata {
        eval_stratum(stratum, &mut env, src, ctx)?;
    }

    let (cols, rel) = env
        .get(&ir.entry)
        .cloned()
        .unwrap_or_else(|| (Vec::new(), Relation::new()));
    let mut rows: Vec<Tuple> = rel.into_iter().collect();

    apply_opts(&ir.opts, &cols, &mut rows)?;

    if let Some(m) = &ir.mutation {
        return build_mutation(m, &cols, &rows, ctx);
    }
    Ok(Output { columns: cols, rows, mutation: None })
}

fn eval_stratum(
    stratum: &Stratum,
    env: &mut Env,
    src: &dyn RelationSource,
    ctx: &Ctx,
) -> Result<(), EvalError> {
    // Ensure each head relation exists with its column names.
    for r in &stratum.rules {
        env.entry(head_rel(r)).or_insert_with(|| (head_cols(&r.head), Relation::new()));
    }

    // Round 0 (also the entire evaluation for a non-recursive stratum): every
    // in-stratum relation starts empty, so any rule referencing one
    // contributes nothing yet — this naturally computes just the base case
    // (e.g. `reachable[x] := axons{..}`) without special-casing it.
    eval_stratum_round(stratum, env, src, ctx)?;
    if !stratum.recursive {
        return Ok(());
    }

    // `in_stratum` — the relations this stratum itself defines. A rule body
    // atom referencing one of these is a "recursive" occurrence; a rule with
    // none is a base rule whose inputs (source relations, lower strata) never
    // change across this stratum's rounds, so round 0 already computed its
    // final contribution and it is never re-run (this also covers `Fixed`
    // rules: their `edges` dependency is always forced to a strictly lower
    // stratum by `derived_deps`, so a fixed rule can never itself be
    // recursive within its own stratum).
    let in_stratum: HashSet<String> = stratum.rules.iter().map(head_rel).collect();

    // delta[rel] = every fact `total` (env) holds after round 0 — everything
    // is "new" going into round 1, since total started empty.
    let mut delta: BTreeMap<String, Relation> = in_stratum
        .iter()
        .map(|rel| (rel.clone(), env.get(rel).map(|(_, r)| r.clone()).unwrap_or_default()))
        .collect();

    let mut rounds = 1usize;
    loop {
        if delta.values().all(|d| d.is_empty()) {
            break;
        }
        if let Some(b) = stratum.bound {
            if rounds as u64 >= b {
                break;
            }
        }
        if rounds >= HARD_CAP {
            return err("evaluation exceeded the hard round cap (non-terminating?)");
        }

        // For each in-stratum atom occurrence in a recursive rule, run one
        // semi-naive variant substituting that occurrence's relation with
        // just this round's delta, leaving every other atom (including other
        // occurrences of the same or a different in-stratum relation) on the
        // normal total-as-of-start-of-round path. Union over every
        // occurrence in every recursive rule — the standard semi-naive
        // scheme (see the module doc and query-optimization.md P3): any
        // derivation that needs a newly-added fact is found through the
        // variant that substitutes the occurrence supplying it.
        let mut binds_by_head: BTreeMap<String, (Head, Vec<Binding>)> = BTreeMap::new();
        for r in &stratum.rules {
            if r.fixed.is_some() {
                continue; // base rule, see above
            }
            let recursive_positions: Vec<usize> = r
                .body
                .iter()
                .enumerate()
                .filter_map(|(i, a)| match a {
                    Atom::Read { rel, .. } | Atom::Apply { rule: rel, .. }
                        if in_stratum.contains(rel) =>
                    {
                        Some(i)
                    }
                    _ => None,
                })
                .collect();
            if recursive_positions.is_empty() {
                continue; // base rule, see above
            }
            for pos in recursive_positions {
                let rel = r.body[pos].rel_name().unwrap().to_string();
                let d = match delta.get(&rel) {
                    Some(d) if !d.is_empty() => d,
                    _ => continue,
                };
                let delta_cols = env.get(&rel).map(|(c, _)| c.clone()).unwrap_or_default();
                let delta_rows: Vec<Tuple> = d.iter().cloned().collect();
                let bs = eval_body_delta_variant(r, env, src, ctx, pos, &delta_cols, &delta_rows)?;
                let e = binds_by_head
                    .entry(head_rel(r))
                    .or_insert_with(|| (r.head.clone(), Vec::new()));
                e.1.extend(bs);
            }
        }

        // Materialize this round's variants: only truly new facts (not
        // already in `total`) become next round's delta. `env` is mutated
        // only here, after every variant for this round has read the
        // pre-round snapshot — preserving the semi-naive invariant that a
        // round's "total" side never sees that same round's own deltas.
        let mut delta_next: BTreeMap<String, Relation> = BTreeMap::new();
        for (rel, (head, binds)) in binds_by_head {
            let new_facts: Vec<Tuple> = if head.has_aggr() {
                agg::aggregate(&head, &binds).map_err(|m| EvalError { msg: m })?
            } else {
                binds.iter().map(|b| project(&head, b)).collect::<Result<_, _>>()?
            };
            let slot = env.get_mut(&rel).unwrap();
            let mut nd = Relation::new();
            for t in new_facts {
                if slot.1.insert(t.clone()) {
                    nd.insert(t);
                }
            }
            delta_next.insert(rel, nd);
        }
        delta = delta_next;
        rounds += 1;
    }
    Ok(())
}

/// Evaluate every rule in `stratum` once against the environment's current
/// state, materializing newly-derived facts into `env`. The complete
/// evaluation for a non-recursive stratum; round 0 for a recursive one.
fn eval_stratum_round(
    stratum: &Stratum,
    env: &mut Env,
    src: &dyn RelationSource,
    ctx: &Ctx,
) -> Result<(), EvalError> {
    let mut binds_by_head: BTreeMap<String, (Head, Option<String>, Vec<Binding>)> = BTreeMap::new();
    for r in &stratum.rules {
        if let Some(f) = &r.fixed {
            let edges = rows_of(&f.edges, env, src)?.1;
            let tuples = fixed::run(f, &r.head, &edges, ctx).map_err(|m| EvalError { msg: m })?;
            let new: Relation = tuples.into_iter().collect();
            env.get_mut(&head_rel(r)).unwrap().1 = new;
        } else {
            let bs = eval_body(r, env, src, ctx)?;
            let e = binds_by_head
                .entry(head_rel(r))
                .or_insert_with(|| (r.head.clone(), r.order.clone(), Vec::new()));
            e.2.extend(bs);
        }
    }
    for (rel, (head, order, binds)) in binds_by_head {
        if let Some(order_var) = order {
            // bounded ordered aggregate (P2): a `running_*` head aggregate,
            // checked safe (paired with `:order`) by `plan()`. One output
            // row per input row, not one per group — see `agg::windowed_aggregate`.
            let tuples =
                agg::windowed_aggregate(&head, &binds, &order_var).map_err(|m| EvalError { msg: m })?;
            env.get_mut(&rel).unwrap().1 = tuples.into_iter().collect();
        } else if head.has_aggr() {
            let tuples = agg::aggregate(&head, &binds).map_err(|m| EvalError { msg: m })?;
            env.get_mut(&rel).unwrap().1 = tuples.into_iter().collect();
        } else {
            let slot = env.get_mut(&rel).unwrap();
            for b in &binds {
                let t = project(&head, b)?;
                slot.1.insert(t);
            }
        }
    }
    Ok(())
}

fn project(head: &Head, b: &Binding) -> Result<Tuple, EvalError> {
    let mut t = Vec::new();
    for a in &head.args {
        match a {
            HeadArg::Var(v) => {
                t.push(b.get(v).cloned().ok_or_else(|| EvalError {
                    msg: format!("head variable `{v}` unbound after body evaluation"),
                })?);
            }
            HeadArg::Aggr { .. } => {
                return err("aggregation head reached the non-aggregate projection path");
            }
        }
    }
    Ok(t)
}

fn eval_body(
    r: &Rule,
    env: &Env,
    src: &dyn RelationSource,
    ctx: &Ctx,
) -> Result<Vec<Binding>, EvalError> {
    let mut bindings = vec![Binding::new()];
    for atom in &r.body {
        bindings = step_atom(atom, bindings, env, src, ctx)?;
        if bindings.is_empty() {
            break;
        }
    }
    Ok(bindings)
}

/// Resolve a relation's columns and rows from the env (derived) or the source.
fn rows_of(
    rel: &str,
    env: &Env,
    src: &dyn RelationSource,
) -> Result<(Vec<String>, Vec<Tuple>), EvalError> {
    if let Some((cols, r)) = env.get(rel) {
        return Ok((cols.clone(), r.iter().cloned().collect()));
    }
    if let Some(schema) = src.schema(rel) {
        return Ok((schema.columns, src.scan(rel).collect()));
    }
    err(format!("unknown relation `{rel}`"))
}

fn step_atom(
    atom: &Atom,
    bindings: Vec<Binding>,
    env: &Env,
    src: &dyn RelationSource,
    ctx: &Ctx,
) -> Result<Vec<Binding>, EvalError> {
    match atom {
        Atom::Read { rel, binds } => read_join(rel, binds, bindings, env, src, ctx),
        Atom::Apply { rule, args } => {
            let binds = Binds::Pos(args.clone());
            read_join(rule, &binds, bindings, env, src, ctx)
        }
        Atom::Cond(call) => {
            let mut out = Vec::new();
            for b in bindings {
                let passes = if let Some(nox) = &ctx.nox_cond {
                    match nox(call, &b) {
                        Some(r) => r,
                        None => eval_call(call, &b, ctx)
                            .map_err(|m| EvalError { msg: m })?
                            .truthy(),
                    }
                } else {
                    eval_call(call, &b, ctx).map_err(|m| EvalError { msg: m })?.truthy()
                };
                if passes {
                    out.push(b);
                }
            }
            Ok(out)
        }
        Atom::Bind { var, term } => {
            let mut out = Vec::new();
            for mut b in bindings {
                let v = eval_term(term, &b, ctx).map_err(|m| EvalError { msg: m })?;
                match b.get(var) {
                    Some(existing) if *existing != v => {}
                    _ => {
                        b.insert(var.clone(), v);
                        out.push(b);
                    }
                }
            }
            Ok(out)
        }
        Atom::Not(inner) => {
            let (rel, binds) = match inner.as_ref() {
                Atom::Read { rel, binds } => (rel.clone(), binds.clone()),
                Atom::Apply { rule, args } => (rule.clone(), Binds::Pos(args.clone())),
                _ => return err("negation must wrap a relation read"),
            };
            let (cols, rows) = rows_of(&rel, env, src)?;
            let mut out = Vec::new();
            let idx = bindings.first().and_then(|s| BoundIndex::build(&binds, &cols, &rows, s));
            for b in bindings {
                let matched = match &idx {
                    Some(i) => any_match(i.candidates(&b, ctx).iter().copied(), &binds, &cols, &b, ctx)?,
                    None => any_match(rows.iter(), &binds, &cols, &b, ctx)?,
                };
                if !matched {
                    out.push(b);
                }
            }
            Ok(out)
        }
    }
}

/// A lookup index over a relation's rows, keyed by the columns of a read
/// that are already fixed before scanning — narrows which rows attempt
/// `unify` instead of scanning all of them (`.claude/plans/query-optimization.md`
/// P0). `unify` remains the sole authority on whether a row actually
/// matches; the index only prunes candidates, so it can never change which
/// unifications succeed, only how many are attempted.
struct BoundIndex<'a> {
    key_cols: Vec<(usize, &'a Term)>,
    by_key: HashMap<Vec<Value>, Vec<&'a Tuple>>,
}

impl<'a> BoundIndex<'a> {
    /// `sample` is one binding from the incoming list. Every binding
    /// reaching a given atom passed through the same preceding atoms, so
    /// they all share the same set of bound variable names — one binding is
    /// enough to decide which of `binds`'s columns are indexable for this
    /// whole read. Returns `None` when no column is indexable (every bound
    /// term is a variable still fresh at this atom) — nothing to narrow.
    fn build(binds: &'a Binds, cols: &[String], rows: &'a [Tuple], sample: &Binding) -> Option<Self> {
        let key_cols = bound_columns(binds, cols, sample);
        if key_cols.is_empty() {
            return None;
        }
        let mut by_key: HashMap<Vec<Value>, Vec<&Tuple>> = HashMap::new();
        for row in rows {
            let key: Vec<Value> = key_cols.iter().map(|(i, _)| row[*i].clone()).collect();
            by_key.entry(key).or_default().push(row);
        }
        Some(BoundIndex { key_cols, by_key })
    }

    /// Candidate rows for `b` — empty when `b`'s key isn't present, or when
    /// a key term can't be evaluated from `b` alone (defensive; the shape
    /// guarantee in `build` means this shouldn't happen in practice).
    fn candidates(&self, b: &Binding, ctx: &Ctx) -> &[&'a Tuple] {
        let mut key = Vec::with_capacity(self.key_cols.len());
        for (_, term) in &self.key_cols {
            match index_key_value(term, b, ctx) {
                Some(v) => key.push(v),
                None => return &[],
            }
        }
        self.by_key.get(&key).map(|v| v.as_slice()).unwrap_or(&[])
    }
}

/// Columns of `binds` whose value is fixed before scanning a row: a
/// variable already present in `sample`, or any non-variable term (an
/// expression evaluable from the binding alone — the same class `unify_term`
/// already evaluates in its non-`Var` branch).
fn bound_columns<'a>(binds: &'a Binds, cols: &[String], sample: &Binding) -> Vec<(usize, &'a Term)> {
    let is_bound = |t: &Term| match t {
        Term::Var(v) => sample.contains_key(v),
        _ => true,
    };
    match binds {
        Binds::Named(pairs) => pairs
            .iter()
            .filter_map(|(col, term)| {
                cols.iter().position(|c| c == col).filter(|_| is_bound(term)).map(|i| (i, term))
            })
            .collect(),
        Binds::Pos(terms) => terms
            .iter()
            .enumerate()
            .filter(|(i, term)| *i < cols.len() && is_bound(term))
            .map(|(i, term)| (i, term))
            .collect(),
    }
}

fn index_key_value(term: &Term, b: &Binding, ctx: &Ctx) -> Option<Value> {
    match term {
        Term::Var(v) => b.get(v).cloned(),
        other => eval_term(other, b, ctx).ok(),
    }
}

/// Try `unify` against each candidate row, pushing every match into `out`.
fn join_matches<'r>(
    rows: impl Iterator<Item = &'r Tuple>,
    binds: &Binds,
    cols: &[String],
    b: &Binding,
    ctx: &Ctx,
    out: &mut Vec<Binding>,
) -> Result<(), EvalError> {
    for row in rows {
        if let Some(nb) = unify(binds, row, cols, b, ctx)? {
            out.push(nb);
        }
    }
    Ok(())
}

/// Whether any candidate row unifies — short-circuits on the first match.
fn any_match<'r>(
    rows: impl Iterator<Item = &'r Tuple>,
    binds: &Binds,
    cols: &[String],
    b: &Binding,
    ctx: &Ctx,
) -> Result<bool, EvalError> {
    for row in rows {
        if unify(binds, row, cols, b, ctx)?.is_some() {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The core of a relation join, over an explicit `(cols, rows)` pair rather
/// than resolving through `env`/`src` — lets a caller substitute a different
/// row set for the same relation (semi-naive's delta substitution, see
/// `eval_body_delta_variant`) without duplicating the indexing logic.
fn join_over(
    cols: &[String],
    rows: &[Tuple],
    binds: &Binds,
    bindings: &[Binding],
    ctx: &Ctx,
) -> Result<Vec<Binding>, EvalError> {
    let mut out = Vec::new();
    if bindings.is_empty() || rows.is_empty() {
        return Ok(out);
    }
    let idx = BoundIndex::build(binds, cols, rows, &bindings[0]);
    for b in bindings {
        match &idx {
            Some(i) => join_matches(i.candidates(b, ctx).iter().copied(), binds, cols, b, ctx, &mut out)?,
            None => join_matches(rows.iter(), binds, cols, b, ctx, &mut out)?,
        }
    }
    Ok(out)
}

fn read_join(
    rel: &str,
    binds: &Binds,
    bindings: Vec<Binding>,
    env: &Env,
    src: &dyn RelationSource,
    ctx: &Ctx,
) -> Result<Vec<Binding>, EvalError> {
    let (cols, rows) = rows_of(rel, env, src)?;
    join_over(&cols, &rows, binds, &bindings, ctx)
}

/// One semi-naive substitution variant of a recursive rule's body: the atom
/// at `delta_pos` reads `delta_rows` instead of the accumulated total; every
/// other atom reads normally via `env`/`src` (see `eval_stratum` and
/// `.claude/plans/query-optimization.md` P3).
fn eval_body_delta_variant(
    r: &Rule,
    env: &Env,
    src: &dyn RelationSource,
    ctx: &Ctx,
    delta_pos: usize,
    delta_cols: &[String],
    delta_rows: &[Tuple],
) -> Result<Vec<Binding>, EvalError> {
    let mut bindings = vec![Binding::new()];
    for (i, atom) in r.body.iter().enumerate() {
        bindings = if i == delta_pos {
            let binds = match atom {
                Atom::Read { binds, .. } => binds.clone(),
                Atom::Apply { args, .. } => Binds::Pos(args.clone()),
                _ => return err("semi-naive delta substitution target must be a relation read"),
            };
            join_over(delta_cols, delta_rows, &binds, &bindings, ctx)?
        } else {
            step_atom(atom, bindings, env, src, ctx)?
        };
        if bindings.is_empty() {
            break;
        }
    }
    Ok(bindings)
}

/// Try to unify a relation read's bindings against a row under `base`.
fn unify(
    binds: &Binds,
    row: &Tuple,
    cols: &[String],
    base: &Binding,
    ctx: &Ctx,
) -> Result<Option<Binding>, EvalError> {
    let mut b = base.clone();
    match binds {
        Binds::Named(pairs) => {
            for (col, term) in pairs {
                let idx = cols
                    .iter()
                    .position(|c| c == col)
                    .ok_or_else(|| EvalError { msg: format!("unknown column `{col}`") })?;
                if !unify_term(term, &row[idx], &mut b, ctx)? {
                    return Ok(None);
                }
            }
        }
        Binds::Pos(terms) => {
            if terms.len() > row.len() {
                return err("positional read has more columns than the relation");
            }
            for (i, term) in terms.iter().enumerate() {
                if !unify_term(term, &row[i], &mut b, ctx)? {
                    return Ok(None);
                }
            }
        }
    }
    Ok(Some(b))
}

fn unify_term(term: &Term, val: &Value, b: &mut Binding, ctx: &Ctx) -> Result<bool, EvalError> {
    match term {
        Term::Var(v) => match b.get(v) {
            Some(existing) => Ok(existing == val),
            None => {
                b.insert(v.clone(), val.clone());
                Ok(true)
            }
        },
        other => {
            let tv = eval_term(other, b, ctx).map_err(|m| EvalError { msg: m })?;
            Ok(&tv == val)
        }
    }
}

fn apply_opts(opts: &[Opt], cols: &[String], rows: &mut Vec<Tuple>) -> Result<(), EvalError> {
    for o in opts {
        match o {
            Opt::Sort { col, desc } => {
                let idx = cols
                    .iter()
                    .position(|c| c == col)
                    .ok_or_else(|| EvalError { msg: format!("sort: unknown column `{col}`") })?;
                rows.sort_by(|a, b| a[idx].cmp(&b[idx]));
                if *desc {
                    rows.reverse();
                }
            }
            Opt::Offset(n) => {
                let n = (*n as usize).min(rows.len());
                rows.drain(0..n);
            }
            Opt::Limit(n) => {
                rows.truncate(*n as usize);
            }
            Opt::AssertNone => {
                if !rows.is_empty() {
                    return err(format!(":assert none failed: {} rows", rows.len()));
                }
            }
            Opt::AssertSome => {
                if rows.is_empty() {
                    return err(":assert some failed: no rows");
                }
            }
        }
    }
    Ok(())
}

fn build_mutation(
    m: &Mutation,
    entry_cols: &[String],
    entry_rows: &[Tuple],
    ctx: &Ctx,
) -> Result<Output, EvalError> {
    let (op, binds) = match m {
        Mutation::Link(b) => (MutOp::Link, b),
        Mutation::Unlink(b) => (MutOp::Unlink, b),
        Mutation::Put { rel, binds } => (MutOp::Put(rel.clone()), binds),
        Mutation::Rm { rel, binds } => (MutOp::Rm(rel.clone()), binds),
    };
    let pairs = match binds {
        Binds::Named(p) => p.clone(),
        Binds::Pos(_) => return err("mutation requires named bindings"),
    };
    let columns: Vec<String> = pairs.iter().map(|(c, _)| c.clone()).collect();

    let mut batch: inf_value::Relation = inf_value::Relation::new();
    for row in entry_rows {
        let mut b = Binding::new();
        for (i, c) in entry_cols.iter().enumerate() {
            b.insert(c.clone(), row[i].clone());
        }
        let mut tuple = Vec::new();
        for (_, term) in &pairs {
            tuple.push(eval_term(term, &b, ctx).map_err(|m| EvalError { msg: m })?);
        }
        batch.insert(tuple);
    }
    Ok(Output { columns, rows: batch.into_iter().collect(), mutation: Some(op) })
}
