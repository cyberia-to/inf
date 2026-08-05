//! End-to-end engine tests: parse → plan → eval over a fixture graph. These are
//! the canonical-semantics corpus the nox lowering is differential-tested against.

use inf_eval::{eval, eval_reactive, Ctx, Event, MutOp, Output};
use inf_parse::parse;
use inf_plan::plan;
use inf_source::LocalSource;
use inf_value::{tag_hash, Tuple, Value};

fn p(name: &str) -> Value {
    Value::Hash(tag_hash(name))
}
fn me() -> Value {
    Value::Hash(tag_hash("@me"))
}

/// A small fixture: a chain seed→a→b→c and a branch seed→d, with focus, karma,
/// and a few cyberlinks owned by @me and @x.
fn fixture() -> LocalSource {
    let mut s = LocalSource::new();
    s.add(
        "axons",
        &["from", "to", "weight_sum"],
        vec![
            vec![p("seed"), p("a"), Value::int(10)],
            vec![p("a"), p("b"), Value::int(5)],
            vec![p("b"), p("c"), Value::int(2)],
            vec![p("seed"), p("d"), Value::int(1)],
        ],
    );
    s.add(
        "focus",
        &["particle", "score"],
        vec![
            vec![p("seed"), Value::int(1)],
            vec![p("a"), Value::int(30)],
            vec![p("b"), Value::int(20)],
            vec![p("c"), Value::int(5)],
            vec![p("d"), Value::int(8)],
        ],
    );
    s.add(
        "karma",
        &["neuron", "k"],
        vec![vec![me(), Value::int(1500)], vec![p("@x"), Value::int(200)]],
    );
    s.add(
        "cyberlinks",
        &["neuron", "from", "to"],
        vec![
            vec![me(), p("seed"), p("a")],
            vec![me(), p("seed"), p("d")],
            vec![p("@x"), p("a"), p("b")],
        ],
    );
    s
}

fn run(src: &str) -> Output {
    let prog = parse(src).expect("parse");
    let ir = plan(&prog).expect("plan");
    eval(&ir, &fixture(), &Ctx::default()).expect("eval")
}

fn col(out: &Output, name: &str) -> usize {
    out.columns.iter().position(|c| c == name).unwrap()
}

#[test]
fn discovery_join_filter_sort_limit() {
    let out = run("?[to, score] := axons{from: #seed, to}, focus{particle: to, score}, gt(score, 5)\n:sort -score\n:limit 10");
    // seed → {a(30), d(8)}; both clear the floor; sorted desc by score
    assert_eq!(out.rows.len(), 2);
    let sc = col(&out, "score");
    let to = col(&out, "to");
    assert_eq!(out.rows[0][to], p("a"));
    assert_eq!(out.rows[0][sc], Value::int(30));
    assert_eq!(out.rows[1][to], p("d"));
}

#[test]
fn bounded_reachability_stops_at_depth() {
    // depth: a,d = 1 ; b = 2 ; c = 3. bound 2 ⇒ {a, d, b}, no c.
    let out = run(
        "reachable[x] := axons{from: #seed, to: x}\nreachable[x] := reachable[mid], axons{from: mid, to: x}\n:bounded 2\n?[x] := reachable[x]",
    );
    let got: std::collections::BTreeSet<Tuple> = out.rows.iter().cloned().collect();
    let want: std::collections::BTreeSet<Tuple> =
        [vec![p("a")], vec![p("d")], vec![p("b")]].into_iter().collect();
    assert_eq!(got, want);
}

#[test]
fn full_closure_without_bound() {
    let out = run(
        "reachable[x] := axons{from: #seed, to: x}\nreachable[x] := reachable[mid], axons{from: mid, to: x}\n?[x] := reachable[x]",
    );
    // full closure from seed: a, b, c, d
    assert_eq!(out.rows.len(), 4);
}

#[test]
fn aggregation_count_per_neuron() {
    let out = run("?[neuron, count(to)] := cyberlinks{neuron, to}");
    // @me linked 2, @x linked 1
    let n = col(&out, "neuron");
    let c = col(&out, "to");
    let mut map = std::collections::BTreeMap::new();
    for r in &out.rows {
        map.insert(r[n].clone(), r[c].clone());
    }
    assert_eq!(map.get(&me()), Some(&Value::int(2)));
    assert_eq!(map.get(&p("@x")), Some(&Value::int(1)));
}

