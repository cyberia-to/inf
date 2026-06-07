//! Stage-3 lowering: inf expressions → [[nox]] patterns, executed on the real nox
//! VM (specs/ir.md). This crate lowers the arithmetic/comparison layer — the
//! conditions and binds that every query uses — to nox opcode formulas (quote 1,
//! add 5, sub 6, mul 7, eq 9, lt 10) and runs them through `nox::reduce`. The
//! differential tests assert the nox result equals the reference engine's.
//!
//! Full relational lowering (look-based scans, compose joins, bounded-compose
//! recursion over multi-field tuples) is the next increment; this establishes
//! and verifies the path to the production nox VM.

use inf_ast::Term;
use nebu::Goldilocks;
use nox::{reduce, NounId, Order, Outcome, Tag};

const N: usize = 8192;
const BUDGET: u64 = 1_000_000;

#[derive(Debug, PartialEq)]
pub enum LowerError {
    Unsupported(String),
    Arena,
    Runtime(String),
}

/// nox opcodes used by the expression lowering.
fn opcode(func: &str) -> Option<u64> {
    Some(match func {
        "add" => 5,
        "sub" => 6,
        "mul" => 7,
        "eq" => 9,
        "lt" => 10,
        _ => return None,
    })
}

/// Lower a closed inf term into a nox formula noun.
fn lower_term(o: &mut Order<N>, t: &Term) -> Result<NounId, LowerError> {
    match t {
        Term::Int(i) if *i >= 0 => quote(o, *i as u64),
        Term::Field(u) => quote(o, *u),
        Term::Call(c) if c.sibling.is_none() => {
            let op = opcode(&c.func)
                .ok_or_else(|| LowerError::Unsupported(format!("operator `{}`", c.func)))?;
            if c.args.len() != 2 {
                return Err(LowerError::Unsupported(format!("`{}` arity", c.func)));
            }
            let a = lower_term(o, &c.args[0])?;
            let b = lower_term(o, &c.args[1])?;
            let body = o.cell(a, b).ok_or(LowerError::Arena)?;
            let tag = o.atom(Goldilocks::new(op), Tag::Field).ok_or(LowerError::Arena)?;
            o.cell(tag, body).ok_or(LowerError::Arena)
        }
        other => Err(LowerError::Unsupported(format!("{other:?}"))),
    }
}

/// `[1 v]` — quote a literal.
fn quote(o: &mut Order<N>, v: u64) -> Result<NounId, LowerError> {
    let one = o.atom(Goldilocks::new(1), Tag::Field).ok_or(LowerError::Arena)?;
    let val = o.atom(Goldilocks::new(v), Tag::Field).ok_or(LowerError::Arena)?;
    o.cell(one, val).ok_or(LowerError::Arena)
}

/// Lower a closed inf expression and run it on the nox VM, returning the field
/// element result.
pub fn lower_and_run(t: &Term) -> Result<u64, LowerError> {
    let mut o: Order<N> = Order::new();
    let formula = lower_term(&mut o, t)?;
    let object = o.atom(Goldilocks::new(0), Tag::Field).ok_or(LowerError::Arena)?;
    let calls = nox::NullCalls;
    let mut tracer = nox::NoTrace;
    match reduce(&mut o, object, formula, BUDGET, &calls, &mut tracer) {
        Outcome::Ok(id, _) => {
            let (g, _) = o.atom_value(id).ok_or(LowerError::Runtime("non-atom result".into()))?;
            Ok(g.as_u64())
        }
        Outcome::Halt(_) => Err(LowerError::Runtime("halted".into())),
        Outcome::Error(e) => Err(LowerError::Runtime(format!("{e:?}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inf_ast::{Call, Term};

    fn call(func: &str, a: Term, b: Term) -> Term {
        Term::Call(Box::new(Call { sibling: None, func: func.into(), args: vec![a, b] }))
    }
    fn int(i: i64) -> Term {
        Term::Int(i)
    }

    /// The differential test: nox result == reference engine result (as a field
    /// element) for arithmetic expressions.
    fn assert_matches(t: &Term) {
        let nox_val = lower_and_run(t).expect("lower+run");
        let reference = inf_eval::eval_const_term(t).expect("eval");
        let want = match reference {
            inf_value::Value::Int(i) => (i as u128 % inf_value::P as u128) as u64,
            inf_value::Value::Field(f) => f.val(),
            other => panic!("non-numeric reference {other:?}"),
        };
        assert_eq!(nox_val, want, "nox vs reference mismatch for {t:?}");
    }

    #[test]
    fn arithmetic_lowers_and_matches_reference() {
        assert_matches(&call("add", int(2), int(3)));
        assert_matches(&call("mul", int(6), int(7)));
        assert_matches(&call("sub", int(10), int(3)));
        // nested: (2 * 3) + 4
        assert_matches(&call("add", call("mul", int(2), int(3)), int(4)));
        // deeper: ((5 + 5) * 2) - 1
        assert_matches(&call("sub", call("mul", call("add", int(5), int(5)), int(2)), int(1)));
    }

    #[test]
    fn runs_on_the_real_nox_vm() {
        // a direct check that the lowering produced a runnable nox formula
        assert_eq!(lower_and_run(&call("add", int(40), int(2))).unwrap(), 42);
    }

    #[test]
    fn unsupported_is_reported_not_faked() {
        let r = lower_and_run(&Term::Str("x".into()));
        assert!(matches!(r, Err(LowerError::Unsupported(_))));
    }
}
