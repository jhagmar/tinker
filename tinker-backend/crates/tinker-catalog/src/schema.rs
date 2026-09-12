//! JSON Schema for catalog instance and answer objects.

/// JSON Schema document for a catalog type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Schema {
    kind: Kind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Kind {
    Integer,
    Array {
        items: Box<Schema>,
    },
    Object {
        properties: Vec<(String, Schema)>,
        required: Vec<String>,
    },
}

impl Schema {
    /// JSON integer.
    #[must_use]
    pub fn integer() -> Self {
        Self {
            kind: Kind::Integer,
        }
    }

    /// JSON array.
    #[must_use]
    pub fn array(items: Self) -> Self {
        Self {
            kind: Kind::Array {
                items: Box::new(items),
            },
        }
    }

    /// JSON object with `additionalProperties` false.
    #[must_use]
    pub fn object(properties: Vec<(String, Self)>, required: Vec<String>) -> Self {
        Self {
            kind: Kind::Object {
                properties,
                required,
            },
        }
    }

    /// Compact JSON Schema text.
    #[must_use]
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        self.write_json(&mut out);
        out
    }

    fn write_json(&self, out: &mut String) {
        match &self.kind {
            Kind::Integer => out.push_str("{\"type\":\"integer\"}"),
            Kind::Array { items } => {
                out.push_str("{\"type\":\"array\",\"items\":");
                items.write_json(out);
                out.push('}');
            }
            Kind::Object {
                properties,
                required,
            } => {
                out.push_str("{\"type\":\"object\",\"properties\":{");
                for (i, (k, v)) in properties.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    push_json_string(out, k);
                    out.push(':');
                    v.write_json(out);
                }
                out.push_str("},\"required\":[");
                for (i, k) in required.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    push_json_string(out, k);
                }
                out.push_str("],\"additionalProperties\":false}");
            }
        }
    }
}

fn push_json_string(out: &mut String, s: &str) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_and_array() {
        assert_eq!(Schema::integer().to_json(), "{\"type\":\"integer\"}");
        assert_eq!(
            Schema::array(Schema::integer()).to_json(),
            "{\"type\":\"array\",\"items\":{\"type\":\"integer\"}}"
        );
    }

    #[test]
    fn object_and_escapes() {
        let schema = Schema::object(
            vec![
                ("v".to_owned(), Schema::array(Schema::integer())),
                ("a\"b".to_owned(), Schema::integer()),
                ("x\\y".to_owned(), Schema::integer()),
                ("nl\n".to_owned(), Schema::integer()),
                ("cr\r".to_owned(), Schema::integer()),
                ("tab\t".to_owned(), Schema::integer()),
                ("nul\u{0001}".to_owned(), Schema::integer()),
            ],
            vec!["v".to_owned()],
        );
        let json = schema.to_json();
        assert!(json.contains("\"v\":{\"type\":\"array\""));
        assert!(json.contains("\"a\\\"b\""));
        assert!(json.contains("\"x\\\\y\""));
        assert!(json.contains("\"nl\\n\""));
        assert!(json.contains("\"cr\\r\""));
        assert!(json.contains("\"tab\\t\""));
        assert!(json.contains("\"nul\\u0001\""));
        assert!(json.contains("\"additionalProperties\":false"));
        assert!(json.starts_with("{\"type\":\"object\""));
        let two_req = Schema::object(
            vec![
                ("a".to_owned(), Schema::integer()),
                ("b".to_owned(), Schema::integer()),
            ],
            vec!["a".to_owned(), "b".to_owned()],
        );
        assert!(two_req.to_json().contains("\"required\":[\"a\",\"b\"]"));
        let ctrl = Schema::object(
            vec![("c\u{001f}".to_owned(), Schema::integer())],
            Vec::new(),
        );
        assert!(ctrl.to_json().contains("\\u001f"));
    }

    #[test]
    fn empty_object() {
        let json = Schema::object(Vec::new(), Vec::new()).to_json();
        assert_eq!(
            json,
            "{\"type\":\"object\",\"properties\":{},\"required\":[],\"additionalProperties\":false}"
        );
    }
}
