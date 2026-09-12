//! JSON data model for instances, answers, and `log` values.

mod parse;

use core::fmt;
use core::str::FromStr;

/// Largest integer magnitude written as a JSON number (`2^53 - 1`).
pub const MAX_SAFE_INT: i64 = 9_007_199_254_740_991;

/// Byte cap for one JSON value (`log` / answer).
pub const MAX_BYTES: usize = 1_048_576;

/// Nesting cap for arrays and objects.
pub const MAX_DEPTH: usize = 64;

/// Canonical decimal integer (optional leading `-`, no leading zeros).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JsonInt(String);

impl JsonInt {
    /// Integer from `i64`.
    #[must_use]
    pub fn from_i64(n: i64) -> Self {
        Self(n.to_string())
    }

    /// Parse a decimal integer token (`0`, `-2`, `9007199254740992`).
    ///
    /// # Errors
    ///
    /// Returns [`JsonError::IntDigits`] when the token is not a canonical-able decimal.
    pub fn from_decimal(raw: &str) -> Result<Self, JsonError> {
        Ok(Self(canonicalize_decimal(raw)?))
    }

    /// Decimal digits.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// `i64` when the decimal fits.
    ///
    /// # Errors
    ///
    /// Returns [`JsonError::IntRange`] when the value does not fit in `i64`.
    pub fn to_i64(&self) -> Result<i64, JsonError> {
        self.0.parse().map_err(|_| JsonError::IntRange)
    }

    /// Whether the value may appear as a JSON number.
    #[must_use]
    pub fn fits_safe_json_number(&self) -> bool {
        match self.to_i64() {
            Ok(n) => (-MAX_SAFE_INT..=MAX_SAFE_INT).contains(&n),
            Err(_) => false,
        }
    }
}

pub(super) fn canonicalize_decimal(raw: &str) -> Result<String, JsonError> {
    let (neg, digits) = if let Some(rest) = raw.strip_prefix('-') {
        (true, rest)
    } else {
        (false, raw)
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(JsonError::IntDigits);
    }
    if digits.len() > 1 && digits.starts_with('0') {
        return Err(JsonError::IntDigits);
    }
    if digits == "0" {
        return Ok("0".to_owned());
    }
    if neg {
        let mut s = String::with_capacity(raw.len());
        s.push('-');
        s.push_str(digits);
        Ok(s)
    } else {
        Ok(digits.to_owned())
    }
}

/// JSON value on the problem / answer / `log` wire.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    /// JSON `null`.
    Null,
    /// JSON boolean.
    Bool(bool),
    /// Integer (JSON number when safe, otherwise `{"$i":"…"}`).
    Int(JsonInt),
    /// Finite JSON number with a fractional part or exponent.
    Float(f64),
    /// JSON string.
    String(String),
    /// JSON array (the encoding of a set).
    Array(Vec<Json>),
    /// JSON object; keys are strings.
    Object(Vec<(String, Json)>),
}

impl Json {
    /// Integer value.
    #[must_use]
    pub fn int(n: i64) -> Self {
        Self::Int(JsonInt::from_i64(n))
    }

    /// Finite float.
    ///
    /// # Errors
    ///
    /// Returns [`JsonError::NotFinite`] when `f` is NaN or infinite.
    pub fn float(f: f64) -> Result<Self, JsonError> {
        if f.is_finite() {
            Ok(Self::Float(f))
        } else {
            Err(JsonError::NotFinite)
        }
    }

    /// Object with unique keys.
    ///
    /// # Errors
    ///
    /// Returns [`JsonError::DuplicateKey`] when a key repeats.
    pub fn object(pairs: Vec<(String, Json)>) -> Result<Self, JsonError> {
        for i in 0..pairs.len() {
            for j in 0..i {
                if pairs[i].0 == pairs[j].0 {
                    return Err(JsonError::DuplicateKey);
                }
            }
        }
        Ok(Self::Object(pairs))
    }

