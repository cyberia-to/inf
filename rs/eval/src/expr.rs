//! Term and condition evaluation. Arithmetic and comparison are computed here in
//! the reference engine (the proof path delegates them to Tri/Bt/Ten). Values are
//! exact: integers and field elements, never floats.

use inf_ast::{Addr, Call, Term};
use inf_value::{tag_hash, Value, F};
use std::collections::BTreeMap;

/// A variable binding produced during evaluation.
pub type Binding = BTreeMap<String, Value>;

/// Session context: the identity `@me` resolves to.
pub struct Ctx {
    pub self_neuron: [u8; 32],
}

impl Default for Ctx {
    fn default() -> Self {
        Ctx { self_neuron: tag_hash("@me") }
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
        other => return Err(format!("unknown function `{other}`")),
    })
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

fn as_str(v: &Value) -> Result<String, String> {
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
    }

    #[test]
    fn me_resolves() {
        let b = Binding::new();
        let c = Ctx::default();
        let v = eval_term(&Term::Addr(Addr::Neuron("me".into())), &b, &c).unwrap();
        assert_eq!(v, Value::Hash(tag_hash("@me")));
    }
}
