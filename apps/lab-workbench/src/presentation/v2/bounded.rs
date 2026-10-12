//! Bounded, duplicate-preserving preflight followed by typed decoding.

use std::{collections::BTreeSet, fmt};

use serde::{
    Deserialize, Deserializer,
    de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor},
};

use super::limits::{
    JSON_BODY_BYTES, JSON_DEPTH as DEPTH, JSON_STRING_BYTES as STRING_BYTES, JSON_VALUES as VALUES,
};
pub(super) const INPUT_BYTES: usize = JSON_BODY_BYTES;

/// Measured lexical usage, with the same counting rules as the current endpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct JsonUsage {
    pub(crate) body_bytes: usize,
    pub(crate) max_depth: usize,
    pub(crate) values_members: usize,
    pub(crate) max_string_bytes: usize,
}

struct Budget {
    remaining: usize,
    max_depth: usize,
    max_string_bytes: usize,
}

/// UTF-8 text with a byte ceiling; IDs and labels additionally forbid empty text.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct Text<const NONEMPTY: bool>(pub(super) String);

impl<const N: bool> TryFrom<String> for Text<N> {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() > STRING_BYTES || (N && value.is_empty()) {
            Err("text byte limit or empty identity/label")
        } else {
            Ok(Self(value))
        }
    }
}

/// Exact canonical signed decimal; never converted through binary64.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct I64Decimal(pub(crate) i64);

/// Exact canonical unsigned decimal, including zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct U64Decimal(pub(crate) u64);

fn canonical_digits(value: &str) -> bool {
    value == "0"
        || (value.starts_with(['1', '2', '3', '4', '5', '6', '7', '8', '9'])
            && value.bytes().all(|byte| byte.is_ascii_digit()))
}

impl TryFrom<String> for I64Decimal {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let digits = value.strip_prefix('-').unwrap_or(&value);
        if value.len() > 20 || value == "-0" || !canonical_digits(digits) {
            return Err("noncanonical I64Decimal");
        }
        value.parse().map(Self).map_err(|_| "I64Decimal overflow")
    }
}

impl TryFrom<String> for U64Decimal {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() > 20 || !canonical_digits(&value) {
            return Err("noncanonical U64Decimal");
        }
        value.parse().map(Self).map_err(|_| "U64Decimal overflow")
    }
}

/// A finite JSON Number; this type is never used for Integer or identities.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(try_from = "f64")]
pub(crate) struct Number(pub(crate) f64);
impl TryFrom<f64> for Number {
    type Error = &'static str;
    fn try_from(value: f64) -> Result<Self, Self::Error> {
        value
            .is_finite()
            .then_some(Self(value))
            .ok_or("nonfinite Number")
    }
}

/// An array which rejects excess elements before decoding/retaining them.
#[derive(Debug)]
pub(crate) struct List<T, const MAX: usize>(pub(crate) Vec<T>);

impl<'de, T: Deserialize<'de>, const MAX: usize> Deserialize<'de> for List<T, MAX> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ArrayVisitor<T, const MAX: usize>(std::marker::PhantomData<T>);
        impl<'de, T: Deserialize<'de>, const MAX: usize> Visitor<'de> for ArrayVisitor<T, MAX> {
            type Value = List<T, MAX>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("bounded array")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while values.len() < MAX {
                    match seq.next_element()? {
                        Some(value) => values.push(value),
                        None => return Ok(List(values)),
                    }
                }
                struct Excess;
                impl<'de> Deserialize<'de> for Excess {
                    fn deserialize<D: Deserializer<'de>>(_: D) -> Result<Self, D::Error> {
                        Err(de::Error::custom("array element limit"))
                    }
                }
                seq.next_element::<Excess>()?;
                Ok(List(values))
            }
        }
        deserializer.deserialize_seq(ArrayVisitor::<T, MAX>(std::marker::PhantomData))
    }
}

/// Optional means absent, not null, except fields explicitly typed nullable.
pub(super) fn optional<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}

/// deserialize_with forces required-field handling, even with an Option payload.
pub(super) fn nullable<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(d)
}

