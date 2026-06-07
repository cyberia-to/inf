//! The `inf` command-line tool: parse, plan, cost, and run inf programs.
//!
//! Commands:
//!   inf check <file>     parse + plan; report strata or errors
//!   inf plan  <file>     print the compiled IR
//!   inf cost  <file>     report a static cycle-ceiling estimate
//!   inf run   <file>     evaluate against the built-in demo graph and print rows
//!
//! `run`/`cost` evaluate over a built-in demo graph (the bootstrap has no bbg
//! source yet); the engine and pipeline are otherwise the real thing.

mod demo;

use inf_eval::{eval, Ctx, MutOp, Output};
use inf_parse::parse;
use inf_plan::plan;
use inf_source::RelationSource;
use inf_value::{Tuple, Value};
use std::process::exit;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: inf <check|plan|cost|run> <file.inf>");
        exit(2);
    }
    let cmd = args[1].as_str();
    let src = match std::fs::read_to_string(&args[2]) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot read {}: {e}", args[2]);
            exit(2);
        }
    };
    let code = match cmd {
        "check" => cmd_check(&src),
        "plan" => cmd_plan(&src),
        "cost" => cmd_cost(&src),
        "run" => cmd_run(&src),
        other => {
            eprintln!("unknown command `{other}` (use check|plan|cost|run)");
            2
        }
    };
    exit(code);
}

fn parse_plan(src: &str) -> Result<inf_ast::IrProgram, i32> {
    let prog = parse(src).map_err(|e| {
        eprintln!("parse error [{}:{}]: {}", e.line, e.col, e.msg);
        1
    })?;
    plan(&prog).map_err(|e| {
        eprintln!("plan error: {}", e.msg);
        1
    })
}

fn cmd_check(src: &str) -> i32 {
    match parse_plan(src) {
        Ok(ir) => {
            let rec = ir.strata.iter().filter(|s| s.recursive).count();
            println!(
                "ok: {} strata ({rec} recursive), entry `{}`{}",
                ir.strata.len(),
                ir.entry,
                if ir.mutation.is_some() { ", mutation" } else { "" }
            );
            0
        }
        Err(c) => c,
    }
}

fn cmd_plan(src: &str) -> i32 {
    match parse_plan(src) {
        Ok(ir) => {
            for (i, s) in ir.strata.iter().enumerate() {
                println!(
                    "stratum {i}{}{}:",
                    if s.recursive { " (recursive)" } else { "" },
                    s.bound.map(|b| format!(" bound={b}")).unwrap_or_default()
                );
                for r in &s.rules {
                    let h = r.head.name.clone().unwrap_or_else(|| "?".into());
                    if let Some(f) = &r.fixed {
                        println!("  {h} <~ {}({}[], ..)", f.algo, f.edges);
                    } else {
                        println!("  {h} := ({} atoms)", r.body.len());
                    }
                }
            }
            0
        }
        Err(c) => c,
    }
}

fn cmd_cost(src: &str) -> i32 {
    let ir = match parse_plan(src) {
        Ok(ir) => ir,
        Err(c) => return c,
    };
    let g = demo::graph();
    // A static estimate: Σ strata (rounds × Σ rules Σ read-atoms × relation size).
    // rounds = the bound for a recursive stratum, else 1.
    let size = |rel: &str| -> u64 {
        if let Some(_s) = g.schema(rel) {
            g.scan(rel).count() as u64
        } else {
            // a derived relation: bound by the largest base relation
            64
        }
    };
    let mut total: u64 = 0;
    for s in &ir.strata {
        let rounds = if s.recursive { s.bound.unwrap_or(16) } else { 1 };
        let mut stratum_cost = 0u64;
        for r in &s.rules {
            if let Some(f) = &r.fixed {
                stratum_cost += size(&f.edges).saturating_mul(8); // algo over edges
            } else {
                for a in &r.body {
                    if let Some(rel) = a.rel_name() {
                        stratum_cost = stratum_cost.saturating_add(size(rel));
                    }
                }
            }
        }
        total = total.saturating_add(rounds.saturating_mul(stratum_cost));
    }
    println!("estimated work: ~{total} units (read+combine, rounds folded in)");
    0
}

fn cmd_run(src: &str) -> i32 {
    let ir = match parse_plan(src) {
        Ok(ir) => ir,
        Err(c) => return c,
    };
    match eval(&ir, &demo::graph(), &Ctx::default()) {
        Ok(out) => {
            print_output(&out);
            0
        }
        Err(e) => {
            eprintln!("eval error: {}", e.msg);
            1
        }
    }
}

fn print_output(out: &Output) {
    if let Some(m) = &out.mutation {
        let op = match m {
            MutOp::Link => "link".to_string(),
            MutOp::Unlink => "unlink".to_string(),
            MutOp::Put(r) => format!("put {r}"),
            MutOp::Rm(r) => format!("rm {r}"),
        };
        println!("signal ({op}) — {} cyberlinks", out.rows.len());
    }
    println!("{}", out.columns.join("\t"));
    for row in &out.rows {
        let cells: Vec<String> = row.iter().map(fmt_value).collect();
        println!("{}", cells.join("\t"));
    }
    if out.mutation.is_none() {
        println!("({} rows)", out.rows.len());
    }
}

fn fmt_value(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Field(f) => format!("f{}", f.val()),
        Value::Word(w) => w.to_string(),
        Value::Hash(h) => format!("#{:02x}{:02x}{:02x}{:02x}", h[0], h[1], h[2], h[3]),
        Value::Bytes(b) => match std::str::from_utf8(b) {
            Ok(s) => format!("\"{s}\""),
            Err(_) => format!("0x{}", b.iter().map(|x| format!("{x:02x}")).collect::<String>()),
        },
        Value::List(items) => {
            let inner: Vec<String> = items.iter().map(fmt_value).collect();
            format!("[{}]", inner.join(", "))
        }
    }
}

#[allow(dead_code)]
fn _tuple_marker(_: &Tuple) {}
