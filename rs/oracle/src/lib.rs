//! Differential oracle. The same logical query is expressed for both inf and
//! CozoScript over identical integer/string data; the results must match. This
//! pins inf-eval's semantics to a battle-tested datalog engine on the subset the
//! two share (integers, bounded recursion, joins, aggregation, negation).

#![cfg(test)]

use cozo::{DataValue, DbInstance};
use inf_eval::{eval, Ctx};
use inf_parse::parse;
use inf_plan::plan;
use inf_source::LocalSource;
use inf_value::Value;

#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Clone)]
enum Norm {
    Int(i64),
    Str(String),
    List(Vec<Norm>),
    Other(String),
}

fn norm_inf(v: &Value) -> Norm {
    match v {
        Value::Int(i) => Norm::Int(*i),
        Value::Bool(b) => Norm::Int(*b as i64),
        Value::Bytes(b) => Norm::Str(String::from_utf8_lossy(b).into_owned()),
        Value::List(l) => Norm::List(l.iter().map(norm_inf).collect()),
        other => Norm::Other(format!("{other:?}")),
    }
}

fn norm_cozo(v: &DataValue) -> Norm {
    if let Some(i) = v.get_int() {
        Norm::Int(i)
    } else if let Some(s) = v.get_str() {
        Norm::Str(s.to_string())
    } else if let Some(sl) = v.get_slice() {
        Norm::List(sl.iter().map(norm_cozo).collect())
    } else {
        Norm::Other(format!("{v:?}"))
    }
}

fn sorted(mut rows: Vec<Vec<Norm>>) -> Vec<Vec<Norm>> {
    rows.sort();
    rows
}

/// inf side: relations `num{id, val}` and `edge{a, b}`, integer data.
fn inf_source() -> LocalSource {
    let mut s = LocalSource::new();
    let n = |a: i64, b: i64| vec![Value::Int(a), Value::Int(b)];
    s.add("num", &["id", "val"], vec![n(1, 10), n(2, 3), n(3, 20), n(4, 8)]);
    s.add("edge", &["a", "b"], vec![n(1, 2), n(2, 3), n(1, 4)]);
    s
}

/// cozo side: the identical data, loaded via CozoScript.
fn cozo_db() -> DbInstance {
    let db = DbInstance::new("mem", "", "").unwrap();
    db.run_default(":create num {id => val}").unwrap();
    db.run_default(":create edge {a, b}").unwrap();
    db.run_default("?[id, val] <- [[1,10],[2,3],[3,20],[4,8]] :put num {id, val}").unwrap();
    db.run_default("?[a, b] <- [[1,2],[2,3],[1,4]] :put edge {a, b}").unwrap();
    db
}

fn check(inf_q: &str, cozo_q: &str) {
    let ir = plan(&parse(inf_q).expect("inf parse")).expect("inf plan");
    let out = eval(&ir, &inf_source(), &Ctx::default()).expect("inf eval");
    let inf_rows = sorted(out.rows.iter().map(|r| r.iter().map(norm_inf).collect()).collect());

    let nr = cozo_db().run_default(cozo_q).expect("cozo run");
    let cozo_rows = sorted(nr.rows.iter().map(|r| r.iter().map(norm_cozo).collect()).collect());

    assert_eq!(inf_rows, cozo_rows, "\n  inf:  {inf_q}\n  cozo: {cozo_q}");
}

#[test]
fn filter() {
    check(
        "?[id, val] := num{id, val}, gt(val, 5)",
        "?[id, val] := *num{id, val}, val > 5",
    );
}

#[test]
fn join() {
    check(
        "?[a, val] := edge{a, b}, num{id: b, val}",
        "?[a, val] := *edge{a, b}, *num{id: b, val}",
    );
}

#[test]
fn aggregation_count() {
    check(
        "?[a, count(b)] := edge{a, b}",
        "?[a, count(b)] := *edge{a, b}",
    );
}

#[test]
fn transitive_closure() {
    check(
        "reach[x] := edge{a, b: x}, eq(a, 1)\nreach[x] := reach[mid], edge{a: mid, b: x}\n?[x] := reach[x]",
        "reach[x] := *edge{a, b: x}, a == 1\nreach[x] := reach[mid], *edge{a: mid, b: x}\n?[x] := reach[x]",
    );
}

#[test]
fn negation() {
    // ids whose value is not > 5 (i.e. the small ones)
    check(
        "big[id] := num{id, val}, gt(val, 5)\n?[id] := num{id, val}, not big[id]",
        "big[id] := *num{id, val}, val > 5\n?[id] := *num{id, val}, not big[id]",
    );
}
