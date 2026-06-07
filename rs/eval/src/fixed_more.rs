//! The remaining fixed-rule algorithms (specs/algorithms.md), sharing
//! `fixed::Graph`. Exact where integers suffice (MST, Yen, label propagation,
//! random walk with a committed seed); fixed-point scaled (×`SCALE`, no float)
//! for the fractional centralities and clustering coefficient.

use crate::fixed::{reconstruct, Graph, PR_SCALE_PUB as SCALE};
use inf_value::{Tuple, Value};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub fn run(
    algo: &str,
    g: &Graph,
    params: &[(String, i64)],
    pos: &[Value],
) -> Result<Vec<Tuple>, String> {
    let p = |k: &str, d: i64| params.iter().find(|(n, _)| n == k).map(|(_, v)| *v).unwrap_or(d);
    let at = |i: usize| pos.get(i).cloned().ok_or_else(|| format!("`{algo}` missing argument {i}"));
    match algo {
        "ClosenessCentrality" => Ok(closeness(g)),
        "BetweennessCentrality" => Ok(betweenness(g)),
        "LabelPropagation" => Ok(label_propagation(g, p("iters", 10))),
        "ClusteringCoefficients" => Ok(clustering(g)),
        "CommunityDetectionLouvain" => Ok(louvain(g)),
        "MinimumSpanningForestKruskal" | "MinimumSpanningTreePrim" => Ok(kruskal(g)),
        "ShortestPathAStar" => Ok(match dijkstra(g, &at(0)?, &at(1)?, &BTreeSet::new(), &BTreeSet::new()) {
            Some((path, cost)) => vec![vec![Value::List(path), Value::Int(cost)]],
            None => vec![],
        }),
        "KShortestPathYen" => Ok(yen(g, &at(0)?, &at(1)?, p("k", 3).max(1) as usize)),
        "RandomWalk" => Ok(random_walk(g, &at(0)?, p("steps", 10), p("times", 1), p("seed", 1) as u64)),
        other => Err(format!("fixed rule `{other}` is not implemented in the reference engine")),
    }
}

fn bfs_dist(g: &Graph, src: &Value) -> BTreeMap<Value, i64> {
    let mut dist = BTreeMap::new();
    let mut q = VecDeque::new();
    dist.insert(src.clone(), 0);
    q.push_back(src.clone());
    while let Some(u) = q.pop_front() {
        let d = dist[&u];
        for (v, _) in g.neighbors(&u) {
            if !dist.contains_key(v) {
                dist.insert(v.clone(), d + 1);
                q.push_back(v.clone());
            }
        }
    }
    dist
}

fn closeness(g: &Graph) -> Vec<Tuple> {
    g.nodes
        .iter()
        .map(|s| {
            let dist = bfs_dist(g, s);
            let sum: i64 = dist.values().sum();
            let reach = (dist.len() as i64) - 1;
            let c = if sum > 0 { SCALE * reach / sum } else { 0 };
            vec![s.clone(), Value::Int(c)]
        })
        .collect()
}

/// Brandes betweenness (directed, unweighted), with fixed-point scaled
/// dependency accumulation.
fn betweenness(g: &Graph) -> Vec<Tuple> {
    let mut betw: BTreeMap<Value, i64> = g.nodes.iter().map(|v| (v.clone(), 0)).collect();
    for s in &g.nodes {
        let mut stack = Vec::new();
        let mut pred: BTreeMap<Value, Vec<Value>> = BTreeMap::new();
        let mut sigma: BTreeMap<Value, i64> = g.nodes.iter().map(|v| (v.clone(), 0)).collect();
        let mut dist: BTreeMap<Value, i64> = g.nodes.iter().map(|v| (v.clone(), -1)).collect();
        sigma.insert(s.clone(), 1);
        dist.insert(s.clone(), 0);
        let mut q = VecDeque::new();
        q.push_back(s.clone());
        while let Some(v) = q.pop_front() {
            stack.push(v.clone());
            for (w, _) in g.neighbors(&v) {
                if dist[w] < 0 {
                    dist.insert(w.clone(), dist[&v] + 1);
                    q.push_back(w.clone());
                }
                if dist[w] == dist[&v] + 1 {
                    *sigma.get_mut(w).unwrap() += sigma[&v];
                    pred.entry(w.clone()).or_default().push(v.clone());
                }
            }
        }
        let mut delta: BTreeMap<Value, i64> = g.nodes.iter().map(|v| (v.clone(), 0)).collect();
        while let Some(w) = stack.pop() {
            if let Some(ps) = pred.get(&w) {
                for v in ps {
                    if sigma[&w] != 0 {
                        let add = sigma[v] * (SCALE + delta[&w]) / sigma[&w];
                        *delta.get_mut(v).unwrap() += add;
                    }
                }
            }
            if &w != s {
                *betw.get_mut(&w).unwrap() += delta[&w];
            }
        }
    }
    betw.into_iter().map(|(v, b)| vec![v, Value::Int(b)]).collect()
}

