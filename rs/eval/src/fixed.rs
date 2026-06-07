//! Fixed-rule graph algorithms (specs/algorithms.md). All run bounded and exact:
//! degree, connected components, BFS/DFS distance, shortest paths, topological
//! sort, and a fixed-point integer PageRank (rank scaled by `PR_SCALE`, no
//! float). The remaining catalog entries return a clear "not implemented" error.

use crate::expr::{eval_term, Ctx};
use inf_ast::{Fixed, Head};
use inf_value::{Tuple, Value};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

const PR_SCALE: i64 = 1_000_000;

/// Edges as (from, to, weight). The edges relation is read positionally:
/// column 0 = from, 1 = to, optional 2 = integer weight (default 1).
struct Graph {
    nodes: Vec<Value>,
    out: BTreeMap<Value, Vec<(Value, i64)>>,
    inc: BTreeMap<Value, Vec<Value>>,
}

impl Graph {
    fn build(edges: &[Tuple]) -> Result<Graph, String> {
        let mut node_set = BTreeSet::new();
        let mut out: BTreeMap<Value, Vec<(Value, i64)>> = BTreeMap::new();
        let mut inc: BTreeMap<Value, Vec<Value>> = BTreeMap::new();
        for e in edges {
            if e.len() < 2 {
                return Err("edges relation needs at least (from, to) columns".into());
            }
            let (f, t) = (e[0].clone(), e[1].clone());
            let w = match e.get(2) {
                Some(Value::Int(i)) => *i,
                Some(_) => return Err("edge weight must be an integer".into()),
                None => 1,
            };
            node_set.insert(f.clone());
            node_set.insert(t.clone());
            out.entry(f.clone()).or_default().push((t.clone(), w));
            inc.entry(t).or_default().push(f);
        }
        Ok(Graph { nodes: node_set.into_iter().collect(), out, inc })
    }
    fn neighbors(&self, v: &Value) -> &[(Value, i64)] {
        self.out.get(v).map(|x| x.as_slice()).unwrap_or(&[])
    }
}

pub fn run(f: &Fixed, head: &Head, edges: &[Tuple], ctx: &Ctx) -> Result<Vec<Tuple>, String> {
    let g = Graph::build(edges)?;
    let param = |k: &str| -> Option<i64> {
        f.params
            .iter()
            .find(|(n, _)| n == k)
            .and_then(|(_, t)| eval_term(t, &Default::default(), ctx).ok())
            .and_then(|v| v.as_int())
    };
    let pos = |i: usize| -> Result<Value, String> {
        let t = f.pos.get(i).ok_or_else(|| format!("`{}` missing positional argument {i}", f.algo))?;
        eval_term(t, &Default::default(), ctx)
    };
    let arity = head.args.len();

    match f.algo.as_str() {
        "DegreeCentrality" => degree_centrality(&g),
        "ConnectedComponents" => connected_components(&g),
        "BreadthFirstSearch" => bfs_depth(&g, &pos(0)?),
        "ShortestPathBFS" => shortest_path_bfs(&g, &pos(0)?, &pos(1)?),
        "ShortestPathDijkstra" => shortest_path_dijkstra(&g, &pos(0)?, &pos(1)?),
        "TopSort" => top_sort(&g),
        "PageRank" => page_rank(&g, param("theta").unwrap_or(85), param("iters").unwrap_or(20)),
        other => Err(format!(
            "fixed rule `{other}` is not implemented in the reference engine (arity {arity})"
        )),
    }
}

fn degree_centrality(g: &Graph) -> Result<Vec<Tuple>, String> {
    let mut out = Vec::new();
    for n in &g.nodes {
        let o = g.out.get(n).map(|v| v.len()).unwrap_or(0) as i64;
        let i = g.inc.get(n).map(|v| v.len()).unwrap_or(0) as i64;
        out.push(vec![n.clone(), Value::Int(o + i), Value::Int(i), Value::Int(o)]);
    }
    Ok(out)
}

fn connected_components(g: &Graph) -> Result<Vec<Tuple>, String> {
    // weakly connected components via union-find over undirected edges
    let idx: BTreeMap<&Value, usize> = g.nodes.iter().enumerate().map(|(i, v)| (v, i)).collect();
    let mut parent: Vec<usize> = (0..g.nodes.len()).collect();
    fn find(parent: &mut Vec<usize>, x: usize) -> usize {
        let mut r = x;
        while parent[r] != r {
            r = parent[r];
        }
        let mut c = x;
        while parent[c] != r {
            let n = parent[c];
            parent[c] = r;
            c = n;
        }
        r
    }
    for (f, tos) in &g.out {
        for (t, _) in tos {
            let a = find(&mut parent, idx[f]);
            let b = find(&mut parent, idx[t]);
            if a != b {
                parent[a] = b;
            }
        }
    }
    let mut out = Vec::new();
    for (i, n) in g.nodes.iter().enumerate() {
        let c = find(&mut parent, i);
        out.push(vec![n.clone(), Value::Int(c as i64)]);
    }
    Ok(out)
}

