//! Aggregation over a group of bindings (specs/functions.md). Reductions are
//! exact: `count`/`sum`/`min`/`max`, `mean` as integer division of `(sum, count)`
//! (no float), and the set collectors `collect`/`unique`/`union`.

use crate::expr::Binding;
use inf_ast::{Head, HeadArg};
use inf_value::{Tuple, Value};
use std::collections::{BTreeMap, BTreeSet};

pub fn aggregate(head: &Head, binds: &[Binding]) -> Result<Vec<Tuple>, String> {
    let group_vars: Vec<&str> = head
        .args
        .iter()
        .filter_map(|a| match a {
            HeadArg::Var(v) => Some(v.as_str()),
            _ => None,
        })
        .collect();

    let mut groups: BTreeMap<Vec<Value>, Vec<Binding>> = BTreeMap::new();
    for b in binds {
        let key: Vec<Value> = group_vars
            .iter()
            .map(|v| b.get(*v).cloned().unwrap_or(Value::Null))
            .collect();
        groups.entry(key).or_default().push(b.clone());
    }

    let mut out = Vec::new();
    for (key, members) in groups {
        let mut tuple = Vec::new();
        let mut gi = 0;
        for arg in &head.args {
            match arg {
                HeadArg::Var(_) => {
                    tuple.push(key[gi].clone());
                    gi += 1;
                }
                HeadArg::Aggr { op, var } => {
                    let vals: Vec<Value> =
                        members.iter().filter_map(|b| b.get(var).cloned()).collect();
                    tuple.push(reduce(op, &vals)?);
                }
            }
        }
        out.push(tuple);
    }
    Ok(out)
}

/// Bounded ordered aggregate (specs/language.md, P2 —
/// `.claude/plans/query-optimization.md`): partitions `binds` by the head's
/// plain vars EXCLUDING `order_var`, sorts each partition by `order_var`,
/// and emits one output row PER INPUT ROW carrying the running aggregate
/// accumulated through that row — unlike `aggregate`, which collapses each
/// group to a single row. `plan()`'s safety check guarantees `order_var` is
/// positively bound and every `running_*` head aggregate has exactly this
/// pairing.
pub fn windowed_aggregate(head: &Head, binds: &[Binding], order_var: &str) -> Result<Vec<Tuple>, String> {
    let group_vars: Vec<&str> = head
        .args
        .iter()
        .filter_map(|a| match a {
            HeadArg::Var(v) if v != order_var => Some(v.as_str()),
            _ => None,
        })
        .collect();

    let mut groups: BTreeMap<Vec<Value>, Vec<&Binding>> = BTreeMap::new();
    for b in binds {
        let key: Vec<Value> = group_vars.iter().map(|v| b.get(*v).cloned().unwrap_or(Value::Null)).collect();
        groups.entry(key).or_default().push(b);
    }

    let mut out = Vec::new();
    for (_, mut members) in groups {
        members.sort_by(|a, b| a.get(order_var).cmp(&b.get(order_var)));
        let mut running: BTreeMap<&str, i64> = BTreeMap::new();
        for b in members {
            let mut tuple = Vec::new();
            for arg in &head.args {
                match arg {
                    HeadArg::Var(v) => {
                        tuple.push(b.get(v).cloned().unwrap_or(Value::Null));
                    }
                    HeadArg::Aggr { op, var } => {
                        tuple.push(running_reduce(op, var, b, &mut running)?);
                    }
                }
            }
            out.push(tuple);
        }
    }
    Ok(out)
}

/// One step of a running aggregate for the current row, given the
/// accumulator state carried from prior rows in this partition (keyed by
/// aggregate variable, so a rule with more than one running aggregate over
/// different variables tracks each independently).
fn running_reduce<'a>(
    op: &str,
    var: &'a str,
    b: &Binding,
    running: &mut BTreeMap<&'a str, i64>,
) -> Result<Value, String> {
    match op {
        "running_count" => {
            let acc = running.entry(var).or_insert(0);
            if b.get(var).is_some() {
                *acc += 1;
            }
            Ok(Value::Int(*acc))
        }
        other => Err(format!("unknown ordered aggregation `{other}` (expected a `running_*` op)")),
    }
}

fn reduce(op: &str, vals: &[Value]) -> Result<Value, String> {
    match op {
        "count" => Ok(Value::Int(vals.len() as i64)),
        "sum" => {
            let mut s = 0i64;
            for v in vals {
                let i = v.as_int().ok_or("sum over non-integer")?;
                s = s.checked_add(i).ok_or("overflow in sum")?;
            }
            Ok(Value::Int(s))
        }
        "min" => vals.iter().cloned().min().ok_or_else(|| "min of empty group".into()),
        "max" => vals.iter().cloned().max().ok_or_else(|| "max of empty group".into()),
        "mean" => {
            if vals.is_empty() {
                return Err("mean of empty group".into());
            }
            let mut s = 0i64;
            for v in vals {
                s = s.checked_add(v.as_int().ok_or("mean over non-integer")?).ok_or("overflow in mean")?;
            }
            Ok(Value::Int(s / vals.len() as i64))
        }
        "collect" => {
            let mut v = vals.to_vec();
            v.sort();
            Ok(Value::List(v))
        }
        "unique" => {
            let set: BTreeSet<Value> = vals.iter().cloned().collect();
            Ok(Value::List(set.into_iter().collect()))
        }
        "union" => {
            let mut set: BTreeSet<Value> = BTreeSet::new();
            for v in vals {
                match v {
                    Value::List(items) => set.extend(items.iter().cloned()),
                    other => {
                        set.insert(other.clone());
                    }
                }
            }
            Ok(Value::List(set.into_iter().collect()))
        }
        other => Err(format!("unknown aggregation `{other}`")),
    }
}
