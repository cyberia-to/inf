//! Term and condition evaluation. Arithmetic and comparison are computed here in
//! the reference engine (the proof path delegates them to Tri/Bt/Ten). Values are
//! exact: integers and field elements, never floats.

use inf_ast::{Addr, Call, Term};
use inf_value::{tag_hash, Value, F};
use std::collections::{BTreeMap, BTreeSet};

/// A variable binding produced during evaluation.
pub type Binding = BTreeMap<String, Value>;

/// A live-register host provider: resolves `Host.fn(args)` to a witness value.
pub type HostFn = Box<dyn Fn(&str, &[Value]) -> Result<Value, String>>;

/// Optional nox-path condition evaluator. Returns Some(true) if the condition
/// holds, Some(false) if it doesn't, or None to fall back to the reference engine.
/// Used by inf-lower to route arithmetic/comparison conditions through the nox VM
/// while leaving hash/string conditions on the reference path.
pub type CondEval = Box<dyn Fn(&Call, &Binding) -> Option<bool>>;

/// Session context: the identity `@me` resolves to, and the optional live host.
pub struct Ctx {
    pub self_neuron: [u8; 32],
    pub host: Option<HostFn>,
    /// If set, condition atoms attempt nox evaluation first; fallback on None.
    pub nox_cond: Option<CondEval>,
}

impl Default for Ctx {
    fn default() -> Self {
        Ctx { self_neuron: tag_hash("@me"), host: None, nox_cond: None }
    }
}

pub fn resolve_addr(a: &Addr, ctx: &Ctx) -> Value {
    match a {
        Addr::Particle(p) => Value::Hash(tag_hash(p)),
        Addr::Neuron(n) if n == "me" => Value::Hash(ctx.self_neuron),
        Addr::Neuron(n) => Value::Hash(tag_hash(&format!("@{n}"))),
        Addr::Name(n) => Value::Hash(tag_hash(&format!("~{n}"))),
    }
}

pub fn eval_term(t: &Term, b: &Binding, ctx: &Ctx) -> Result<Value, String> {
    Ok(match t {
        Term::Var(v) => b
            .get(v)
            .cloned()
            .ok_or_else(|| format!("unbound variable `{v}`"))?,
        Term::Int(i) => Value::Int(*i),
        Term::Bool(x) => Value::Bool(*x),
        Term::Str(s) => Value::str(s),
        Term::Field(u) => Value::Field(F::from_u64(*u)),
        Term::Addr(a) => resolve_addr(a, ctx),
        Term::List(items) => Value::List(
            items
                .iter()
                .map(|x| eval_term(x, b, ctx))
                .collect::<Result<_, _>>()?,
        ),
        Term::Call(c) => eval_call(c, b, ctx)?,
    })
}

