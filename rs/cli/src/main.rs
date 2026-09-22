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
    let mut g = demo::graph();
    g.ensure_stats(); // compute committed stats from demo data

    // Committed stats when available (specs/cost.md): node_count + relation_sizes
    // from the BBG root give a static bound without scanning at query time.
    let stats = g.graph_stats();
    let node_count = stats.as_ref().map(|s| s.node_count).unwrap_or(64);
    let diameter = stats.as_ref().map(|s| s.diameter_bound).unwrap_or(16);

    // A static estimate: Σ strata (rounds × Σ rules Σ read-atoms × relation size).
    // rounds = explicit `:bounded N` for recursive strata, else diameter_bound.
    let size = |rel: &str| -> u64 {
        if let Some(_s) = g.schema(rel) {
            g.scan(rel).count() as u64
        } else {
            // derived relation: bound by the committed node_count
            node_count
        }
    };
    let mut reads = 0u64;
    let mut combine = 0u64;
    let mut total = 0u64;
    let mut max_rounds = 1u64;
    for s in &ir.strata {
        let rounds = if s.recursive {
            s.bound.unwrap_or(diameter) // committed diameter if no explicit bound
        } else {
            1
        };
        max_rounds = max_rounds.max(rounds);
        let (mut sr, mut sc) = (0u64, 0u64);
        for r in &s.rules {
            if let Some(f) = &r.fixed {
                sr = sr.saturating_add(size(&f.edges));
                sc = sc.saturating_add(size(&f.edges).saturating_mul(7)); // algorithm core
            } else {
                let mut natoms = 0u64;
                for a in &r.body {
                    if let Some(rel) = a.rel_name() {
                        sr = sr.saturating_add(size(rel));
                        natoms += 1;
                    }
                }
                sc = sc.saturating_add(natoms.saturating_sub(1).saturating_mul(4)); // joins
            }
        }
        reads = reads.saturating_add(sr);
        combine = combine.saturating_add(sc);
        total = total.saturating_add(rounds.saturating_mul(sr.saturating_add(sc)));
    }
    println!("estimated work: ~{total} units");
    println!("  reads:     {reads}  (Lens openings / scans — bbg side)");
    println!("  combine:   {combine}  (joins + algorithm cores — inf side)");
    println!("  recursion: up to ×{max_rounds} on recursive strata");
    if let Some(s) = &stats {
        println!("  committed: node_count={} diameter_bound={} max_degree={}",
            s.node_count, s.diameter_bound, s.max_degree);
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use inf_value::F;

    // matches demo::graph()'s axons/focus schema exactly (see inf-parse's own
    // vertical_slice test).
    const VALID: &str = "?[to, score] := axons{from: #seed, to}, focus{particle: to, score}, gt(score, 5)\n:sort -score\n:limit 20";
    const RECURSIVE: &str = "reachable[p] := axons{from: #seed, to: p}\nreachable[p] := reachable[mid], axons{from: mid, to: p}\n?[p] := reachable[p]\n:bounded 5";

    #[test]
    fn parse_plan_accepts_a_valid_program() {
        let ir = parse_plan(VALID).unwrap();
        assert_eq!(ir.strata.len(), 1);
    }

    #[test]
    fn parse_plan_rejects_a_syntax_error_with_code_one() {
        assert_eq!(parse_plan("not inf syntax {{{").unwrap_err(), 1);
    }

    #[test]
    fn cmd_run_reports_an_eval_error_for_an_unresolved_relation() {
        // parses and plans fine — relation existence is an eval-time concern,
        // not a plan()-time one.
        assert_eq!(cmd_run("?[x] := nonexistent_relation{x}"), 1);
    }

    #[test]
    fn cmd_check_reports_stratum_and_recursion_shape() {
        assert_eq!(cmd_check(VALID), 0);
        assert_eq!(cmd_check(RECURSIVE), 0);
        assert_eq!(cmd_check("not inf syntax {{{"), 1);
    }

    #[test]
    fn cmd_plan_succeeds_on_a_valid_program_and_propagates_errors() {
        assert_eq!(cmd_plan(VALID), 0);
        assert_eq!(cmd_plan(RECURSIVE), 0);
        assert_eq!(cmd_plan("not inf syntax {{{"), 1);
    }

    #[test]
    fn cmd_cost_succeeds_on_a_valid_program_and_propagates_errors() {
        assert_eq!(cmd_cost(VALID), 0);
        assert_eq!(cmd_cost(RECURSIVE), 0);
        assert_eq!(cmd_cost("not inf syntax {{{"), 1);
    }

    #[test]
    fn cmd_run_succeeds_on_a_valid_program_and_propagates_parse_errors() {
        assert_eq!(cmd_run(VALID), 0);
        assert_eq!(cmd_run("not inf syntax {{{"), 1);
    }

    #[test]
    fn fmt_value_null_bool_int_word() {
        assert_eq!(fmt_value(&Value::Null), "null");
        assert_eq!(fmt_value(&Value::Bool(true)), "true");
        assert_eq!(fmt_value(&Value::Bool(false)), "false");
        assert_eq!(fmt_value(&Value::Int(-3)), "-3");
        assert_eq!(fmt_value(&Value::Word(9)), "9");
    }

    #[test]
    fn fmt_value_field_uses_f_prefix() {
        assert_eq!(fmt_value(&Value::Field(F::from_u64(7))), "f7");
    }

    #[test]
    fn fmt_value_hash_prints_first_four_bytes() {
        let mut h = [0u8; 32];
        h[0] = 0xde;
        h[1] = 0xad;
        h[2] = 0xbe;
        h[3] = 0xef;
        h[4] = 0xff; // must not appear — only the first four bytes print
        assert_eq!(fmt_value(&Value::Hash(h)), "#deadbeef");
    }

    #[test]
    fn fmt_value_bytes_prints_valid_utf8_as_a_quoted_string() {
        assert_eq!(fmt_value(&Value::Bytes(b"hi".to_vec())), "\"hi\"");
    }

    #[test]
    fn fmt_value_bytes_falls_back_to_hex_for_invalid_utf8() {
        assert_eq!(fmt_value(&Value::Bytes(vec![0xff, 0x00])), "0xff00");
    }

    #[test]
    fn fmt_value_list_joins_elements_with_comma_space() {
        let v = Value::List(vec![Value::Int(1), Value::Bool(true), Value::Null]);
        assert_eq!(fmt_value(&v), "[1, true, null]");
    }
}
