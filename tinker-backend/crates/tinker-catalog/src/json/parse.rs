//! Recursive-descent JSON parser with size and depth caps.

use super::{Json, JsonError, JsonInt, MAX_BYTES, MAX_DEPTH};

struct Parser<'a> {
    bytes: &'a [u8],
    i: usize,
}

pub(super) fn parse(input: &str) -> Result<Json, JsonError> {
    if input.len() > MAX_BYTES {
        return Err(JsonError::TooLarge);
    }
    let mut p = Parser {
        bytes: input.as_bytes(),
        i: 0,
    };
    p.skip_ws();
    if p.eof() {
        return Err(JsonError::Empty);
    }
    let value = p.value(0)?;
    p.skip_ws();
    if !p.eof() {
        return Err(JsonError::Trailing);
    }
    Ok(value)
}

impl<'a> Parser<'a> {
    fn eof(&self) -> bool {
        self.i >= self.bytes.len()
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.i).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let b = self.peek()?;
        self.i += 1;
        Some(b)
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, JsonError> {
        match self.peek() {
            None => Err(JsonError::UnexpectedEof),
            Some(b'n') => self.literal(b"null", Json::Null),
            Some(b't') => self.literal(b"true", Json::Bool(true)),
            Some(b'f') => self.literal(b"false", Json::Bool(false)),
            Some(b'"') => Ok(Json::String(self.string()?)),
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => {
                self.i += 1;
                Err(JsonError::Unexpected)
            }
        }
    }

    fn literal(&mut self, token: &[u8], value: Json) -> Result<Json, JsonError> {
        for &b in token {
            match self.bump() {
                Some(x) if x == b => {}
                Some(_) => return Err(JsonError::InvalidLiteral),
                None => return Err(JsonError::UnexpectedEof),
            }
        }
        if self.peek().is_some_and(is_token_cont) {
            return Err(JsonError::InvalidLiteral);
        }
        Ok(value)
    }

    fn number(&mut self) -> Result<Json, JsonError> {
        let start = self.i;
        if self.peek() == Some(b'-') {
            self.i += 1;
        }
        match self.peek() {
            Some(b'0') => self.i += 1,
            Some(b'1'..=b'9') => {
                self.i += 1;
                while self.peek().is_some_and(|b| b.is_ascii_digit()) {
                    self.i += 1;
                }
            }
            _ => return Err(JsonError::InvalidNumber),
        }
        let mut is_float = false;
        if self.peek() == Some(b'.') {
            is_float = true;
            self.i += 1;
            if !self.peek().is_some_and(|b| b.is_ascii_digit()) {
                return Err(JsonError::InvalidNumber);
            }
            while self.peek().is_some_and(|b| b.is_ascii_digit()) {
                self.i += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            is_float = true;
            self.i += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.i += 1;
            }
            if !self.peek().is_some_and(|b| b.is_ascii_digit()) {
                return Err(JsonError::InvalidNumber);
            }
            while self.peek().is_some_and(|b| b.is_ascii_digit()) {
                self.i += 1;
            }
        }
        let raw = core::str::from_utf8(&self.bytes[start..self.i]).expect("ascii number");
        if is_float {
            let f: f64 = raw.parse().map_err(|_| JsonError::InvalidNumber)?;
            if !f.is_finite() {
                return Err(JsonError::NotFinite);
            }
            Ok(Json::Float(f))
        } else {
            let int = JsonInt::from_decimal(raw)?;
            if !int.fits_safe_json_number() {
                return Err(JsonError::UntaggedWideInt);
            }
            Ok(Json::Int(int))
        }
    }

    fn string(&mut self) -> Result<String, JsonError> {
        let start = self.bump().expect("string opener");
        debug_assert_eq!(start, b'"');
        let mut out = String::new();
        loop {
            match self.bump() {
                None => return Err(JsonError::InvalidString),
                Some(b'"') => return Ok(out),
                Some(b'\\') => out.push(self.escape()?),
                Some(b) if b < 0x20 => return Err(JsonError::UnescapedControl),
                Some(_) => {
                    self.i -= 1;
                    let rest = core::str::from_utf8(&self.bytes[self.i..]).expect("utf-8 input");
                    let ch = rest.chars().next().expect("non-empty");
                    self.i += ch.len_utf8();
                    out.push(ch);
                }
            }
        }
    }

