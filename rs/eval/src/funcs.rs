//! Extended builtins kept out of the core dispatch to respect the file-size
//! limit: regex over particle content (via the `regex` crate) and JSON field
//! access (via `serde_json`). Both operate on the `Bytes` value (particle
//! content) and are deterministic.

use crate::expr::as_str;
use inf_value::Value;

pub fn extra(func: &str, a: &[Value]) -> Result<Value, String> {
    let need = |n: usize| -> Result<(), String> {
        if a.len() == n {
            Ok(())
        } else {
            Err(format!("`{func}` expects {n} args, got {}", a.len()))
        }
    };
    Ok(match func {
        "regex_matches" => {
            need(2)?;
            let re = compile(&as_str(&a[1])?)?;
            Value::Bool(re.is_match(&as_str(&a[0])?))
        }
        "regex_replace" => {
            need(3)?;
            let re = compile(&as_str(&a[1])?)?;
            Value::str(&re.replace(&as_str(&a[0])?, as_str(&a[2])?.as_str()))
        }
        "regex_extract" => {
            need(2)?;
            let re = compile(&as_str(&a[1])?)?;
            let s = as_str(&a[0])?;
            Value::List(re.find_iter(&s).map(|m| Value::str(m.as_str())).collect())
        }
        // is_json: check if a string is valid JSON
        "is_json" => {
            need(1)?;
            let ok = serde_json::from_str::<serde_json::Value>(&as_str(&a[0])?).is_ok();
            Value::Bool(ok)
        }
        // json_get / get: read a field by key name (specs/functions.md json section)
        "json_get" | "get" => {
            need(2)?;
            let j: serde_json::Value =
                serde_json::from_str(&as_str(&a[0])?).map_err(|e| format!("json_get: {e}"))?;
            let key = as_str(&a[1])?;
            let v = j.get(&key).cloned().unwrap_or(serde_json::Value::Null);
            json_to_value(&v)
        }
        // parse_json: decode a JSON string into a Value (alias for json_get without a key)
        "parse_json" => {
            need(1)?;
            let j: serde_json::Value =
                serde_json::from_str(&as_str(&a[0])?).map_err(|e| format!("parse_json: {e}"))?;
            json_to_value(&j)
        }
        // dump_json: encode a Value back to a JSON string
        "dump_json" => {
            need(1)?;
            let s = match &a[0] {
                Value::Null => "null".to_string(),
                Value::Bool(b) => b.to_string(),
                Value::Int(i) => i.to_string(),
                Value::Bytes(b) => {
                    let s = std::str::from_utf8(b).unwrap_or("?");
                    serde_json::to_string(s).unwrap_or_else(|_| "null".to_string())
                }
                Value::List(items) => {
                    let parts: Vec<String> = items.iter().map(value_to_json).collect();
                    format!("[{}]", parts.join(","))
                }
                other => value_to_json(other),
            };
            Value::str(&s)
        }
        other => return Err(format!("unknown function `{other}`")),
    })
}

fn compile(pat: &str) -> Result<regex::Regex, String> {
    regex::Regex::new(pat).map_err(|e| format!("bad regex: {e}"))
}

fn value_to_json(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Field(f) => f.val().to_string(),
        Value::Word(w) => w.to_string(),
        Value::Bytes(b) => {
            let s = std::str::from_utf8(b).unwrap_or("?");
            serde_json::to_string(s).unwrap_or_else(|_| "null".to_string())
        }
        Value::Hash(h) => {
            let hex: String = h.iter().map(|b| format!("{b:02x}")).collect();
            format!("\"{hex}\"")
        }
        Value::List(items) => {
            let parts: Vec<String> = items.iter().map(value_to_json).collect();
            format!("[{}]", parts.join(","))
        }
    }
}

/// Map a JSON scalar to a value; non-scalars round-trip back to their JSON text.
fn json_to_value(v: &serde_json::Value) -> Value {
    match v {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Int(i)
            } else {
                Value::str(&n.to_string()) // non-integer numbers stay textual (no float)
            }
        }
        serde_json::Value::String(s) => Value::str(s),
        other => Value::str(&other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regex_and_json() {
        assert_eq!(
            extra("regex_matches", &[Value::str("hello42"), Value::str(r"\d+")]).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            extra("regex_extract", &[Value::str("a1 b2"), Value::str(r"\d")]).unwrap(),
            Value::List(vec![Value::str("1"), Value::str("2")])
        );
        assert_eq!(
            extra("json_get", &[Value::str(r#"{"k": 7}"#), Value::str("k")]).unwrap(),
            Value::Int(7)
        );
        assert_eq!(
            extra("get", &[Value::str(r#"{"x": 42}"#), Value::str("x")]).unwrap(),
            Value::Int(42),
            "get is alias for json_get"
        );
        assert_eq!(
            extra("is_json", &[Value::str("not json{")]).unwrap(),
            Value::Bool(false)
        );
        assert_eq!(
            extra("parse_json", &[Value::str(r#"{"a": 1}"#)]).unwrap(),
            Value::str(r#"{"a":1}"#),
            "parse_json on object round-trips through text"
        );
        assert_eq!(
            extra("dump_json", &[Value::Int(42)]).unwrap(),
            Value::str("42")
        );
        assert_eq!(
            extra("dump_json", &[Value::Bool(true)]).unwrap(),
            Value::str("true")
        );
    }
}