#[test]
fn negation_finds_unlinked_under_topic() {
    // particles @me did not link from #seed, among focus particles
    let out = run(
        "linked[x] := cyberlinks{neuron: @me, from: #seed, to: x}\n?[x] := focus{particle: x, score}, not linked[x]",
    );
    let got: std::collections::BTreeSet<Tuple> = out.rows.iter().cloned().collect();
    // @me linked a and d from seed; remaining focus particles: seed, b, c
    let want: std::collections::BTreeSet<Tuple> =
        [vec![p("seed")], vec![p("b")], vec![p("c")]].into_iter().collect();
    assert_eq!(got, want);
}

#[test]
fn underscore_temp_relation_usable_by_later_rules() {
    // a `_`-prefixed helper relation is an intra-program temp, visible downstream
    let out = run("_hot[x] := focus{particle: x, score}, gt(score, 10)\n?[x] := _hot[x]");
    let got: std::collections::BTreeSet<Tuple> = out.rows.iter().cloned().collect();
    let want: std::collections::BTreeSet<Tuple> =
        [vec![p("a")], vec![p("b")]].into_iter().collect(); // focus > 10: a(30), b(20)
    assert_eq!(got, want);
}

#[test]
fn assert_none_passes_when_invariant_holds() {
    // no neuron has negative karma
    let out = run("?[neuron, k] := karma{neuron, k}, lt(k, 0)\n:assert none");
    assert!(out.rows.is_empty());
}

#[test]
fn assert_none_fails_when_violated() {
    let prog = parse("?[neuron, k] := karma{neuron, k}, gt(k, 0)\n:assert none").unwrap();
    let ir = plan(&prog).unwrap();
    let r = eval(&ir, &fixture(), &Ctx::default());
    assert!(r.is_err(), ":assert none must fail when rows exist");
}

#[test]
fn mutation_derives_link_batch() {
    // link every particle that points to #b also to #new
    let out = run("?[from] := axons{from, to: #b}\n:link { neuron: @me, from, to: #new }");
    assert_eq!(out.mutation, Some(MutOp::Link));
    // a → b, so the batch is exactly one cyberlink: (@me, a, #new)
    assert_eq!(out.rows.len(), 1);
    assert_eq!(out.columns, vec!["neuron", "from", "to"]);
    let to = col(&out, "to");
    let neuron = col(&out, "neuron");
    for row in &out.rows {
        assert_eq!(row[to], p("new"));
        assert_eq!(row[neuron], me());
    }
}

#[test]
fn fixed_rule_dijkstra_shortest_path() {
    let out = run(
        "edges[from, to, w] := axons{from, to, weight_sum: w}\n?[path, cost] <~ ShortestPathDijkstra(edges[], #seed, #c)",
    );
    assert_eq!(out.rows.len(), 1);
    let path = col(&out, "path");
    let cost = col(&out, "cost");
    assert_eq!(out.rows[0][cost], Value::int(17)); // 10 + 5 + 2
    assert_eq!(
        out.rows[0][path],
        Value::List(vec![p("seed"), p("a"), p("b"), p("c")])
    );
}

#[test]
fn fixed_rule_connected_components_one_component() {
    let out = run("edges[from, to] := axons{from, to}\n?[node, comp] <~ ConnectedComponents(edges[])");
    assert_eq!(out.rows.len(), 5); // seed, a, b, c, d
    let comp = col(&out, "comp");
    let first = out.rows[0][comp].clone();
    assert!(out.rows.iter().all(|r| r[comp] == first), "all in one component");
}

#[test]
fn fixed_rule_degree_centrality() {
    let out = run("edges[from, to] := axons{from, to}\n?[node, deg, ind, outd] <~ DegreeCentrality(edges[])");
    let node = col(&out, "node");
    let outd = col(&out, "outd");
    let row = out.rows.iter().find(|r| r[node] == p("seed")).unwrap();
    assert_eq!(row[outd], Value::int(2)); // seed → a, seed → d
}

#[test]
fn fixed_rule_dfs_and_scc() {
    let dfs = run("edges[from, to] := axons{from, to}\n?[node, depth] <~ DepthFirstSearch(edges[], #seed)");
    assert_eq!(dfs.rows.len(), 5); // reaches all nodes
    let node = col(&dfs, "node");
    let depth = col(&dfs, "depth");
    let seed = dfs.rows.iter().find(|r| r[node] == p("seed")).unwrap();
    assert_eq!(seed[depth], Value::int(0));

    // the fixture is a DAG, so every node is its own strongly-connected component
    let scc = run("edges[from, to] := axons{from, to}\n?[node, comp] <~ StronglyConnectedComponent(edges[])");
    let c = col(&scc, "comp");
    let comps: std::collections::BTreeSet<_> = scc.rows.iter().map(|r| r[c].clone()).collect();
    assert_eq!(comps.len(), 5);
}