    fn escape(&mut self) -> Result<char, JsonError> {
        match self.bump() {
            Some(b'"') => Ok('"'),
            Some(b'\\') => Ok('\\'),
            Some(b'/') => Ok('/'),
            Some(b'b') => Ok('\u{0008}'),
            Some(b'f') => Ok('\u{000c}'),
            Some(b'n') => Ok('\n'),
            Some(b'r') => Ok('\r'),
            Some(b't') => Ok('\t'),
            Some(b'u') => self.unicode_escape(),
            Some(_) | None => Err(JsonError::InvalidEscape),
        }
    }

    fn unicode_escape(&mut self) -> Result<char, JsonError> {
        let u = self.hex4()?;
        if (0xD800..=0xDBFF).contains(&u) {
            if self.bump() != Some(b'\\') || self.bump() != Some(b'u') {
                return Err(JsonError::InvalidEscape);
            }
            let low = self.hex4()?;
            if !(0xDC00..=0xDFFF).contains(&low) {
                return Err(JsonError::InvalidEscape);
            }
            let cp = 0x10000 + (u32::from(u - 0xD800) << 10) + u32::from(low - 0xDC00);
            Ok(char::from_u32(cp).expect("surrogate pair"))
        } else if (0xDC00..=0xDFFF).contains(&u) {
            Err(JsonError::InvalidEscape)
        } else {
            Ok(char::from_u32(u32::from(u)).expect("bmp"))
        }
    }

    fn hex4(&mut self) -> Result<u16, JsonError> {
        let mut v = 0u16;
        for _ in 0..4 {
            let b = self.bump().ok_or(JsonError::InvalidEscape)?;
            let n = hex_val(b).ok_or(JsonError::InvalidEscape)?;
            v = (v << 4) | u16::from(n);
        }
        Ok(v)
    }

