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
        let rules: Vec<Rule> =
            prog.rules.iter().filter(|r| stratum[&head_rel(r)] == s).cloned().collect();
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
    })
}

/// Derived relations a rule depends on, with strictness (negation/aggregation).
fn derived_deps(r: &Rule, derived: &HashSet<String>) -> Vec<(String, bool)> {
    let aggr = r.head.has_aggr();
    let mut out = Vec::new();
    for a in &r.body {
        match a {
            Atom::Read { rel, .. } | Atom::Apply { rule: rel, .. } => {
                if derived.contains(rel) {
                    out.push((rel.clone(), aggr));
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
            "reachable[p] := axons{from: #seed, to: p}\nreachable[p] := reachable[mid], axons{from: mid, to: p}\n?[p] := reachable[p]\n:bounded 5",
        );
        let rec = p.strata.iter().find(|s| s.recursive).expect("a recursive stratum");
        assert_eq!(rec.bound, Some(5));
        // the recursive stratum defines `reachable`
        assert!(rec.rules.iter().any(|r| r.head.name.as_deref() == Some("reachable")));
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