    /// Compact JSON text.
    ///
    /// # Errors
    ///
    /// Returns [`JsonError::TooLarge`] when the text would exceed [`MAX_BYTES`],
    /// or [`JsonError::NotFinite`] for a non-finite float.
    pub fn to_compact_string(&self) -> Result<String, JsonError> {
        let mut out = String::new();
        self.write_compact(&mut out)?;
        if out.len() > MAX_BYTES {
            return Err(JsonError::TooLarge);
        }
        Ok(out)
    }

    fn write_compact(&self, out: &mut String) -> Result<(), JsonError> {
        match self {
            Self::Null => out.push_str("null"),
            Self::Bool(true) => out.push_str("true"),
            Self::Bool(false) => out.push_str("false"),
            Self::Int(n) if n.fits_safe_json_number() => out.push_str(n.as_str()),
            Self::Int(n) => {
                out.push_str("{\"$i\":");
                write_json_string(out, n.as_str());
                out.push('}');
            }
            Self::Float(f) => write_float(out, *f)?,
            Self::String(s) => write_json_string(out, s),
            Self::Array(items) => {
                out.push('[');
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    v.write_compact(out)?;
                }
                out.push(']');
            }
            Self::Object(pairs) => {
                out.push('{');
                for (i, (k, v)) in pairs.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_json_string(out, k);
                    out.push(':');
                    v.write_compact(out)?;
                }
                out.push('}');
            }
        }
        Ok(())
    }
}

impl FromStr for Json {
    type Err = JsonError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse::parse(s)
    }
}

/// Why parse or encode failed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JsonError {
    /// Empty input (whitespace only).
    Empty,
    /// Input or output exceeds [`MAX_BYTES`].
    TooLarge,
    /// Nesting exceeds [`MAX_DEPTH`].
    TooDeep,
    /// Non-whitespace after the value.
    Trailing,
    /// Input ended inside a token.
    UnexpectedEof,
    /// Byte not allowed here.
    Unexpected,
    /// `true` / `false` / `null` misspelled.
    InvalidLiteral,
    /// Number grammar failed.
    InvalidNumber,
    /// Untagged integer outside the 53-bit JSON-number range.
    UntaggedWideInt,
    /// String not closed.
    InvalidString,
    /// Bad `\\` escape or surrogate pair.
    InvalidEscape,
    /// Control byte in a string without an escape.
    UnescapedControl,
    /// Repeated object key.
    DuplicateKey,
    /// `$i` object that is not exactly `{ "$i": "<decimal>" }`.
    InvalidTag,
    /// Decimal integer token with leading zeros, a sign `+`, or non-digits.
    IntDigits,
    /// Decimal does not fit in `i64`.
    IntRange,
    /// NaN or infinity.
    NotFinite,
    /// Catalog conversion expected an object.
    ExpectedObject,
    /// Catalog conversion expected an array.
    ExpectedArray,
    /// Catalog conversion expected an integer.
    ExpectedInt,
    /// Required object field is missing.
    MissingField,
    /// Object has a field the catalog type does not allow.
    ExtraField,
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Empty => "empty JSON",
            Self::TooLarge => "JSON exceeds 1 MiB",
            Self::TooDeep => "JSON nesting is too deep",
            Self::Trailing => "trailing JSON text",
            Self::UnexpectedEof => "unexpected end of JSON",
            Self::Unexpected => "unexpected JSON byte",
            Self::InvalidLiteral => "invalid JSON literal",
            Self::InvalidNumber => "invalid JSON number",
            Self::UntaggedWideInt => "integer outside 53 bits must use {\"$i\":\"…\"}",
            Self::InvalidString => "invalid JSON string",
            Self::InvalidEscape => "invalid JSON escape",
            Self::UnescapedControl => "unescaped control in JSON string",
            Self::DuplicateKey => "duplicate JSON object key",
            Self::InvalidTag => "invalid {\"$i\":\"…\"} integer tag",
            Self::IntDigits => "invalid integer decimal",
            Self::IntRange => "integer does not fit i64",
            Self::NotFinite => "non-finite JSON number",
            Self::ExpectedObject => "expected JSON object",
            Self::ExpectedArray => "expected JSON array",
            Self::ExpectedInt => "expected JSON integer",
            Self::MissingField => "missing JSON object field",
            Self::ExtraField => "unexpected JSON object field",
        };
        f.write_str(s)
    }
}