    fn object(&mut self, depth: usize) -> Result<Json, JsonError> {
        if depth >= MAX_DEPTH {
            return Err(JsonError::TooDeep);
        }
        let depth = depth + 1;
        self.i += 1;
        self.skip_ws();
        let mut pairs = Vec::new();
        if self.peek() == Some(b'}') {
            self.i += 1;
            return lift_tag(pairs);
        }
        loop {
            self.skip_ws();
            match self.peek() {
                Some(b'"') => {}
                None => return Err(JsonError::UnexpectedEof),
                Some(_) => return Err(JsonError::Unexpected),
            }
            let key = self.string()?;
            if pairs.iter().any(|(k, _)| *k == key) {
                return Err(JsonError::DuplicateKey);
            }
            self.skip_ws();
            if self.bump() != Some(b':') {
                return Err(JsonError::Unexpected);
            }
            self.skip_ws();
            let val = self.value(depth)?;
            pairs.push((key, val));
            self.skip_ws();
            match self.bump() {
                Some(b'}') => return lift_tag(pairs),
                Some(b',') => {}
                Some(_) => return Err(JsonError::Unexpected),
                None => return Err(JsonError::UnexpectedEof),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Json, JsonError> {
        if depth >= MAX_DEPTH {
            return Err(JsonError::TooDeep);
        }
        let depth = depth + 1;
        self.i += 1;
        self.skip_ws();
        let mut items = Vec::new();
        if self.peek() == Some(b']') {
            self.i += 1;
            return Ok(Json::Array(items));
        }
        loop {
            self.skip_ws();
            items.push(self.value(depth)?);
            self.skip_ws();
            match self.bump() {
                Some(b']') => return Ok(Json::Array(items)),
                Some(b',') => {}
                Some(_) => return Err(JsonError::Unexpected),
                None => return Err(JsonError::UnexpectedEof),
            }
        }
    }
}

fn lift_tag(pairs: Vec<(String, Json)>) -> Result<Json, JsonError> {
    if pairs.len() == 1 && pairs[0].0 == "$i" {
        return match &pairs[0].1 {
            Json::String(s) => Ok(Json::Int(JsonInt::from_decimal(s)?)),
            _ => Err(JsonError::InvalidTag),
        };
    }
    if pairs.iter().any(|(k, _)| k == "$i") {
        return Err(JsonError::InvalidTag);
    }
    Ok(Json::Object(pairs))
}

fn is_token_cont(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err(s: &str) -> JsonError {
        parse(s).expect_err("error")
    }

    #[test]
    fn parse_errors() {
        let cases: &[(&str, JsonError)] = &[
            ("", JsonError::Empty),
            ("   ", JsonError::Empty),
            ("null x", JsonError::Trailing),
            ("nul", JsonError::UnexpectedEof),
            ("nxll", JsonError::InvalidLiteral),
            ("truee", JsonError::InvalidLiteral),
            ("falsee", JsonError::InvalidLiteral),
            ("tru", JsonError::UnexpectedEof),
            ("\"\\", JsonError::InvalidEscape),
            ("+", JsonError::Unexpected),
            ("01", JsonError::Trailing),
            ("-", JsonError::InvalidNumber),
            ("1.", JsonError::InvalidNumber),
            ("1e", JsonError::InvalidNumber),
            ("1e+", JsonError::InvalidNumber),
            ("\"abc", JsonError::InvalidString),
            ("\"\\x\"", JsonError::InvalidEscape),
            ("\"\\u12\"", JsonError::InvalidEscape),
            ("\"\\u12zz\"", JsonError::InvalidEscape),
            ("\"\\uD800\"", JsonError::InvalidEscape),
            ("\"\\uD800\\u0000\"", JsonError::InvalidEscape),
            ("\"\\uDC00\"", JsonError::InvalidEscape),
            ("\"\u{0001}\"", JsonError::UnescapedControl),
            ("{\"a\":1,\"a\":2}", JsonError::DuplicateKey),
            ("{\"a\" 1}", JsonError::Unexpected),
            ("{\"a\":1 x}", JsonError::Unexpected),
            ("{1:2}", JsonError::Unexpected),
            ("[1 2]", JsonError::Unexpected),
            ("{\"a\":", JsonError::UnexpectedEof),
            ("[1,", JsonError::UnexpectedEof),
            ("{\"a\":1", JsonError::UnexpectedEof),
            ("[1", JsonError::UnexpectedEof),
            ("[", JsonError::UnexpectedEof),
            ("{", JsonError::UnexpectedEof),
            ("{\"a\":1,}", JsonError::Unexpected),
            ("[1,]", JsonError::Unexpected),
            ("{\"$i\":true}", JsonError::InvalidTag),
            ("{\"$i\":1}", JsonError::InvalidTag),
            ("{\"$i\":\"1\",\"x\":2}", JsonError::InvalidTag),
            ("{\"$i\":\"01\"}", JsonError::IntDigits),
            ("9007199254740992", JsonError::UntaggedWideInt),
            ("-9007199254740992", JsonError::UntaggedWideInt),
        ];
        for (src, want) in cases {
            assert_eq!(err(src), *want, "{src}");
        }
    }

    #[test]
    fn too_large_and_too_deep() {
        let big = "a".repeat(MAX_BYTES + 1);
        assert_eq!(err(&big), JsonError::TooLarge);
        let mut deep = String::new();
        for _ in 0..=MAX_DEPTH {
            deep.push('[');
        }
        for _ in 0..=MAX_DEPTH {
            deep.push(']');
        }
        assert_eq!(err(&deep), JsonError::TooDeep);
        let mut deep_obj = String::from("{}");
        for _ in 0..=MAX_DEPTH {
            deep_obj = format!("{{\"a\":{deep_obj}}}");
        }
        assert_eq!(err(&deep_obj), JsonError::TooDeep);
    }

    #[test]
    fn numbers_and_escapes() {
        assert_eq!(parse("0").expect("0"), Json::int(0));
        assert_eq!(parse("-0").expect("-0"), Json::int(0));
        assert_eq!(parse("1e2").expect("e"), Json::Float(100.0));
        assert_eq!(parse("1E+2").expect("E"), Json::Float(100.0));
        assert_eq!(parse("-1.25").expect("f"), Json::Float(-1.25));
        let s = parse(r#""\b\f\n\r\t\"\\\/""#).expect("esc");
        assert_eq!(s, Json::String("\u{0008}\u{000c}\n\r\t\"\\/".into()));
        assert_eq!(parse(r#""\u0041""#).expect("A"), Json::String("A".into()));
        assert_eq!(
            parse(r#""\u00aB""#).expect("ab"),
            Json::String("\u{00ab}".into())
        );
        assert_eq!(parse("{\"$i\":\"-2\"}").expect("tag"), Json::int(-2));
        assert_eq!(parse("[]").expect("arr"), Json::Array(Vec::new()));
        let obj = parse("{\"a\":null}").expect("obj");
        assert_eq!(
            obj,
            Json::object(vec![("a".into(), Json::Null)]).expect("obj")
        );
    }

    #[test]
    fn utf8_string() {
        assert_eq!(
            parse("\"café\"").expect("utf8"),
            Json::String("café".into())
        );
        assert_eq!(parse("\"𝄞\"").expect("clef"), Json::String("𝄞".into()));
    }

    #[test]
    fn nonfinite_number() {
        // 1e999 overflows f64 to inf
        assert_eq!(err("1e999"), JsonError::NotFinite);
    }
}
