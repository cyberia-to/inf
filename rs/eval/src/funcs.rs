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
        "is_json" => {
            need(1)?;
            let ok = serde_json::from_str::<serde_json::Value>(&as_str(&a[0])?).is_ok();
            Value::Bool(ok)
        }
        "json_get" => {
            need(2)?;
            let j: serde_json::Value =
                serde_json::from_str(&as_str(&a[0])?).map_err(|e| format!("json_get: {e}"))?;
            let key = as_str(&a[1])?;
            let v = j.get(&key).cloned().unwrap_or(serde_json::Value::Null);
            json_to_value(&v)
        }
        other => return Err(format!("unknown function `{other}`")),
    })
}

fn compile(pat: &str) -> Result<regex::Regex, String> {
    regex::Regex::new(pat).map_err(|e| format!("bad regex: {e}"))
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
            extra("is_json", &[Value::str("not json{")]).unwrap(),
            Value::Bool(false)
        );
    }
}
