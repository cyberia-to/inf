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
    fn me_resolves() {
        let b = Binding::new();
        let c = Ctx::default();
        let v = eval_term(&Term::Addr(Addr::Neuron("me".into())), &b, &c).unwrap();
        assert_eq!(v, Value::Hash(tag_hash("@me")));
    }
}
