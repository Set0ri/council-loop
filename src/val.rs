//! Reading JSON documents the forgiving way the run files need: a missing key is an empty value,
//! never a panic. `run.json`, ballots and seat outputs are written by models and by two
//! implementations, so every read states its fallback.

use serde_json::{Map, Value};

static NULL: Value = Value::Null;
static NO_ITEMS: Vec<Value> = Vec::new();

/// Accessors with Python's `dict.get` manners.
pub trait Get {
    /// `d.get(key)`: the value, or `null`.
    fn at(&self, key: &str) -> &Value;
    /// The string at `key`, or `""` when it is missing or not a string.
    fn s(&self, key: &str) -> &str;
    /// The list at `key`, or an empty list.
    fn list(&self, key: &str) -> &Vec<Value>;
    /// The object at `key`, when it is one.
    fn map(&self, key: &str) -> Option<&Map<String, Value>>;
    /// The whole number at `key`, when it is one (a boolean is not a number).
    fn int(&self, key: &str) -> Option<i64>;
    /// The number at `key` as a float, when it is one.
    fn num(&self, key: &str) -> Option<f64>;
    /// `bool(d.get(key))`.
    fn truthy(&self, key: &str) -> bool;
    /// `str(d.get(key))`: `None` for a missing key, a string shown bare.
    fn show(&self, key: &str) -> String;
    /// Is the key present at all (`key in d`)?
    fn has(&self, key: &str) -> bool;
}

impl Get for Value {
    fn at(&self, key: &str) -> &Value {
        self.get(key).unwrap_or(&NULL)
    }
    fn s(&self, key: &str) -> &str {
        self.get(key).and_then(Value::as_str).unwrap_or("")
    }
    fn list(&self, key: &str) -> &Vec<Value> {
        self.get(key).and_then(Value::as_array).unwrap_or(&NO_ITEMS)
    }
    fn map(&self, key: &str) -> Option<&Map<String, Value>> {
        self.get(key).and_then(Value::as_object)
    }
    fn int(&self, key: &str) -> Option<i64> {
        self.get(key).and_then(as_int)
    }
    fn num(&self, key: &str) -> Option<f64> {
        self.get(key).and_then(Value::as_f64)
    }
    fn truthy(&self, key: &str) -> bool {
        crate::py::truthy(self.at(key))
    }
    fn show(&self, key: &str) -> String {
        crate::py::to_str(self.at(key))
    }
    fn has(&self, key: &str) -> bool {
        self.get(key).is_some()
    }
}

/// A whole number (Python's `isinstance(x, int) and not isinstance(x, bool)`).
pub fn as_int(value: &Value) -> Option<i64> {
    match value {
        Value::Number(n) if n.is_i64() || n.is_u64() => n.as_i64(),
        _ => None,
    }
}

/// The entries of an object, or nothing when the value is not one.
pub fn items(value: &Value) -> impl Iterator<Item = (&String, &Value)> {
    value.as_object().into_iter().flat_map(|m| m.iter())
}

/// `{**base, **extra}`: keys of `base` keep their place, new keys follow in order.
pub fn merged(base: &Value, extra: &Value) -> Map<String, Value> {
    let mut out = base.as_object().cloned().unwrap_or_default();
    for (k, v) in items(extra) {
        out.insert(k.clone(), v.clone());
    }
    out
}

/// A list of strings from a JSON list (other items are shown as Python would print them).
pub fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|a| a.iter().map(crate::py::to_str).collect())
        .unwrap_or_default()
}