pub(crate) fn write_json_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str("\\u00");
                let n = c as u32;
                out.push(hex_digit(n / 16));
                out.push(hex_digit(n % 16));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn hex_digit(n: u32) -> char {
    char::from(b"0123456789abcdef"[n as usize])
}

fn write_float(out: &mut String, f: f64) -> Result<(), JsonError> {
    if !f.is_finite() {
        return Err(JsonError::NotFinite);
    }
    if f == 0.0 {
        if f.is_sign_negative() {
            out.push_str("-0.0");
        } else {
            out.push_str("0.0");
        }
        return Ok(());
    }
    let s = format!("{f}");
    out.push_str(&s);
    if !s.contains('.') && !s.contains('e') && !s.contains('E') {
        out.push_str(".0");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Json {
        s.parse().expect("parse")
    }

    fn dump(j: &Json) -> String {
        j.to_compact_string().expect("encode")
    }

    fn golden(name: &str) -> &'static str {
        match name {
            "null" => {
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../languages/goldens/null.json"
                ))
            }
            "true" => {
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../languages/goldens/true.json"
                ))
            }
            "false" => {
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../languages/goldens/false.json"
                ))
            }
            "string" => {
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../languages/goldens/string.json"
                ))
            }
            "string-escape" => {
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../languages/goldens/string-escape.json"
                ))
            }
            "ctrl" => {
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../languages/goldens/ctrl.json"
                ))
            }
            "empty-array" => {
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../languages/goldens/empty-array.json"
                ))
            }
            "empty-object" => {
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../languages/goldens/empty-object.json"
                ))
            }
            "float" => {
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../languages/goldens/float.json"
                ))
            }
            "float-zero" => {
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../languages/goldens/float-zero.json"
                ))
            }
            "float-neg-zero" => {
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../languages/goldens/float-neg-zero.json"
                ))
            }
            "float-two" => {
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../languages/goldens/float-two.json"
                ))
            }
            "int-safe" => {
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../languages/goldens/int-safe.json"
                ))
            }
            "int-neg" => {
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../languages/goldens/int-neg.json"
                ))
            }
            "int-wide" => {
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../languages/goldens/int-wide.json"
                ))
            }
            "int-sum-instance" => {
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../languages/goldens/int-sum-instance.json"
                ))
            }
            "int-sum-instance-wide" => {
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../languages/goldens/int-sum-instance-wide.json"
                ))
            }
            "int-sum-answer" => {
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../languages/goldens/int-sum-answer.json"
                ))
            }
            "int-sum-answer-wide" => {
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../languages/goldens/int-sum-answer-wide.json"
                ))
            }
            "nested" => {
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../languages/goldens/nested.json"
                ))
            }
            "set-as-array" => {
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../languages/goldens/set-as-array.json"
                ))
            }
            other => panic!("missing golden {other}"),
        }
    }

    #[test]
    fn goldens_round_trip() {
        for name in [
            "null",
            "true",
            "false",
            "string",
            "string-escape",
            "ctrl",
            "empty-array",
            "empty-object",
            "float",
            "float-zero",
            "float-neg-zero",
            "float-two",
            "int-safe",
            "int-neg",
            "int-wide",
            "int-sum-instance",
            "int-sum-instance-wide",
            "int-sum-answer",
            "int-sum-answer-wide",
            "nested",
            "set-as-array",
        ] {
            let src = golden(name).trim();
            let parsed = parse(src);
            assert_eq!(dump(&parsed), src, "{name}");
        }
    }

    #[test]
    #[should_panic(expected = "missing golden")]
    fn missing_golden_panics() {
        let _ = golden("no-such-fixture");
    }

    #[test]
    fn display_every_error() {
        let all = [
            JsonError::Empty,
            JsonError::TooLarge,
            JsonError::TooDeep,
            JsonError::Trailing,
            JsonError::UnexpectedEof,
            JsonError::Unexpected,
            JsonError::InvalidLiteral,
            JsonError::InvalidNumber,
            JsonError::UntaggedWideInt,
            JsonError::InvalidString,
            JsonError::InvalidEscape,
            JsonError::UnescapedControl,
            JsonError::DuplicateKey,
            JsonError::InvalidTag,
            JsonError::IntDigits,
            JsonError::IntRange,
            JsonError::NotFinite,
            JsonError::ExpectedObject,
            JsonError::ExpectedArray,
            JsonError::ExpectedInt,
            JsonError::MissingField,
            JsonError::ExtraField,
        ];
        for e in all {
            assert!(!e.to_string().is_empty());
        }
    }

    #[test]
    fn json_int_helpers() {
        let n = JsonInt::from_i64(-3);
        assert_eq!(n.as_str(), "-3");
        assert_eq!(n.to_i64().expect("i64"), -3);
        assert!(n.fits_safe_json_number());
        let wide = JsonInt::from_i64(MAX_SAFE_INT + 1);
        assert!(!wide.fits_safe_json_number());
        let huge = JsonInt::from_decimal("999999999999999999999").expect("digits");
        assert!(!huge.fits_safe_json_number());
        assert_eq!(huge.to_i64(), Err(JsonError::IntRange));
        assert_eq!(JsonInt::from_decimal("01"), Err(JsonError::IntDigits));
        assert_eq!(JsonInt::from_decimal(""), Err(JsonError::IntDigits));
        assert_eq!(JsonInt::from_decimal("-"), Err(JsonError::IntDigits));
        assert_eq!(JsonInt::from_decimal("+1"), Err(JsonError::IntDigits));
        assert_eq!(JsonInt::from_decimal("1e2"), Err(JsonError::IntDigits));
        assert_eq!(JsonInt::from_decimal("-0").expect("z").as_str(), "0");
        assert_eq!(JsonInt::from_decimal("0").expect("z").as_str(), "0");
    }

    #[test]
    fn float_ctor_and_zero() {
        assert_eq!(Json::float(f64::NAN), Err(JsonError::NotFinite));
        assert_eq!(Json::float(f64::INFINITY), Err(JsonError::NotFinite));
        let z = Json::float(0.0).expect("z");
        assert_eq!(dump(&z), "0.0");
        let nz = Json::float(-0.0).expect("nz");
        assert_eq!(dump(&nz), "-0.0");
        let two = Json::float(2.0).expect("2");
        assert_eq!(dump(&two), "2.0");
    }

    #[test]
    fn object_duplicate_and_int() {
        let err = Json::object(vec![
            ("a".into(), Json::Null),
            ("a".into(), Json::Bool(true)),
        ]);
        assert_eq!(err, Err(JsonError::DuplicateKey));
        let nan = Json::Float(f64::NAN);
        assert_eq!(nan.to_compact_string(), Err(JsonError::NotFinite));
        assert_eq!(dump(&Json::int(7)), "7");
        let wide = Json::int(MAX_SAFE_INT + 1);
        assert_eq!(dump(&wide), "{\"$i\":\"9007199254740992\"}");
    }

    #[test]
    fn encode_too_large() {
        let big = Json::String("a".repeat(MAX_BYTES));
        assert_eq!(big.to_compact_string(), Err(JsonError::TooLarge));
    }

    #[test]
    fn parse_whitespace_and_pretty() {
        let j: Json = " { \"v\" : [ 1 , 2 ] } ".parse().expect("ws");
        assert_eq!(dump(&j), "{\"v\":[1,2]}");
    }

    #[test]
    fn surrogate_pair_and_slash() {
        let j: Json = r#""\uD834\uDD1E\/""#.parse().expect("clef");
        assert_eq!(j, Json::String("\u{1D11E}/".into()));
        assert_eq!(dump(&j), "\"\u{1D11E}/\"");
    }
}
