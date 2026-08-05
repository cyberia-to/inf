//! The planner: AST → IR (specs/ir.md). It stratifies the rules so negation and
//! aggregation compute bottom-up, marks recursive strata (which iterate to a
//! bounded fixed point), and runs the well-formedness checks from
//! specs/language.md (range restriction, negation safety).

use inf_ast::*;
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq)]
pub struct PlanError {
    pub msg: String,
}

fn err<T>(msg: impl Into<String>) -> Result<T, PlanError> {
    Err(PlanError { msg: msg.into() })
}

/// The relation name a rule defines (`?` for the entry rule).
fn head_rel(r: &Rule) -> String {
    r.head.name.clone().unwrap_or_else(|| ENTRY.to_string())
}

pub fn plan(prog: &Program) -> Result<IrProgram, PlanError> {
    if prog.rules.is_empty() {
        return err("empty program: no rules");
    }
    if !prog.rules.iter().any(|r| r.head.name.is_none()) {
        return err("no entry rule: a program must have exactly one `?` rule");
    }
    if prog.rules.iter().filter(|r| r.head.name.is_none()).count() > 1 {
        return err("more than one entry rule `?`");
    }

    let derived: HashSet<String> = prog.rules.iter().map(head_rel).collect();

    for r in &prog.rules {
        check_safety(r)?;
    }

    // Stratum numbers by fixed-point relaxation: positive deps keep the stratum,
    // negation/aggregation deps push one higher. A strict cycle never converges.
    let names: Vec<String> = derived.iter().cloned().collect();
    let n = names.len();
    let mut stratum: std::collections::HashMap<String, usize> =
        names.iter().map(|s| (s.clone(), 0usize)).collect();

    for iter in 0..=n {
        let mut changed = false;
        for r in &prog.rules {
            let h = head_rel(r);
            for (g, strict) in derived_deps(r, &derived) {
                let want = stratum[&g] + usize::from(strict);
                if want > stratum[&h] {
                    *stratum.get_mut(&h).unwrap() = want;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
        if iter == n {
            return err("query is not stratifiable: negation or aggregation inside a recursive cycle");
        }
    }

    let max_s = *stratum.values().max().unwrap_or(&0);
    let mut strata = Vec::new();
    for s in 0..=max_s {
        let rules: Vec<Rule> = prog
            .rules
            .iter()
            .filter(|r| stratum[&head_rel(r)] == s)
            .cloned()
            .map(|mut r| {
                r.body = reorder_body(r.body);
                r
            })
            .collect();
        if rules.is_empty() {
            continue;
        }
        let in_stratum: HashSet<String> = rules.iter().map(head_rel).collect();
        let recursive = rules
            .iter()
            .any(|r| derived_deps(r, &derived).iter().any(|(g, _)| in_stratum.contains(g)));
        let bound = rules.iter().filter_map(|r| r.bound).max();
        strata.push(Stratum { rules, recursive, bound });
    }

    Ok(IrProgram {
        strata,
        entry: ENTRY.to_string(),
        mutation: prog.mutation.clone(),
        opts: prog.opts.clone(),
        subscribe: prog.subscribe.clone(),
    })
}

/// Derived relations a rule depends on, with strictness (must be a lower
/// stratum). Negation and aggregation are strict. The entry rule `?` is forced
/// strictly above its dependencies so it observes the final, fully-computed
/// relations — a non-recursive projection never shares a stratum with the
/// recursion it reads.
fn derived_deps(r: &Rule, derived: &HashSet<String>) -> Vec<(String, bool)> {
    let base_strict = r.head.has_aggr() || r.head.name.is_none();
    let mut out = Vec::new();
    if let Some(f) = &r.fixed {
        // a fixed rule consumes the whole edges relation, so it must be strictly
        // above it
        if derived.contains(&f.edges) {
            out.push((f.edges.clone(), true));
        }
        return out;
    }
    for a in &r.body {
        match a {
            Atom::Read { rel, .. } | Atom::Apply { rule: rel, .. } => {
                if derived.contains(rel) {
                    out.push((rel.clone(), base_strict));
                }
            }
            Atom::Not(inner) => {
                if let Some(rel) = inner.rel_name() {
                    if derived.contains(rel) {
                        out.push((rel.to_string(), true));
                    }
                }
            }
            _ => {}
        }
    }
    out.sort();
    out.dedup();
    out
}

// ── join ordering (.claude/plans/query-optimization.md P1) ────────────────────
//
// The evaluator threads bindings through `Rule.body` strictly in written
// order (see inf-eval `eval_body`) — atom order IS join order. Reorder each
// body once, here, so query authors never have to hand-order joins:
//
//   1. filter pushdown — move each `Cond`/`Not`/`Bind` atom to the earliest
//      point where its free variables are already bound. always safe: a
//      filter's truth value never depends on when it runs, only on its
//      variables being bound.
//   2. most-bound-first among interchangeable `Read`/`Apply` runs — a join
//      between two positive atoms is commutative, so among atoms that are
//      simultaneously ready, prefer the one with more already-bound columns
//      (a static selectivity proxy; `plan()` has no `RelationSource` to ask
//      for real cardinalities).
//
// Implemented as a single greedy schedule: repeatedly pick the
// highest-priority atom whose free variables are already satisfied. This is
// a topological sort with a priority tiebreak, not a permutation search — it
// never reorders past a real dependency, so it can only ever match or beat
// the original body's join cost. Ties keep the original relative order
// (stable), which matters for determinism (specs/proof.md).

/// Whether `s` reads only variables already present in `bound` — used both to
/// decide if an atom is *ready* to run next and, for `Read`/`Apply`, to score
/// how many of its columns are already fixed.
fn is_subset(s: &HashSet<String>, bound: &HashSet<String>) -> bool {
    s.iter().all(|v| bound.contains(v))
}

/// Variables an atom's free (non-binding) positions require to already be
/// bound before it can run. For `Read`/`Apply`, a plain `Var` is a
/// fresh-bind-or-check — it never requires prior binding — but a variable
/// nested inside a non-`Var` term does (the term is evaluated against the
/// current binding, same as `unify_term`'s non-`Var` branch). `Not` requires
/// every variable in its wrapped atom's binds, matching `check_safety`'s
/// negation-safety rule.
fn required_vars(atom: &Atom) -> HashSet<String> {
    let mut acc = HashSet::new();
    let free_in_binds = |binds: &Binds, acc: &mut HashSet<String>| {
        let terms: Vec<&Term> = match binds {
            Binds::Named(pairs) => pairs.iter().map(|(_, t)| t).collect(),
            Binds::Pos(v) => v.iter().collect(),
        };
        for t in terms {
            if !matches!(t, Term::Var(_)) {
                term_vars(t, acc);
            }
        }
    };
    match atom {
        Atom::Read { binds, .. } => free_in_binds(binds, &mut acc),
        Atom::Apply { args, .. } => {
            for t in args {
                if !matches!(t, Term::Var(_)) {
                    term_vars(t, &mut acc);
                }
            }
        }
        Atom::Cond(call) => call.args.iter().for_each(|a| term_vars(a, &mut acc)),
        Atom::Bind { term, .. } => term_vars(term, &mut acc),
        Atom::Not(inner) => match inner.as_ref() {
            Atom::Read { binds, .. } => binds_vars(binds, &mut acc),
            Atom::Apply { args, .. } => args.iter().for_each(|t| term_vars(t, &mut acc)),
            _ => {}
        },
    }
    acc
}

/// Variables an atom adds to the bound set once it has run.
fn introduced_vars(atom: &Atom) -> HashSet<String> {
    let mut acc = HashSet::new();
    let vars_in_binds = |binds: &Binds, acc: &mut HashSet<String>| {
        let terms: Vec<&Term> = match binds {
            Binds::Named(pairs) => pairs.iter().map(|(_, t)| t).collect(),
            Binds::Pos(v) => v.iter().collect(),
        };
        for t in terms {
            if let Term::Var(v) = t {
                acc.insert(v.clone());
            }
        }
    };
    match atom {
        Atom::Read { binds, .. } => vars_in_binds(binds, &mut acc),
        Atom::Apply { args, .. } => {
            for t in args {
                if let Term::Var(v) = t {
                    acc.insert(v.clone());
                }
            }
        }
        Atom::Bind { var, .. } => {
            acc.insert(var.clone());
        }
        Atom::Cond(_) | Atom::Not(_) => {}
    }
    acc
}

/// Count of an atom's bind columns whose term is already evaluable given
/// `bound` — a plain already-bound `Var`, or any non-`Var` term (its free
/// variables are already confirmed bound, since only *ready* atoms are
/// scored). Used only to rank `Read`/`Apply` atoms against each other.
fn bound_column_count(terms: &[&Term], bound: &HashSet<String>) -> usize {
    terms
        .iter()
        .filter(|t| match t {
            Term::Var(v) => bound.contains(v),
            _ => true,
        })
        .count()
}

/// Sort key for the greedy schedule: filters/binds (class 0) before relation
/// reads (class 1); among reads, more already-bound columns first (negated
/// so a normal ascending sort picks the most-bound atom); original position
/// breaks ties, keeping the schedule stable and deterministic.
fn priority_key(atom: &Atom, bound: &HashSet<String>, orig_idx: usize) -> (u8, i64, usize) {
    match atom {
        Atom::Cond(_) | Atom::Not(_) | Atom::Bind { .. } => (0, 0, orig_idx),
        Atom::Read { binds, .. } => {
            let terms: Vec<&Term> = match binds {
                Binds::Named(pairs) => pairs.iter().map(|(_, t)| t).collect(),
                Binds::Pos(v) => v.iter().collect(),
            };
            (1, -(bound_column_count(&terms, bound) as i64), orig_idx)
        }
        Atom::Apply { args, .. } => {
            let terms: Vec<&Term> = args.iter().collect();
            (1, -(bound_column_count(&terms, bound) as i64), orig_idx)
        }
    }
}

/// Reorder one rule body via the greedy schedule above. Falls back to
/// appending any atom that never becomes ready in its original relative
/// order — this should not happen for a body that was valid (executable
/// left-to-right) in its original order, but degrading to a no-op reorder
/// rather than dropping atoms keeps this pass safe even if that assumption
/// is ever violated by a future grammar extension.
fn reorder_body(body: Vec<Atom>) -> Vec<Atom> {
    let n = body.len();
    let mut remaining: Vec<(usize, Atom)> = body.into_iter().enumerate().collect();
    let mut bound: HashSet<String> = HashSet::new();
    let mut out: Vec<Atom> = Vec::with_capacity(n);

    while !remaining.is_empty() {
        let mut best: Option<(usize, (u8, i64, usize))> = None;
        for (pos, (orig_idx, atom)) in remaining.iter().enumerate() {
            if !is_subset(&required_vars(atom), &bound) {
                continue;
            }
            let key = priority_key(atom, &bound, *orig_idx);
            if best.is_none_or(|(_, bk)| key < bk) {
                best = Some((pos, key));
            }
        }
        match best {
            Some((pos, _)) => {
                let (_, atom) = remaining.remove(pos);
                bound.extend(introduced_vars(&atom));
                out.push(atom);
            }
            None => {
                out.extend(remaining.into_iter().map(|(_, a)| a));
                break;
            }
        }
    }
    out
}

// ── safety ──────────────────────────────────────────────────────────────────

fn term_vars(t: &Term, acc: &mut HashSet<String>) {
    match t {
        Term::Var(v) => {
            acc.insert(v.clone());
        }
        Term::Call(c) => c.args.iter().for_each(|a| term_vars(a, acc)),
        Term::List(items) => items.iter().for_each(|a| term_vars(a, acc)),
        _ => {}
    }
}

fn binds_vars(b: &Binds, acc: &mut HashSet<String>) {
    match b {
        Binds::Named(v) => v.iter().for_each(|(_, t)| term_vars(t, acc)),
        Binds::Pos(v) => v.iter().for_each(|t| term_vars(t, acc)),
    }
}

/// Vars a positive (non-negated) atom binds.
fn positive_vars(r: &Rule) -> HashSet<String> {
    let mut s = HashSet::new();
    for a in &r.body {
        match a {
            Atom::Read { binds, .. } => binds_vars(binds, &mut s),
            Atom::Apply { args, .. } => args.iter().for_each(|t| term_vars(t, &mut s)),
            Atom::Bind { var, .. } => {
                s.insert(var.clone());
            }
            _ => {}
        }
    }
    s
}

fn check_safety(r: &Rule) -> Result<(), PlanError> {
    if r.fixed.is_some() {
        return Ok(()); // a fixed rule binds its head from the algorithm output
    }
    let bound = positive_vars(r);
    // range restriction: every head variable must be positively bound
    for ha in &r.head.args {
        let v = match ha {
            HeadArg::Var(v) => v,
            HeadArg::Aggr { var, .. } => var,
        };
        if !bound.contains(v) {
            return err(format!(
                "unsafe rule: head variable `{v}` is not bound by a positive body atom"
            ));
        }
    }
    // negation safety: every variable in a negated atom must be positively bound
    for a in &r.body {
        if let Atom::Not(inner) = a {
            let mut nv = HashSet::new();
            match inner.as_ref() {
                Atom::Read { binds, .. } => binds_vars(binds, &mut nv),
                Atom::Apply { args, .. } => args.iter().for_each(|t| term_vars(t, &mut nv)),
                _ => {}
            }
            for v in &nv {
                if !bound.contains(v) {
                    return err(format!(
                        "unsafe negation: variable `{v}` in a negated atom is not positively bound"
                    ));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use inf_parse::parse;

    fn ir(src: &str) -> IrProgram {
        plan(&parse(src).unwrap()).unwrap()
    }

    #[test]
    fn non_recursive_single_stratum() {
        let p = ir("?[to, s] := axons{from: #seed, to}, focus{particle: to, score: s}");
        assert_eq!(p.strata.len(), 1);
        assert!(!p.strata[0].recursive);
    }

    #[test]
    fn recursion_is_detected_and_bounded() {
        let p = ir(
            "reachable[p] := axons{from: #seed, to: p}\nreachable[p] := reachable[mid], axons{from: mid, to: p}\n:bounded 5\n?[p] := reachable[p]",
        );
        let rec = p.strata.iter().find(|s| s.recursive).expect("a recursive stratum");
        assert_eq!(rec.bound, Some(5));
        // the recursive stratum defines `reachable`
        assert!(rec.rules.iter().any(|r| r.head.name.as_deref() == Some("reachable")));
    }

    #[test]
    fn reordering_prefers_the_more_bound_read_among_ready_atoms() {
        // `b{x, y}` shares `x` with the preceding read, so it becomes ready
        // with one already-bound column once `a{x}` runs; `c{y}` shares
        // nothing with what's bound so far. the planner should schedule the
        // join (`b`) before the unrelated scan (`c`) even though `c` was
        // written first (P1, query-optimization.md).
        let p = ir("?[x, y] := a{x}, c{y}, b{x, y}");
        let entry = p.strata.last().unwrap().rules.iter().find(|r| r.head.name.is_none()).unwrap();
        let names: Vec<&str> = entry.body.iter().filter_map(|a| a.rel_name()).collect();
        assert_eq!(names, vec!["a", "b", "c"], "b (shares x) should schedule before c (shares nothing)");
    }

    #[test]
    fn reordering_pushes_cond_atoms_to_their_earliest_bound_point() {
        // mirrors lytics' passage_ids `mtc` rule exactly: two Cond filters
        // depend on `arrival_ts`, which only the third read introduces —
        // they must land strictly after it, never before.
        let p = ir(
            "mtc[neuron, ts, arrival_ts] := ts_ev{neuron, ts}, fs{neuron, first}, arrival_ev{neuron, ts: arrival_ts}, gt(arrival_ts, first), le(arrival_ts, ts)\n?[neuron, ts, count(arrival_ts)] := mtc[neuron, ts, arrival_ts]",
        );
        let mtc = p
            .strata
            .iter()
            .flat_map(|s| &s.rules)
            .find(|r| r.head.name.as_deref() == Some("mtc"))
            .unwrap();
        let arrival_pos = mtc
            .body
            .iter()
            .position(|a| matches!(a, Atom::Read { rel, .. } if rel == "arrival_ev"))
            .unwrap();
        for (i, a) in mtc.body.iter().enumerate() {
            if let Atom::Cond(c) = a {
                assert!(i > arrival_pos, "{} must run after arrival_ev binds arrival_ts", c.func);
            }
        }
    }

    #[test]
    fn negation_stratifies_above() {
        // ? depends (negatively) on `linked`, so it lands in a higher stratum
        let p = ir(
            "linked[p] := axons{from: #t, to: p}\n?[p] := focus{particle: p, score: s}, not linked[p]",
        );
        assert!(p.strata.len() >= 2);
    }

    #[test]
    fn negation_in_recursion_is_rejected() {
        let r = plan(&parse("r[p] := axons{from: #s, to: p}\nr[p] := focus{particle: p}, not r[p]\n?[p] := r[p]").unwrap());
        assert!(r.is_err(), "negation over self should be unstratifiable");
    }

    #[test]
    fn unsafe_head_var_rejected() {
        let r = plan(&parse("?[x, y] := axons{from: x, to: x}").unwrap());
        assert!(r.is_err(), "y is not bound");
    }

    #[test]
    fn missing_entry_rejected() {
        let r = plan(&parse("r[x] := axons{from: x, to: x}").unwrap());
        assert!(r.is_err());
    }
}
