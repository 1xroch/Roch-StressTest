// Roch StressTest -- one window for every stress test on the machine.
// Copyright (C) 2026 Roch Studio
// SPDX-License-Identifier: GPL-3.0-or-later

//! A tool's settings: a map from field key to value.
//!
//! JSON values rather than a struct per tool, because the window treats every
//! tool alike -- it draws a row per field and hands the values back -- and
//! Linpack Extended's chain of tests travels in the same map as a list.

use serde_json::{Map, Value};

pub type Config = Map<String, Value>;

/// Builds a config from `(key, value)` pairs.
pub fn config<const N: usize>(pairs: [(&str, Value); N]) -> Config {
    pairs.into_iter().map(|(key, value)| (key.to_string(), value)).collect()
}

/// A whole number from a value that may be a number, a bool or digits in a string.
pub fn as_int(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number.as_i64().or_else(|| number.as_f64().map(|f| f as i64)),
        Value::Bool(flag) => Some(i64::from(*flag)),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

pub trait ConfigExt {
    fn int(&self, key: &str) -> i64;
    fn int_or(&self, key: &str, default: i64) -> i64;
    fn flag(&self, key: &str) -> bool;
    fn flag_or(&self, key: &str, default: bool) -> bool;
    fn text(&self, key: &str) -> String;
    /// A copy with `key` set to `value`.
    fn with(&self, key: &str, value: Value) -> Config;
}

impl ConfigExt for Config {
    fn int(&self, key: &str) -> i64 {
        self.int_or(key, 0)
    }

    fn int_or(&self, key: &str, default: i64) -> i64 {
        self.get(key).and_then(as_int).unwrap_or(default)
    }

    fn flag(&self, key: &str) -> bool {
        self.flag_or(key, false)
    }

    fn flag_or(&self, key: &str, default: bool) -> bool {
        match self.get(key) {
            Some(Value::Bool(flag)) => *flag,
            Some(Value::Number(number)) => number.as_i64().is_some_and(|n| n != 0),
            Some(Value::String(text)) => matches!(text.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"),
            Some(Value::Null) | None => default,
            Some(_) => default,
        }
    }

    fn text(&self, key: &str) -> String {
        match self.get(key) {
            Some(Value::String(text)) => text.clone(),
            Some(Value::Null) | None => String::new(),
            Some(other) => other.to_string(),
        }
    }

    fn with(&self, key: &str, value: Value) -> Config {
        let mut copy = self.clone();
        copy.insert(key.to_string(), value);
        copy
    }
}