#[test]
fn fixed_rule_pagerank_deterministic_and_positive() {
    let q = "edges[from, to] := axons{from, to}\n?[node, rank] <~ PageRank(edges[], iters: 30)";
    let out = run(q);
    assert_eq!(out.rows.len(), 5);
    let rank = col(&out, "rank");
    assert!(out.rows.iter().all(|r| r[rank].as_int().unwrap() > 0), "ranks positive");
    // deterministic: same query, same result
    assert_eq!(run(q).rows, out.rows);
}

#[test]
fn fixed_rule_mst_and_astar_and_yen() {
    // MST of the (tree) fixture: all 4 edges, total weight 10+5+2+1 = 18
    let mst = run("edges[from, to, w] := axons{from, to, weight_sum: w}\n?[from, to, w] <~ MinimumSpanningForestKruskal(edges[])");
    assert_eq!(mst.rows.len(), 4);
    let w = col(&mst, "w");
    let total: i64 = mst.rows.iter().map(|r| r[w].as_int().unwrap()).sum();
    assert_eq!(total, 18);

    // A* seed→c (zero heuristic ⇒ Dijkstra): cost 17
    let a = run("edges[from, to, w] := axons{from, to, weight_sum: w}\n?[path, cost] <~ ShortestPathAStar(edges[], #seed, #c)");
    assert_eq!(a.rows[0][col(&a, "cost")], Value::int(17));

    // Yen k=2 from seed to c: only one path exists in the DAG
    let y = run("edges[from, to, w] := axons{from, to, weight_sum: w}\n?[path, cost] <~ KShortestPathYen(edges[], #seed, #c, k: 2)");
    assert_eq!(y.rows.len(), 1);
    assert_eq!(y.rows[0][col(&y, "cost")], Value::int(17));
}

#[test]
fn fixed_rule_centralities_and_community() {
    // betweenness: b lies on shortest paths (seed→c, a→c); sinks have 0
    let bw = run("edges[from, to] := axons{from, to}\n?[node, c] <~ BetweennessCentrality(edges[])");
    let n = col(&bw, "node");
    let c = col(&bw, "c");
    let bval = |who| bw.rows.iter().find(|r| r[n] == who).unwrap()[c].as_int().unwrap();
    assert!(bval(p("b")) > 0, "b is a bridge");
    assert_eq!(bval(p("c")), 0, "sink c has zero betweenness");

    // closeness: seed reaches others ⇒ positive; sink c reaches none ⇒ 0
    let cl = run("edges[from, to] := axons{from, to}\n?[node, c] <~ ClosenessCentrality(edges[])");
    let n2 = col(&cl, "node");
    let c2 = col(&cl, "c");
    assert!(cl.rows.iter().find(|r| r[n2] == p("seed")).unwrap()[c2].as_int().unwrap() > 0);

    // these run and label every node
    for algo in ["LabelPropagation", "ClusteringCoefficients", "CommunityDetectionLouvain"] {
        let q = format!("edges[from, to] := axons{{from, to}}\n?[node, x] <~ {algo}(edges[])");
        assert_eq!(run(&q).rows.len(), 5, "{algo} labels all nodes");
    }
}

#[test]
fn fixed_rule_random_walk_seeded_deterministic() {
    let q = "edges[from, to] := axons{from, to}\n?[node, visits] <~ RandomWalk(edges[], #seed, steps: 5, times: 10, seed: 42)";
    let out = run(q);
    assert!(!out.rows.is_empty());
    assert_eq!(run(q).rows, out.rows, "seeded walk is deterministic");
}

#[test]
fn reactive_re_evaluates_on_subscribed_events() {
    let ir = plan(&parse("?[to] := axons{from: #seed, to}\n:subscribe axons").unwrap()).unwrap();
    let mut base = LocalSource::new();
    base.add("axons", &["from", "to", "weight_sum"], vec![]);
    let events = vec![
        Event { rel: "axons".into(), tuple: vec![p("seed"), p("a"), Value::int(1)] },
        Event { rel: "axons".into(), tuple: vec![p("seed"), p("b"), Value::int(1)] },
    ];
    let outs = eval_reactive(&ir, base, &events, &Ctx::default()).unwrap();
    assert_eq!(outs.len(), 2); // each axons event fires the subscription
    assert_eq!(outs[0].rows.len(), 1); // {a}
    assert_eq!(outs[1].rows.len(), 2); // {a, b}
}