pub fn eval_call(c: &Call, b: &Binding, ctx: &Ctx) -> Result<Value, String> {
    // The sibling (Tri/Rs/Bt/Ten) is the proof-path owner; the reference engine
    // computes the same operation directly, dispatching on the function name.
    let a: Vec<Value> = c
        .args
        .iter()
        .map(|x| eval_term(x, b, ctx))
        .collect::<Result<_, _>>()?;
    // live register: `Host.fn(args)` is resolved by the host provider as a witness
    if c.sibling.as_deref() == Some("Host") {
        let host = ctx
            .host
            .as_ref()
            .ok_or("live register: no host provider configured")?;
        return host(&c.func, &a);
    }
    let arity = |n: usize| -> Result<(), String> {
        if a.len() == n {
            Ok(())
        } else {
            Err(format!("`{}` expects {n} args, got {}", c.func, a.len()))
        }
    };
    Ok(match c.func.as_str() {
        "add" | "sub" | "mul" | "div" => {
            arity(2)?;
            arith(&c.func, &a[0], &a[1])?
        }
        "gt" => {
            arity(2)?;
            Value::Bool(a[0] > a[1])
        }
        "lt" => {
            arity(2)?;
            Value::Bool(a[0] < a[1])
        }
        "ge" => {
            arity(2)?;
            Value::Bool(a[0] >= a[1])
        }
        "le" => {
            arity(2)?;
            Value::Bool(a[0] <= a[1])
        }
        "eq" => {
            arity(2)?;
            Value::Bool(a[0] == a[1])
        }
        "neq" => {
            arity(2)?;
            Value::Bool(a[0] != a[1])
        }
        "min" => {
            arity(2)?;
            if a[0] <= a[1] { a[0].clone() } else { a[1].clone() }
        }
        "max" => {
            arity(2)?;
            if a[0] >= a[1] { a[0].clone() } else { a[1].clone() }
        }
        "and" => {
            arity(2)?;
            Value::Bool(a[0].truthy() && a[1].truthy())
        }
        "or" => {
            arity(2)?;
            Value::Bool(a[0].truthy() || a[1].truthy())
        }
        "not" => {
            arity(1)?;
            Value::Bool(!a[0].truthy())
        }
        "in" => {
            arity(2)?;
            match &a[1] {
                Value::List(items) => Value::Bool(items.contains(&a[0])),
                _ => return Err("`in` expects a list as its second argument".into()),
            }
        }
        "mod" => {
            arity(2)?;
            let (x, y) = (a[0].as_int().ok_or("mod: integers")?, a[1].as_int().ok_or("mod: integers")?);
            if y == 0 {
                return Err("modulo by zero".into());
            }
            Value::Int(x % y)
        }
        "neg" => {
            arity(1)?;
            match &a[0] {
                Value::Int(i) => Value::Int(-i),
                Value::Field(f) => Value::Field(f.neg()),
                _ => return Err("neg: number".into()),
            }
        }
        "abs" => {
            arity(1)?;
            Value::Int(a[0].as_int().ok_or("abs: integer")?.abs())
        }
        "length" => {
            arity(1)?;
            match &a[0] {
                Value::List(l) => Value::Int(l.len() as i64),
                Value::Bytes(b) => Value::Int(b.len() as i64),
                _ => return Err("length: list or bytes".into()),
            }
        }
        "concat" => {
            arity(2)?;
            match (&a[0], &a[1]) {
                (Value::Bytes(x), Value::Bytes(y)) => {
                    let mut v = x.clone();
                    v.extend_from_slice(y);
                    Value::Bytes(v)
                }
                (Value::List(x), Value::List(y)) => {
                    let mut v = x.clone();
                    v.extend_from_slice(y);
                    Value::List(v)
                }
                _ => return Err("concat: two bytes or two lists".into()),
            }
        }
        "lowercase" | "uppercase" => {
            arity(1)?;
            let s = as_str(&a[0])?;
            Value::str(&if c.func == "lowercase" { s.to_lowercase() } else { s.to_uppercase() })
        }
        "starts_with" | "ends_with" | "str_includes" => {
            arity(2)?;
            let (s, t) = (as_str(&a[0])?, as_str(&a[1])?);
            Value::Bool(match c.func.as_str() {
                "starts_with" => s.starts_with(&t),
                "ends_with" => s.ends_with(&t),
                _ => s.contains(&t),
            })
        }
        "first" | "last" => {
            arity(1)?;
            match &a[0] {
                Value::List(l) => {
                    let x = if c.func == "first" { l.first() } else { l.last() };
                    x.cloned().unwrap_or(Value::Null)
                }
                _ => return Err("first/last: list".into()),
            }
        }
        "get" => {
            arity(2)?;
            match (&a[0], &a[1]) {
                (Value::List(l), Value::Int(i)) if *i >= 0 => {
                    l.get(*i as usize).cloned().unwrap_or(Value::Null)
                }
                _ => return Err("get: (list, non-negative int)".into()),
            }
        }
        "coalesce" => {
            arity(2)?;
            if a[0].is_null() { a[1].clone() } else { a[0].clone() }
        }
        // type tests (specs/functions.md: type and identity section)
        "is_null"   => { arity(1)?; Value::Bool(matches!(&a[0], Value::Null)) }
        "is_int"    => { arity(1)?; Value::Bool(matches!(&a[0], Value::Int(_))) }
        "is_bytes"  => { arity(1)?; Value::Bool(matches!(&a[0], Value::Bytes(_))) }
        "is_string" => { arity(1)?; Value::Bool(matches!(&a[0], Value::Bytes(_))) }
        "is_bool"   => { arity(1)?; Value::Bool(matches!(&a[0], Value::Bool(_))) }
        "is_list"   => { arity(1)?; Value::Bool(matches!(&a[0], Value::List(_))) }
        "is_hash"   => { arity(1)?; Value::Bool(matches!(&a[0], Value::Hash(_))) }
        "is_field"  => { arity(1)?; Value::Bool(matches!(&a[0], Value::Field(_))) }
        "to_bytes"  => {
            arity(1)?;
            match &a[0] {
                Value::Bytes(b) => Value::Bytes(b.clone()),
                Value::Int(i) => Value::Bytes(i.to_le_bytes().to_vec()),
                other => Value::str(&format!("{other:?}")),
            }
        }
        "to_int" => {
            arity(1)?;
            match &a[0] {
                Value::Int(i) => Value::Int(*i),
                Value::Bool(b) => Value::Int(*b as i64),
                Value::Field(f) => Value::Int(f.val() as i64),
                Value::Bytes(b) => {
                    let s = std::str::from_utf8(b).map_err(|_| "to_int: invalid utf8")?;
                    Value::Int(s.trim().parse().map_err(|_| "to_int: not an integer")?)
                }
                _ => return Err("to_int: unsupported value".into()),
            }
        }
        "to_string" => {
            arity(1)?;
            let s = match &a[0] {
                Value::Int(i) => i.to_string(),
                Value::Bytes(b) => String::from_utf8_lossy(b).to_string(),
                Value::Bool(b) => b.to_string(),
                other => format!("{other:?}"),
            };
            Value::str(&s)
        }
        "pow" => {
            arity(2)?;
            let (x, y) = (a[0].as_int().ok_or("pow: integers")?, a[1].as_int().ok_or("pow: integers")?);
            if y < 0 {
                return Err("pow: negative exponent".into());
            }
            let mut r: i64 = 1;
            for _ in 0..y {
                r = r.checked_mul(x).ok_or("overflow in pow")?;
            }
            Value::Int(r)
        }
        "signum" => {
            arity(1)?;
            Value::Int(a[0].as_int().ok_or("signum: integer")?.signum())
        }
        "trim" | "trim_start" | "trim_end" => {
            arity(1)?;
            let s = as_str(&a[0])?;
            Value::str(match c.func.as_str() {
                "trim" => s.trim(),
                "trim_start" => s.trim_start(),
                _ => s.trim_end(),
            })
        }
        "chars" => {
            arity(1)?;
            Value::List(as_str(&a[0])?.chars().map(|ch| Value::str(&ch.to_string())).collect())
        }
        "reverse" => {
            arity(1)?;
            match &a[0] {
                Value::Bytes(_) => Value::str(&as_str(&a[0])?.chars().rev().collect::<String>()),
                Value::List(l) => {
                    let mut v = l.clone();
                    v.reverse();
                    Value::List(v)
                }
                _ => return Err("reverse: string or list".into()),
            }
        }
        "slice" => {
            arity(3)?;
            let i = a[1].as_int().ok_or("slice: integer")?.max(0) as usize;
            let j = a[2].as_int().ok_or("slice: integer")?.max(0) as usize;
            match &a[0] {
                Value::List(l) => {
                    Value::List(l.iter().skip(i).take(j.saturating_sub(i)).cloned().collect())
                }
                Value::Bytes(_) => {
                    let cs: Vec<char> = as_str(&a[0])?.chars().collect();
                    Value::str(&cs.iter().skip(i).take(j.saturating_sub(i)).collect::<String>())
                }
                _ => return Err("slice: list or string".into()),
            }
        }
        "sorted" => {
            arity(1)?;
            let mut l = as_list(&a[0])?;
            l.sort();
            Value::List(l)
        }
        "prepend" => {
            arity(2)?;
            let mut v = vec![a[0].clone()];
            v.extend(as_list(&a[1])?);
            Value::List(v)
        }
        "append" => {
            arity(2)?;
            let mut v = as_list(&a[0])?;
            v.push(a[1].clone());
            Value::List(v)
        }
        "union" | "intersection" | "difference" => {
            arity(2)?;
            let x: BTreeSet<Value> = as_list(&a[0])?.into_iter().collect();
            let y: BTreeSet<Value> = as_list(&a[1])?.into_iter().collect();
            let r: Vec<Value> = match c.func.as_str() {
                "union" => x.union(&y).cloned().collect(),
                "intersection" => x.intersection(&y).cloned().collect(),
                _ => x.difference(&y).cloned().collect(),
            };
            Value::List(r)
        }
        "vec" => {
            arity(1)?;
            match &a[0] {
                Value::List(_) => a[0].clone(),
                _ => return Err("vec: list of numbers".into()),
            }
        }
        "ip_dist" | "l2_dist" => {
            arity(2)?;
            let (x, y) = (as_list(&a[0])?, as_list(&a[1])?);
            if x.len() != y.len() {
                return Err("vector length mismatch".into());
            }
            let mut acc: i64 = 0;
            for (p, q) in x.iter().zip(y.iter()) {
                let pi = p.as_int().ok_or("vec: integer components")?;
                let qi = q.as_int().ok_or("vec: integer components")?;
                let term = if c.func == "ip_dist" {
                    pi.checked_mul(qi).ok_or("overflow")?
                } else {
                    let d = pi.checked_sub(qi).ok_or("overflow")?;
                    d.checked_mul(d).ok_or("overflow")?
                };
                acc = acc.checked_add(term).ok_or("overflow")?;
            }
            Value::Int(acc) // ip = dot product; l2 = squared distance (exact, no sqrt)
        }
        other => return crate::funcs::extra(other, &a),
    })
}