fn bfs_depth(g: &Graph, src: &Value) -> Result<Vec<Tuple>, String> {
    let mut depth: BTreeMap<Value, i64> = BTreeMap::new();
    let mut q = VecDeque::new();
    depth.insert(src.clone(), 0);
    q.push_back(src.clone());
    while let Some(u) = q.pop_front() {
        let d = depth[&u];
        for (v, _) in g.neighbors(&u) {
            if !depth.contains_key(v) {
                depth.insert(v.clone(), d + 1);
                q.push_back(v.clone());
            }
        }
    }
    Ok(depth.into_iter().map(|(n, d)| vec![n, Value::Int(d)]).collect())
}

fn reconstruct(prev: &BTreeMap<Value, Value>, a: &Value, b: &Value) -> Value {
    let mut path = vec![b.clone()];
    let mut cur = b.clone();
    while &cur != a {
        match prev.get(&cur) {
            Some(p) => {
                path.push(p.clone());
                cur = p.clone();
            }
            None => break,
        }
    }
    path.reverse();
    Value::List(path)
}

fn shortest_path_bfs(g: &Graph, a: &Value, b: &Value) -> Result<Vec<Tuple>, String> {
    let mut prev: BTreeMap<Value, Value> = BTreeMap::new();
    let mut seen: BTreeSet<Value> = BTreeSet::new();
    let mut q = VecDeque::new();
    seen.insert(a.clone());
    q.push_back(a.clone());
    while let Some(u) = q.pop_front() {
        if &u == b {
            return Ok(vec![vec![reconstruct(&prev, a, b)]]);
        }
        for (v, _) in g.neighbors(&u) {
            if seen.insert(v.clone()) {
                prev.insert(v.clone(), u.clone());
                q.push_back(v.clone());
            }
        }
    }
    Ok(vec![]) // unreachable
}

fn shortest_path_dijkstra(g: &Graph, a: &Value, b: &Value) -> Result<Vec<Tuple>, String> {
    let mut dist: BTreeMap<Value, i64> = BTreeMap::new();
    let mut prev: BTreeMap<Value, Value> = BTreeMap::new();
    let mut done: BTreeSet<Value> = BTreeSet::new();
    dist.insert(a.clone(), 0);
    loop {
        // pick the unfinished node with smallest distance (deterministic by value)
        let next = dist
            .iter()
            .filter(|(n, _)| !done.contains(*n))
            .min_by_key(|(n, d)| (**d, (*n).clone()))
            .map(|(n, d)| (n.clone(), *d));
        let (u, du) = match next {
            Some(x) => x,
            None => break,
        };
        done.insert(u.clone());
        if &u == b {
            break;
        }
        for (v, w) in g.neighbors(&u) {
            if *w < 0 {
                return Err("Dijkstra requires non-negative weights".into());
            }
            let nd = du + w;
            if dist.get(v).map_or(true, |&old| nd < old) {
                dist.insert(v.clone(), nd);
                prev.insert(v.clone(), u.clone());
            }
        }
    }
    match dist.get(b) {
        Some(&cost) => Ok(vec![vec![reconstruct(&prev, a, b), Value::Int(cost)]]),
        None => Ok(vec![]),
    }
}

fn top_sort(g: &Graph) -> Result<Vec<Tuple>, String> {
    let mut indeg: BTreeMap<Value, i64> = g.nodes.iter().map(|n| (n.clone(), 0)).collect();
    for tos in g.out.values() {
        for (t, _) in tos {
            *indeg.get_mut(t).unwrap() += 1;
        }
    }
    let mut q: BTreeSet<Value> =
        indeg.iter().filter(|(_, d)| **d == 0).map(|(n, _)| n.clone()).collect();
    let mut out = Vec::new();
    let mut order = 0i64;
    while let Some(u) = q.iter().next().cloned() {
        q.remove(&u);
        out.push(vec![u.clone(), Value::Int(order)]);
        order += 1;
        for (v, _) in g.neighbors(&u) {
            let d = indeg.get_mut(v).unwrap();
            *d -= 1;
            if *d == 0 {
                q.insert(v.clone());
            }
        }
    }
    if out.len() != g.nodes.len() {
        return Err("TopSort: graph has a cycle".into());
    }
    Ok(out)
}

fn page_rank(g: &Graph, theta_pct: i64, iters: i64) -> Result<Vec<Tuple>, String> {
    let n = g.nodes.len() as i64;
    if n == 0 {
        return Ok(vec![]);
    }
    let (num, den) = (theta_pct, 100i64);
    let mut rank: BTreeMap<Value, i64> = g.nodes.iter().map(|v| (v.clone(), PR_SCALE / n)).collect();
    for _ in 0..iters.max(0) {
        let dangling: i64 = g
            .nodes
            .iter()
            .filter(|v| g.out.get(*v).map_or(true, |o| o.is_empty()))
            .map(|v| rank[v])
            .sum();
        let base = ((den - num) * PR_SCALE / den) / n + (num * dangling / den) / n;
        let mut next: BTreeMap<Value, i64> = g.nodes.iter().map(|v| (v.clone(), base)).collect();
        for (u, tos) in &g.out {
            if tos.is_empty() {
                continue;
            }
            let share = (num * rank[u] / den) / tos.len() as i64;
            for (v, _) in tos {
                *next.get_mut(v).unwrap() += share;
            }
        }
        rank = next;
    }
    Ok(rank.into_iter().map(|(v, r)| vec![v, Value::Int(r)]).collect())
}
