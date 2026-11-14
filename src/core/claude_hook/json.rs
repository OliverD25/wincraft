//! A JSON value that keeps the order of an object's keys, so editing the
//! user's Claude Code `settings.json` changes only the hook entries.
//! serde_json's own `Value` sorts the keys without its `preserve_order`
//! feature, which WinCraft does not turn on.

use std::fmt;

use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::ser::{Serialize, SerializeMap, SerializeSeq, Serializer};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Object(Vec<(String, Json)>),
    Array(Vec<Json>),
    /// A string, number, true, false or null.
    Leaf(Value),
}

impl Json {
    pub fn object() -> Json {
        Json::Object(Vec::new())
    }

    pub fn text(text: &str) -> Json {
        Json::Leaf(Value::String(text.to_string()))
    }

    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Json> {
        match self {
            Json::Object(entries) => entries.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// The value under `key`, added at the end with `make` when missing.
    pub fn entry(&mut self, key: &str, make: impl FnOnce() -> Json) -> Option<&mut Json> {
        let Json::Object(entries) = self else {
            return None;
        };
        let at = match entries.iter().position(|(k, _)| k == key) {
            Some(at) => at,
            None => {
                entries.push((key.to_string(), make()));
                entries.len() - 1
            }
        };
        Some(&mut entries[at].1)
    }

    pub fn remove(&mut self, key: &str) {
        if let Json::Object(entries) = self {
            entries.retain(|(k, _)| k != key);
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Leaf(Value::String(text)) => Some(text),
            _ => None,
        }
    }
}

impl<'de> Deserialize<'de> for Json {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(JsonVisitor)
    }
}

struct JsonVisitor;

impl<'de> Visitor<'de> for JsonVisitor {
    type Value = Json;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("any JSON value")
    }

    fn visit_bool<E>(self, v: bool) -> Result<Json, E> {
        Ok(Json::Leaf(Value::Bool(v)))
    }

    fn visit_i64<E>(self, v: i64) -> Result<Json, E> {
        Ok(Json::Leaf(Value::from(v)))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Json, E> {
        Ok(Json::Leaf(Value::from(v)))
    }

    fn visit_f64<E>(self, v: f64) -> Result<Json, E> {
        Ok(Json::Leaf(Value::from(v)))
    }

    fn visit_str<E>(self, v: &str) -> Result<Json, E> {
        Ok(Json::text(v))
    }

    fn visit_string<E>(self, v: String) -> Result<Json, E> {
        Ok(Json::Leaf(Value::String(v)))
    }

    fn visit_unit<E>(self) -> Result<Json, E> {
        Ok(Json::Leaf(Value::Null))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Json, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = seq.next_element()? {
            items.push(item);
        }
        Ok(Json::Array(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Json, A::Error> {
        let mut entries = Vec::new();
        while let Some((key, value)) = map.next_entry::<String, Json>()? {
            entries.push((key, value));
        }
        Ok(Json::Object(entries))
    }
}

impl Serialize for Json {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Json::Leaf(value) => value.serialize(serializer),
            Json::Array(items) => {
                let mut seq = serializer.serialize_seq(Some(items.len()))?;
                for item in items {
                    seq.serialize_element(item)?;
                }
                seq.end()
            }
            Json::Object(entries) => {
                let mut map = serializer.serialize_map(Some(entries.len()))?;
                for (key, value) in entries {
                    map.serialize_entry(key, value)?;
                }
                map.end()
            }
        }
    }
}