fn as_list(v: &Value) -> Result<Vec<Value>, String> {
    match v {
        Value::List(l) => Ok(l.clone()),
        _ => Err("expected a list".into()),
    }
}

fn arith(op: &str, x: &Value, y: &Value) -> Result<Value, String> {
    use Value::{Field, Int};
    match (x, y) {
        (Int(p), Int(q)) => {
            let r = match op {
                "add" => p.checked_add(*q),
                "sub" => p.checked_sub(*q),
                "mul" => p.checked_mul(*q),
                "div" => {
                    if *q == 0 {
                        return Err("integer division by zero".into());
                    }
                    p.checked_div(*q)
                }
                _ => unreachable!(),
            };
            Ok(Int(r.ok_or_else(|| format!("integer overflow in `{op}`"))?))
        }
        // any field operand promotes to field arithmetic
        (a, b) => {
            let f = to_field(a)?;
            let g = to_field(b)?;
            Ok(Field(match op {
                "add" => f.add(g),
                "sub" => f.sub(g),
                "mul" => f.mul(g),
                "div" => f.mul(g.inv().ok_or("field division by zero")?),
                _ => unreachable!(),
            }))
        }
    }
}

pub(crate) fn as_str(v: &Value) -> Result<String, String> {
    match v {
        Value::Bytes(b) => Ok(String::from_utf8_lossy(b).to_string()),
        _ => Err("expected a text value".into()),
    }
}

