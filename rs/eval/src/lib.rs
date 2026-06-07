//! The reference evaluator: a naive-fixed-point datalog engine over the IR and a
//! `RelationSource` (specs/ir.md, specs/language.md). Each stratum is evaluated
//! bottom-up; recursive strata iterate to a bounded fixed point. This is the
//! canonical-semantics reference the nox lowering must match.

mod agg;
mod expr;
mod fixed;
mod fixed_more;
mod funcs;

pub use expr::Ctx;

use inf_ast::*;
use inf_source::RelationSource;
use inf_value::{Relation, Tuple, Value};
use std::collections::BTreeMap;

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

    let mut rounds = 0usize;
    loop {
        let mut changed = false;
        let mut binds_by_head: BTreeMap<String, (Head, Vec<Binding>)> = BTreeMap::new();
        for r in &stratum.rules {
            if let Some(f) = &r.fixed {
                let edges = rows_of(&f.edges, env, src)?.1;
                let tuples =
                    fixed::run(f, &r.head, &edges, ctx).map_err(|m| EvalError { msg: m })?;
                let new: Relation = tuples.into_iter().collect();
                let slot = env.get_mut(&head_rel(r)).unwrap();
                if slot.1 != new {
                    slot.1 = new;
                    changed = true;
                }
            } else {
                let bs = eval_body(r, env, src, ctx)?;
                let e = binds_by_head
                    .entry(head_rel(r))
                    .or_insert_with(|| (r.head.clone(), Vec::new()));
                e.1.extend(bs);
            }
        }

        for (rel, (head, binds)) in binds_by_head {
            let cols = head_cols(&head);
            if head.has_aggr() {
                let tuples = agg::aggregate(&head, &binds).map_err(|m| EvalError { msg: m })?;
                let new: Relation = tuples.into_iter().collect();
                let slot = env.get_mut(&rel).unwrap();
                if slot.1 != new {
                    slot.1 = new;
                    changed = true;
                }
            } else {
                let slot = env.get_mut(&rel).unwrap();
                let _ = cols;
                for b in &binds {
                    let t = project(&head, b)?;
                    if slot.1.insert(t) {
                        changed = true;
                    }
                }
            }
        }

        rounds += 1;
        if !changed {
            break;
        }
        if stratum.recursive {
            if let Some(b) = stratum.bound {
                if rounds as u64 >= b {
                    break;
                }
            }
        }
        if rounds >= HARD_CAP {
            return err("evaluation exceeded the hard round cap (non-terminating?)");
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
                let v = eval_call(call, &b, ctx).map_err(|m| EvalError { msg: m })?;
                if v.truthy() {
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
            for b in bindings {
                let mut matched = false;
                for row in &rows {
                    if unify(&binds, row, &cols, &b, ctx)?.is_some() {
                        matched = true;
                        break;
                    }
                }
                if !matched {
                    out.push(b);
                }
            }
            Ok(out)
        }
    }
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
    let mut out = Vec::new();
    for b in &bindings {
        for row in &rows {
            if let Some(nb) = unify(binds, row, &cols, b, ctx)? {
                out.push(nb);
            }
        }
    }
    Ok(out)
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