#[test]
fn live_host_call_is_a_witness() {
    let ir = plan(&parse("?[to, px] := axons{from: #seed, to}, px = Host.price(to)").unwrap()).unwrap();
    let ctx = Ctx {
        self_neuron: tag_hash("@me"),
        host: Some(Box::new(|func: &str, _args: &[Value]| {
            assert_eq!(func, "price");
            Ok(Value::int(99))
        })),
        nox_cond: None,
    };
    let out = eval(&ir, &fixture(), &ctx).unwrap();
    let px = col(&out, "px");
    assert!(!out.rows.is_empty());
    assert!(out.rows.iter().all(|r| r[px] == Value::int(99)));

    // without a host provider the live call is an error
    let r = eval(&ir, &fixture(), &Ctx::default());
    assert!(r.is_err(), "live register needs a host provider");
}

#[test]
fn infix_arithmetic_in_bind() {
    let out = run("?[to, boosted] := axons{from: #seed, to, weight_sum: w}, boosted = w * 2");
    let b = col(&out, "boosted");
    let to = col(&out, "to");
    let mut map = std::collections::BTreeMap::new();
    for r in &out.rows {
        map.insert(r[to].clone(), r[b].clone());
    }
    // seed→a weight 10 ⇒ 20 ; seed→d weight 1 ⇒ 2
    assert_eq!(map.get(&p("a")), Some(&Value::int(20)));
    assert_eq!(map.get(&p("d")), Some(&Value::int(2)));
}

/// doubling transitive closure — `path[x,z] := path{x,y}, path{y,z}` has TWO
/// occurrences of the same in-stratum relation in one rule body, the classic
/// stress case for semi-naive's "one variant per occurrence" scheme
/// (`.claude/plans/query-optimization.md` P3). hand-verified fixed point for
/// the chain a→b→c→d: {ab,bc,cd,ac,bd,ad} — all 6 forward-reachable pairs.
#[test]
fn semi_naive_handles_multiple_occurrences_of_the_same_recursive_relation() {
    let mut s = LocalSource::new();
    s.add(
        "edge",
        &["from", "to"],
        vec![
            vec![Value::str("a"), Value::str("b")],
            vec![Value::str("b"), Value::str("c")],
            vec![Value::str("c"), Value::str("d")],
        ],
    );
    // `path`'s own columns are named after its head vars ("x", "z") — the
    // recursive rule binds by those names, not `edge`'s ("from", "to").
    let script = "path[x, z] := edge{from: x, to: z}\n\
        path[x, z] := path{x, z: y}, path{x: y, z}\n\
        ?[x, z] := path{x, z}";
    let prog = parse(script).expect("parse");
    let ir = plan(&prog).expect("plan");
    let out = eval(&ir, &s, &Ctx::default()).expect("eval");

    let x = col(&out, "x");
    let z = col(&out, "z");
    let mut pairs: Vec<(String, String)> = out
        .rows
        .iter()
        .map(|r| {
            let sx = if let Value::Bytes(b) = &r[x] { String::from_utf8_lossy(b).into_owned() } else { panic!() };
            let sz = if let Value::Bytes(b) = &r[z] { String::from_utf8_lossy(b).into_owned() } else { panic!() };
            (sx, sz)
        })
        .collect();
    pairs.sort();
    assert_eq!(
        pairs,
        vec![
            ("a".into(), "b".into()),
            ("a".into(), "c".into()),
            ("a".into(), "d".into()),
            ("b".into(), "c".into()),
            ("b".into(), "d".into()),
            ("c".into(), "d".into()),
        ]
    );
}