fn to_field(v: &Value) -> Result<F, String> {
    match v {
        Value::Field(f) => Ok(*f),
        Value::Int(i) if *i >= 0 => Ok(F::from_u64(*i as u64)),
        _ => Err(format!("cannot use {v:?} in field arithmetic")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inf_ast::Call;

    fn call(func: &str, args: Vec<Term>) -> Call {
        Call { sibling: None, func: func.into(), args }
    }

    #[test]
    fn arithmetic_is_exact() {
        let b = Binding::new();
        let c = Ctx::default();
        let e = eval_call(&call("mul", vec![Term::Int(6), Term::Int(7)]), &b, &c).unwrap();
        assert_eq!(e, Value::Int(42));
        assert!(eval_call(&call("div", vec![Term::Int(1), Term::Int(0)]), &b, &c).is_err());
    }

    #[test]
    fn comparisons() {
        let b = Binding::new();
        let c = Ctx::default();
        assert_eq!(
            eval_call(&call("gt", vec![Term::Int(5), Term::Int(3)]), &b, &c).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            eval_call(&call("in", vec![Term::Int(2), Term::List(vec![Term::Int(1), Term::Int(2)])]), &b, &c).unwrap(),
            Value::Bool(true)
        );
    }

    #[test]
    fn type_tests_and_conversions() {
        let b = Binding::new();
        let c = Ctx::default();
        assert_eq!(eval_call(&call("is_null",  vec![Term::Int(0)]),          &b, &c).unwrap(), Value::Bool(false));
        assert_eq!(eval_call(&call("is_null",  vec![Term::Str("".into())]),  &b, &c).unwrap(), Value::Bool(false));
        assert_eq!(eval_call(&call("is_int",   vec![Term::Int(5)]),          &b, &c).unwrap(), Value::Bool(true));
        assert_eq!(eval_call(&call("is_int",   vec![Term::Str("x".into())]), &b, &c).unwrap(), Value::Bool(false));
        assert_eq!(eval_call(&call("is_bytes", vec![Term::Str("a".into())]), &b, &c).unwrap(), Value::Bool(true));
        assert_eq!(eval_call(&call("is_list",  vec![Term::List(vec![])]),    &b, &c).unwrap(), Value::Bool(true));
        assert_eq!(eval_call(&call("is_bool",  vec![Term::Bool(true)]),      &b, &c).unwrap(), Value::Bool(true));
        assert_eq!(eval_call(&call("to_bytes", vec![Term::Int(1)]),          &b, &c).unwrap(),
            Value::Bytes(1i64.to_le_bytes().to_vec()));
    }

    #[test]
    fn string_and_list_functions() {
        let b = Binding::new();
        let c = Ctx::default();
        assert_eq!(
            eval_call(&call("starts_with", vec![Term::Str("hello".into()), Term::Str("he".into())]), &b, &c).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            eval_call(&call("length", vec![Term::List(vec![Term::Int(1), Term::Int(2)])]), &b, &c).unwrap(),
            Value::Int(2)
        );
        assert_eq!(
            eval_call(&call("mod", vec![Term::Int(10), Term::Int(3)]), &b, &c).unwrap(),
            Value::Int(1)
        );
        assert_eq!(
            eval_call(&call("uppercase", vec![Term::Str("ab".into())]), &b, &c).unwrap(),
            Value::str("AB")
        );
        assert_eq!(
            eval_call(&call("pow", vec![Term::Int(2), Term::Int(10)]), &b, &c).unwrap(),
            Value::Int(1024)
        );
        let lst = |xs: &[i64]| Term::List(xs.iter().map(|i| Term::Int(*i)).collect());
        // dot product (1*4 + 2*5 + 3*6) = 32
        assert_eq!(
            eval_call(&call("ip_dist", vec![lst(&[1, 2, 3]), lst(&[4, 5, 6])]), &b, &c).unwrap(),
            Value::Int(32)
        );
        // squared L2: (1-4)^2 + (2-6)^2 = 9 + 16 = 25
        assert_eq!(
            eval_call(&call("l2_dist", vec![lst(&[1, 2]), lst(&[4, 6])]), &b, &c).unwrap(),
            Value::Int(25)
        );
        assert_eq!(
            eval_call(&call("union", vec![lst(&[1, 2]), lst(&[2, 3])]), &b, &c).unwrap(),
            Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)])
        );
    }

    #[test]
    fn me_resolves() {
        let b = Binding::new();
        let c = Ctx::default();
        let v = eval_term(&Term::Addr(Addr::Neuron("me".into())), &b, &c).unwrap();
        assert_eq!(v, Value::Hash(tag_hash("@me")));
    }
}