// No JSON value tree is built. Only bounded object-key sets remain on the stack.
struct Scan<'a> {
    budget: &'a mut Budget,
    depth: usize,
}
impl Scan<'_> {
    fn charge<E: de::Error>(&mut self) -> Result<(), E> {
        self.budget.remaining = self
            .budget
            .remaining
            .checked_sub(1)
            .ok_or_else(|| E::custom("JSON value/member limit"))?;
        Ok(())
    }
}
impl<'de> DeserializeSeed<'de> for Scan<'_> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(mut self, d: D) -> Result<(), D::Error> {
        self.charge()?;
        d.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for Scan<'_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("bounded JSON")
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> Result<(), E> {
        Ok(())
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> Result<(), E> {
        Ok(())
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> Result<(), E> {
        Ok(())
    }
    fn visit_f64<E: de::Error>(self, value: f64) -> Result<(), E> {
        if value.is_finite() {
            Ok(())
        } else {
            Err(E::custom("nonfinite number"))
        }
    }
    fn visit_unit<E: de::Error>(self) -> Result<(), E> {
        Ok(())
    }
    fn visit_str<E: de::Error>(self, value: &str) -> Result<(), E> {
        self.budget.max_string_bytes = self.budget.max_string_bytes.max(value.len());
        if value.len() <= STRING_BYTES {
            Ok(())
        } else {
            Err(E::custom("string byte limit"))
        }
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        if self.depth > DEPTH {
            return Err(de::Error::custom("JSON depth limit"));
        }
        self.budget.max_depth = self.budget.max_depth.max(self.depth);
        while seq
            .next_element_seed(Scan {
                budget: self.budget,
                depth: self.depth + 1,
            })?
            .is_some()
        {}
        Ok(())
    }
    fn visit_map<A: MapAccess<'de>>(mut self, mut map: A) -> Result<(), A::Error> {
        if self.depth > DEPTH {
            return Err(de::Error::custom("JSON depth limit"));
        }
        self.budget.max_depth = self.budget.max_depth.max(self.depth);
        let mut keys = BTreeSet::new();
        while let Some(key) = map.next_key::<Text<false>>()? {
            self.budget.max_string_bytes = self.budget.max_string_bytes.max(key.0.len());
            self.charge()?;
            if !keys.insert(key) {
                return Err(de::Error::custom("duplicate JSON key"));
            }
            map.next_value_seed(Scan {
                budget: self.budget,
                depth: self.depth + 1,
            })?;
        }
        Ok(())
    }
}

pub(super) fn preflight(bytes: &[u8]) -> Result<(), String> {
    inspect(bytes).map(|_| ())
}

pub(super) fn inspect(bytes: &[u8]) -> Result<JsonUsage, String> {
    if bytes.len() > INPUT_BYTES {
        return Err("JSON body byte limit".into());
    }
    // A valid <=512-byte decoded string uses <=6*512 encoded bytes. Bound serde's
    // escape scratch buffer before it can copy an oversized string (including keys).
    let (mut in_string, mut escaped, mut length) = (false, false, 0usize);
    for &byte in bytes {
        if in_string {
            if byte == b'"' && !escaped {
                in_string = false;
                continue;
            }
            length += 1;
            if length > STRING_BYTES * 6 {
                return Err("encoded string byte limit".into());
            }
            escaped = byte == b'\\' && !escaped;
        } else if byte == b'"' {
            in_string = true;
            length = 0;
        }
    }
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let mut budget = Budget {
        remaining: VALUES,
        max_depth: 0,
        max_string_bytes: 0,
    };
    Scan {
        budget: &mut budget,
        depth: 1,
    }
    .deserialize(&mut deserializer)
    .map_err(|error| error.to_string())?;
    deserializer.end().map_err(|error| error.to_string())?;
    Ok(JsonUsage {
        body_bytes: bytes.len(),
        max_depth: budget.max_depth,
        values_members: VALUES - budget.remaining,
        max_string_bytes: budget.max_string_bytes,
    })
}
