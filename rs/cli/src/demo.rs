//! A built-in demo cybergraph for `inf run`/`inf cost` during the bootstrap,
//! before a real bbg/cybergraph source is wired in. A small chain seed→a→b→c
//! with a branch seed→d, plus focus, karma, and a few @me cyberlinks.

use inf_source::LocalSource;
use inf_value::{tag_hash, Value};

fn p(name: &str) -> Value {
    Value::Hash(tag_hash(name))
}
fn me() -> Value {
    Value::Hash(tag_hash("@me"))
}

pub fn graph() -> LocalSource {
    let mut s = LocalSource::new().with_snapshot(1);
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