fn undirected_neighbors(g: &Graph, v: &Value) -> BTreeSet<Value> {
    let mut s: BTreeSet<Value> = g.neighbors(v).iter().map(|(t, _)| t.clone()).collect();
    if let Some(srcs) = g.inc.get(v) {
        s.extend(srcs.iter().cloned());
    }
    s.remove(v);
    s
}

fn label_propagation(g: &Graph, iters: i64) -> Vec<Tuple> {
    let idx: BTreeMap<&Value, i64> = g.nodes.iter().enumerate().map(|(i, v)| (v, i as i64)).collect();
    let mut label: BTreeMap<Value, i64> = g.nodes.iter().map(|v| (v.clone(), idx[v])).collect();
    for _ in 0..iters.max(0) {
        for v in &g.nodes {
            let mut tally: BTreeMap<i64, i64> = BTreeMap::new();
            for n in undirected_neighbors(g, v) {
                *tally.entry(label[&n]).or_default() += 1;
            }
            // most frequent, ties broken by smallest label (deterministic)
            if let Some((&best, _)) = tally.iter().max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0))) {
                label.insert(v.clone(), best);
            }
        }
    }
    label.into_iter().map(|(v, l)| vec![v, Value::Int(l)]).collect()
}

fn clustering(g: &Graph) -> Vec<Tuple> {
    let connected = |a: &Value, b: &Value| {
        g.neighbors(a).iter().any(|(t, _)| t == b) || g.neighbors(b).iter().any(|(t, _)| t == a)
    };
    g.nodes
        .iter()
        .map(|v| {
            let neigh: Vec<Value> = undirected_neighbors(g, v).into_iter().collect();
            let deg = neigh.len() as i64;
            let coeff = if deg < 2 {
                0
            } else {
                let mut links = 0i64;
                for i in 0..neigh.len() {
                    for j in (i + 1)..neigh.len() {
                        if connected(&neigh[i], &neigh[j]) {
                            links += 1;
                        }
                    }
                }
                SCALE * 2 * links / (deg * (deg - 1))
            };
            vec![v.clone(), Value::Int(coeff)]
        })
        .collect()
}

/// Single-level greedy modularity (each node joins the neighbor community where
/// it has the most edges). A simplification of full multi-level Louvain.
fn louvain(g: &Graph) -> Vec<Tuple> {
    let idx: BTreeMap<&Value, i64> = g.nodes.iter().enumerate().map(|(i, v)| (v, i as i64)).collect();
    let mut comm: BTreeMap<Value, i64> = g.nodes.iter().map(|v| (v.clone(), idx[v])).collect();
    for _ in 0..g.nodes.len().max(1) {
        let mut moved = false;
        for v in &g.nodes {
            let mut tally: BTreeMap<i64, i64> = BTreeMap::new();
            for n in undirected_neighbors(g, v) {
                *tally.entry(comm[&n]).or_default() += 1;
            }
            if let Some((&best, _)) = tally.iter().max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0))) {
                if comm[v] != best {
                    comm.insert(v.clone(), best);
                    moved = true;
                }
            }
        }
        if !moved {
            break;
        }
    }
    comm.into_iter().map(|(v, c)| vec![v, Value::Int(c)]).collect()
}

fn kruskal(g: &Graph) -> Vec<Tuple> {
    // undirected: canonicalize each edge, keep the minimum weight per pair
    let mut edges: BTreeMap<(Value, Value), i64> = BTreeMap::new();
    for (f, tos) in &g.out {
        for (t, w) in tos {
            let key = if f <= t { (f.clone(), t.clone()) } else { (t.clone(), f.clone()) };
            let e = edges.entry(key).or_insert(*w);
            if *w < *e {
                *e = *w;
            }
        }
    }
    let mut sorted: Vec<((Value, Value), i64)> = edges.into_iter().collect();
    sorted.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
    let idx: BTreeMap<&Value, usize> = g.nodes.iter().enumerate().map(|(i, v)| (v, i)).collect();
    let mut parent: Vec<usize> = (0..g.nodes.len()).collect();
    fn find(p: &mut Vec<usize>, x: usize) -> usize {
        let mut r = x;
        while p[r] != r {
            r = p[r];
        }
        let mut c = x;
        while p[c] != r {
            let n = p[c];
            p[c] = r;
            c = n;
        }
        r
    }
    let mut out = Vec::new();
    for ((f, t), w) in sorted {
        let (a, b) = (find(&mut parent, idx[&f]), find(&mut parent, idx[&t]));
        if a != b {
            parent[a] = b;
            out.push(vec![f, t, Value::Int(w)]);
        }
    }
    out
}