/// a naive fixed-point re-derives every rule over the FULL accumulated
/// relation each round — for a chain graph, where each round's delta is one
/// new hop, that is O(1+2+..+N) = O(N²) total join work. semi-naive joins
/// only each round's new fact against the (P0-indexed) edge relation, O(N)
/// total. a long chain makes the difference dramatic — this is a regression
/// guard for P3, not a precise timing budget (`.claude/plans/query-optimization.md`).
#[test]
fn semi_naive_scale_long_chain() {
    let depth = 300;
    let mut s = LocalSource::new();
    // `#n0` in the query resolves to a Hash value (see `p()`/`me()` above) —
    // build the chain with Hash identities to match, not `Value::str`.
    let edges: Vec<Tuple> =
        (0..depth).map(|i| vec![p(&format!("n{i}")), p(&format!("n{}", i + 1))]).collect();
    s.add("axons", &["from", "to"], edges);

    // `reachable`'s own column is named `x` (its head var, see the
    // multi-occurrence test above for why the recursive atom binds by that
    // name, not `axons`'s `to`).
    let script = "reachable[x] := axons{from: #n0, to: x}\n\
        reachable[x] := reachable{x: mid}, axons{from: mid, to: x}\n\
        ?[x] := reachable{x}";
    let prog = parse(script).expect("parse");
    let ir = plan(&prog).expect("plan");

    let t0 = std::time::Instant::now();
    let out = eval(&ir, &s, &Ctx::default()).expect("eval");
    let elapsed = t0.elapsed();
    eprintln!("semi-naive {depth}-hop chain closure: {:?}", elapsed);

    assert_eq!(out.rows.len(), depth as usize);
    assert!(elapsed.as_secs() < 5, "chain closure took {:?} — semi-naive may have regressed to naive re-derivation", elapsed);
}

/// mirrors lytics' `passage_ids` `mtc` rule shape exactly (an inequality
/// self-join computing a running count — see
/// `~/cyber/lytics/rs/ingest/src/inf_reports.rs`), the pattern measured at
/// ~485ms for 1600 events before the P0 indexed-join fix
/// (`.claude/plans/query-optimization.md`). 2000 events here, larger than
/// the reported bottleneck. asserts correctness and a generous time ceiling
/// that only trips if the join regresses back to a full O(events ×
/// arrivals) scan — not a precise timing budget.
#[test]
fn scale_inequality_self_join_running_count() {
    let neurons = 50;
    let events_per_neuron = 40;
    let mut s = LocalSource::new();

    let mut ts_ev = Vec::new();
    let mut arrival_ev = Vec::new();
    let mut fs = Vec::new();
    for n in 0..neurons {
        let neuron = Value::str(&format!("n{n}"));
        fs.push(vec![neuron.clone(), Value::int(0)]);
        for i in 0..events_per_neuron {
            let ts = Value::int(i as i64 * 10);
            ts_ev.push(vec![neuron.clone(), ts.clone()]);
            if i % 3 == 0 {
                arrival_ev.push(vec![neuron.clone(), ts]);
            }
        }
    }
    s.add("ts_ev", &["neuron", "ts"], ts_ev);
    s.add("fs", &["neuron", "first"], fs);
    s.add("arrival_ev", &["neuron", "ts"], arrival_ev);

    let script = "mtc[neuron, ts, arrival_ts] := ts_ev{neuron, ts}, fs{neuron, first}, \
        arrival_ev{neuron, ts: arrival_ts}, gt(arrival_ts, first), le(arrival_ts, ts)\n\
        ?[neuron, ts, count(arrival_ts)] := mtc[neuron, ts, arrival_ts]";
    let prog = parse(script).expect("parse");
    let ir = plan(&prog).expect("plan");

    let t0 = std::time::Instant::now();
    let out = eval(&ir, &s, &Ctx::default()).expect("eval");
    let elapsed = t0.elapsed();
    eprintln!(
        "indexed inequality self-join over {} events: {:?}",
        neurons * events_per_neuron,
        elapsed
    );
    assert!(
        elapsed.as_secs() < 2,
        "inequality self-join took {:?} — indexed-join may have regressed",
        elapsed
    );

    // mtc is an inner join, not a left join: an event with zero qualifying
    // arrivals (gt(arrival_ts, first), le(arrival_ts, ts)) produces no row —
    // matches `passage_ids`' documented gap-fill-to-0 semantics, the caller
    // defaults missing keys rather than expecting every event present here.
    // per neuron, events before the first arrival after ts=0 (i.e. ts=0,10,20,
    // since the next arrival is at ts=30) have no qualifying arrival: 3 events
    // × 50 neurons = 150 missing.
    assert_eq!(out.rows.len(), neurons * events_per_neuron - 150);

    // n0 @ ts=50: arrivals strictly after first(=0) and at-or-before 50 are
    // {30} (0 excluded by gt, 60 excluded by le) ⇒ count 1.
    let neuron_col = col(&out, "neuron");
    let ts_col = col(&out, "ts");
    let cnt_col = col(&out, "arrival_ts");
    let row = out
        .rows
        .iter()
        .find(|r| r[neuron_col] == Value::str("n0") && r[ts_col] == Value::int(50))
        .expect("n0 @ ts=50 present");
    assert_eq!(row[cnt_col], Value::int(1));
}