fn dijkstra(
    g: &Graph,
    a: &Value,
    b: &Value,
    blocked_edges: &BTreeSet<(Value, Value)>,
    blocked_nodes: &BTreeSet<Value>,
) -> Option<(Vec<Value>, i64)> {
    let mut dist: BTreeMap<Value, i64> = BTreeMap::new();
    let mut prev: BTreeMap<Value, Value> = BTreeMap::new();
    let mut done: BTreeSet<Value> = BTreeSet::new();
    dist.insert(a.clone(), 0);
    loop {
        let next = dist
            .iter()
            .filter(|(n, _)| !done.contains(*n))
            .min_by_key(|(n, d)| (**d, (*n).clone()))
            .map(|(n, d)| (n.clone(), *d));
        let (u, du) = next?;
        done.insert(u.clone());
        if &u == b {
            break;
        }
        for (v, w) in g.neighbors(&u) {
            if blocked_nodes.contains(v) || blocked_edges.contains(&(u.clone(), v.clone())) {
                continue;
            }
            let nd = du + w;
            if dist.get(v).map_or(true, |&old| nd < old) {
                dist.insert(v.clone(), nd);
                prev.insert(v.clone(), u.clone());
            }
        }
    }
    let cost = *dist.get(b)?;
    match reconstruct(&prev, a, b) {
        Value::List(path) => Some((path, cost)),
        _ => None,
    }
}

/// Yen's k loopless shortest paths.
fn yen(g: &Graph, a: &Value, b: &Value, k: usize) -> Vec<Tuple> {
    let mut accepted: Vec<(Vec<Value>, i64)> = Vec::new();
    let first = match dijkstra(g, a, b, &BTreeSet::new(), &BTreeSet::new()) {
        Some(x) => x,
        None => return vec![],
    };
    accepted.push(first);
    let mut candidates: BTreeSet<(i64, Vec<Value>)> = BTreeSet::new();
    while accepted.len() < k {
        let prev = accepted.last().unwrap().0.clone();
        for i in 0..prev.len().saturating_sub(1) {
            let spur = prev[i].clone();
            let root = &prev[..=i];
            let mut blocked_edges = BTreeSet::new();
            for (p, _) in &accepted {
                if p.len() > i && &p[..=i] == root {
                    blocked_edges.insert((p[i].clone(), p[i + 1].clone()));
                }
            }
            let blocked_nodes: BTreeSet<Value> = root[..i].iter().cloned().collect();
            if let Some((spur_path, _)) = dijkstra(g, &spur, b, &blocked_edges, &blocked_nodes) {
                let mut total = root[..i].to_vec();
                total.extend(spur_path);
                let cost = path_cost(g, &total);
                if let Some(c) = cost {
                    candidates.insert((c, total));
                }
            }
        }
        match candidates.iter().next().cloned() {
            Some((c, path)) => {
                candidates.remove(&(c, path.clone()));
                if !accepted.iter().any(|(p, _)| p == &path) {
                    accepted.push((path, c));
                }
            }
            None => break,
        }
    }
    accepted.into_iter().map(|(p, c)| vec![Value::List(p), Value::Int(c)]).collect()
}

fn path_cost(g: &Graph, path: &[Value]) -> Option<i64> {
    let mut total = 0i64;
    for w in path.windows(2) {
        let edge = g.neighbors(&w[0]).iter().find(|(t, _)| *t == w[1]).map(|(_, c)| *c)?;
        total += edge;
    }
    Some(total)
}

fn random_walk(g: &Graph, start: &Value, steps: i64, times: i64, seed: u64) -> Vec<Tuple> {
    let mut visits: BTreeMap<Value, i64> = BTreeMap::new();
    let mut state = seed.wrapping_add(0x9E3779B97F4A7C15);
    let mut next = || {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        state >> 33
    };
    for _ in 0..times.max(0) {
        let mut cur = start.clone();
        for _ in 0..steps.max(0) {
            let neigh = g.neighbors(&cur);
            if neigh.is_empty() {
                break;
            }
            let idx = (next() as usize) % neigh.len();
            cur = neigh[idx].0.clone();
            *visits.entry(cur.clone()).or_default() += 1;
        }
    }
    visits.into_iter().map(|(v, c)| vec![v, Value::Int(c)]).collect()
}
