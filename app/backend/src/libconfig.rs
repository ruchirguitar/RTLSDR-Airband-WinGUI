//! Reader/writer for the libconfig text format used by rtl_airband's .conf files.
//!
//! The tree is exchanged with the web UI as JSON in the same shape the Python
//! GUI used (engine/gui/config_io.py): groups are JSON objects (key order kept),
//! lists and arrays are JSON arrays, and every scalar leaf is wrapped as
//! `{"__scalar__": true, "type": "int"|"float"|"bool"|"string", "value": ...}`
//! so that `120.0` (float) and `120` (int) survive the trip through JavaScript.

use serde_json::{json, Map, Value};
use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

pub fn scalar(kind: &str, value: Value) -> Value {
    json!({"__scalar__": true, "type": kind, "value": value})
}

struct Parser<'a> {
    src: &'a [u8],
    pos: usize,
    line: usize,
}

impl<'a> Parser<'a> {
    fn err<T>(&self, message: impl Into<String>) -> Result<T, ParseError> {
        Err(ParseError { line: self.line, message: message.into() })
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let c = self.peek()?;
        self.pos += 1;
        if c == b'\n' {
            self.line += 1;
        }
        Some(c)
    }

    fn starts_with(&self, s: &str) -> bool {
        self.src[self.pos..].starts_with(s.as_bytes())
    }

    fn skip_ws(&mut self) -> Result<(), ParseError> {
        loop {
            match self.peek() {
                Some(c) if c.is_ascii_whitespace() => {
                    self.bump();
                }
                Some(b'#') => self.skip_line(),
                Some(b'/') if self.starts_with("//") => self.skip_line(),
                Some(b'/') if self.starts_with("/*") => {
                    let start_line = self.line;
                    self.pos += 2;
                    loop {
                        if self.starts_with("*/") {
                            self.pos += 2;
                            break;
                        }
                        if self.bump().is_none() {
                            return Err(ParseError { line: start_line, message: "unterminated comment".into() });
                        }
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    fn skip_line(&mut self) {
        while let Some(c) = self.peek() {
            if c == b'\n' {
                break;
            }
            self.pos += 1;
        }
    }

    fn parse_settings(&mut self, closing: Option<u8>) -> Result<Map<String, Value>, ParseError> {
        let mut map = Map::new();
        loop {
            self.skip_ws()?;
            match self.peek() {
                None if closing.is_none() => return Ok(map),
                None => return self.err("unexpected end of file, missing '}'"),
                Some(c) if Some(c) == closing => {
                    self.bump();
                    return Ok(map);
                }
                Some(b'@') => return self.err("@include is not supported"),
                _ => {}
            }
            let name = self.parse_name()?;
            self.skip_ws()?;
            match self.bump() {
                Some(b'=') | Some(b':') => {}
                _ => return self.err(format!("expected '=' or ':' after '{name}'")),
            }
            let value = self.parse_value()?;
            self.skip_ws()?;
            if matches!(self.peek(), Some(b';') | Some(b',')) {
                self.bump();
            }
            if map.contains_key(&name) {
                return self.err(format!("duplicate setting '{name}'"));
            }
            map.insert(name, value);
        }
    }

    fn parse_name(&mut self) -> Result<String, ParseError> {
        let start = self.pos;
        match self.peek() {
            Some(c) if c.is_ascii_alphabetic() || c == b'*' => {}
            Some(c) => return self.err(format!("unexpected character '{}'", c as char)),
            None => return self.err("unexpected end of file"),
        }
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'*') {
                self.pos += 1;
            } else {
                break;
            }
        }
        Ok(String::from_utf8_lossy(&self.src[start..self.pos]).into_owned())
    }

    fn parse_value(&mut self) -> Result<Value, ParseError> {
        self.skip_ws()?;
        match self.peek() {
            Some(b'{') => {
                self.bump();
                Ok(Value::Object(self.parse_settings(Some(b'}'))?))
            }
            Some(b'(') => {
                self.bump();
                self.parse_sequence(b')')
            }
            Some(b'[') => {
                self.bump();
                self.parse_sequence(b']')
            }
            Some(b'"') => self.parse_string(),
            Some(_) => self.parse_bare_scalar(),
            None => self.err("unexpected end of file, expected a value"),
        }
    }

    fn parse_sequence(&mut self, closing: u8) -> Result<Value, ParseError> {
        let mut items = Vec::new();
        loop {
            self.skip_ws()?;
            if self.peek() == Some(closing) {
                self.bump();
                return Ok(Value::Array(items));
            }
            items.push(self.parse_value()?);
            self.skip_ws()?;
            match self.peek() {
                Some(b',') => {
                    self.bump();
                }
                Some(c) if c == closing => {}
                _ => return self.err(format!("expected ',' or '{}'", closing as char)),
            }
        }
    }

    fn parse_string(&mut self) -> Result<Value, ParseError> {
        let mut out = Vec::new();
        // Adjacent string literals are concatenated, as in C.
        while self.peek() == Some(b'"') {
            self.bump();
            loop {
                match self.bump() {
                    None => return self.err("unterminated string"),
                    Some(b'"') => break,
                    Some(b'\\') => match self.bump() {
                        Some(b'n') => out.push(b'\n'),
                        Some(b't') => out.push(b'\t'),
                        Some(b'r') => out.push(b'\r'),
                        Some(b'f') => out.push(0x0c),
                        Some(b'\\') => out.push(b'\\'),
                        Some(b'"') => out.push(b'"'),
                        Some(b'x') => {
                            let hex = self.src.get(self.pos..self.pos + 2).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok());
                            match hex {
                                Some(b) => {
                                    out.push(b);
                                    self.pos += 2;
                                }
                                None => return self.err("invalid \\x escape"),
                            }
                        }
                        _ => return self.err("invalid escape in string"),
                    },
                    Some(c) => out.push(c),
                }
            }
            self.skip_ws()?;
        }
        Ok(scalar("string", Value::String(String::from_utf8_lossy(&out).into_owned())))
    }

    fn parse_bare_scalar(&mut self) -> Result<Value, ParseError> {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || matches!(c, b'.' | b'+' | b'-') {
                self.pos += 1;
            } else {
                break;
            }
        }
        let token = String::from_utf8_lossy(&self.src[start..self.pos]).into_owned();
        if token.is_empty() {
            return self.err(format!("unexpected character '{}'", self.peek().map(|c| c as char).unwrap_or(' ')));
        }
        let lower = token.to_ascii_lowercase();
        if lower == "true" || lower == "false" {
            return Ok(scalar("bool", Value::Bool(lower == "true")));
        }
        let digits = lower.trim_end_matches('l');
        let (neg, unsigned) = match digits.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, digits.strip_prefix('+').unwrap_or(digits)),
        };
        if let Some(hex) = unsigned.strip_prefix("0x") {
            if let Ok(v) = i64::from_str_radix(hex, 16) {
                return Ok(scalar("int", json!(if neg { -v } else { v })));
            }
        } else if !unsigned.is_empty() && unsigned.bytes().all(|c| c.is_ascii_digit()) {
            if let Ok(v) = digits.parse::<i64>() {
                return Ok(scalar("int", json!(v)));
            }
        } else if let Ok(v) = lower.parse::<f64>() {
            if v.is_finite() {
                return Ok(scalar("float", json!(v)));
            }
        }
        self.err(format!("invalid value '{token}'"))
    }
}

pub fn parse(text: &str) -> Result<Value, ParseError> {
    let mut p = Parser { src: text.as_bytes(), pos: 0, line: 1 };
    Ok(Value::Object(p.parse_settings(None)?))
}

fn is_scalar(v: &Value) -> bool {
    v.get("__scalar__").and_then(Value::as_bool).unwrap_or(false)
}

fn format_float(f: f64) -> String {
    let s = format!("{f:?}");
    if s.contains('.') || s.contains('e') || s.contains("inf") || s.contains("NaN") {
        s
    } else {
        format!("{s}.0")
    }
}

fn escape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\x{:02x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn dump_scalar(v: &Value) -> Result<String, String> {
    let kind = v.get("type").and_then(Value::as_str).unwrap_or("string");
    let raw = v.get("value").unwrap_or(&Value::Null);
    Ok(match kind {
        "bool" => match raw {
            Value::Bool(b) => b.to_string(),
            Value::String(s) => (s.eq_ignore_ascii_case("true")).to_string(),
            other => (!other.is_null() && other.as_f64().unwrap_or(0.0) != 0.0).to_string(),
        },
        "int" => {
            let n = match raw {
                Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)).unwrap_or(0),
                Value::String(s) => s.trim().parse::<f64>().map(|f| f as i64).unwrap_or(0),
                _ => 0,
            };
            if n > i32::MAX as i64 || n < i32::MIN as i64 {
                format!("{n}L")
            } else {
                n.to_string()
            }
        }
        "float" => {
            let f = match raw {
                Value::Number(n) => n.as_f64().unwrap_or(0.0),
                Value::String(s) => s.trim().parse::<f64>().unwrap_or(0.0),
                _ => 0.0,
            };
            format_float(f)
        }
        _ => escape_string(&match raw {
            Value::String(s) => s.clone(),
            Value::Null => String::new(),
            other => other.to_string(),
        }),
    })
}

fn dump_value(v: &Value, indent: usize, out: &mut String) -> Result<(), String> {
    let pad = "    ".repeat(indent);
    match v {
        Value::Object(map) if is_scalar(v) => {
            let _ = map;
            out.push_str(&dump_scalar(v)?);
        }
        Value::Object(map) => {
            out.push_str("{\n");
            dump_settings(map, indent + 1, out)?;
            out.push_str(&pad);
            out.push('}');
        }
        Value::Array(items) => {
            let all_scalars = items.iter().all(is_scalar);
            if all_scalars {
                let parts: Result<Vec<_>, _> = items.iter().map(dump_scalar).collect();
                let _ = write!(out, "( {} )", parts?.join(", "));
            } else {
                out.push_str("(\n");
                for (i, item) in items.iter().enumerate() {
                    out.push_str(&"    ".repeat(indent + 1));
                    dump_value(item, indent + 1, out)?;
                    if i + 1 < items.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                out.push_str(&pad);
                out.push(')');
            }
        }
        Value::Bool(b) => out.push_str(&b.to_string()),
        Value::Number(n) => out.push_str(&n.to_string()),
        Value::String(s) => out.push_str(&escape_string(s)),
        Value::Null => return Err("null value".into()),
    }
    Ok(())
}

fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '*') && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '*'))
}

fn dump_settings(map: &Map<String, Value>, indent: usize, out: &mut String) -> Result<(), String> {
    let pad = "    ".repeat(indent);
    for (key, value) in map {
        if !valid_name(key) {
            return Err(format!("invalid setting name '{key}'"));
        }
        let _ = write!(out, "{pad}{key} = ");
        dump_value(value, indent, out)?;
        out.push_str(";\n");
    }
    Ok(())
}

/// Serializes the JSON tree (as produced by [`parse`], possibly edited by the UI)
/// back to libconfig text.
pub fn dump(tree: &Value) -> Result<String, String> {
    let map = tree.as_object().ok_or("config root must be an object")?;
    let mut out = String::new();
    dump_settings(map, 0, &mut out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf<'a>(tree: &'a Value, path: &[&str]) -> &'a Value {
        let mut v = tree;
        for p in path {
            v = match p.parse::<usize>() {
                Ok(i) => &v[i],
                Err(_) => &v[*p],
            };
        }
        v
    }

    #[test]
    fn parses_scalar_types() {
        let t = parse("a = 1; b = 120.0; c = true; d = \"x\"; e = 0x10; f = -3; g = 1e-5; h = 5000000000L;").unwrap();
        assert_eq!(t["a"], scalar("int", json!(1)));
        assert_eq!(t["b"], scalar("float", json!(120.0)));
        assert_eq!(t["c"], scalar("bool", json!(true)));
        assert_eq!(t["d"], scalar("string", json!("x")));
        assert_eq!(t["e"], scalar("int", json!(16)));
        assert_eq!(t["f"], scalar("int", json!(-3)));
        assert_eq!(t["g"], scalar("float", json!(1e-5)));
        assert_eq!(t["h"], scalar("int", json!(5000000000i64)));
    }

    #[test]
    fn parses_nested_structures_and_comments() {
        let text = r#"
# comment
// another
/* block
   comment */
devices:
(
  {
    type = "rtlsdr";
    centerfreq = 145.0;
    channels: ( { freq = 145.4; outputs: ( { type = "file"; directory = "C:\\rec"; } ); } );
  }
);
freqs = [ 1.0, 2.0 ];
"#;
        let t = parse(text).unwrap();
        assert_eq!(leaf(&t, &["devices", "0", "type", "value"]), &json!("rtlsdr"));
        assert_eq!(leaf(&t, &["devices", "0", "channels", "0", "outputs", "0", "directory", "value"]), &json!("C:\\rec"));
        assert_eq!(t["freqs"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn keeps_key_order() {
        let t = parse("z = 1; a = 2; m = 3;").unwrap();
        let keys: Vec<_> = t.as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, ["z", "a", "m"]);
    }

    #[test]
    fn concatenates_adjacent_strings() {
        let t = parse("s = \"ab\" \"cd\";").unwrap();
        assert_eq!(t["s"]["value"], json!("abcd"));
    }

    #[test]
    fn reports_errors_with_line_numbers() {
        let e = parse("a = 1;\nb = ;\n").unwrap_err();
        assert_eq!(e.line, 2);
        assert!(parse("a = { b = 1;").is_err());
        assert!(parse("a = \"open").is_err());
        assert!(parse("a = 1; a = 2;").is_err());
    }

    #[test]
    fn round_trips_through_dump() {
        let text = "a = 1;\nb = 120.0;\nc = false;\nd = \"q\\\"uote\";\ng = { x = ( 1, 2 ); y = ( { k = 1; } ); };\nempty = ( );\n";
        let t = parse(text).unwrap();
        let dumped = dump(&t).unwrap();
        assert_eq!(parse(&dumped).unwrap(), t);
    }

    #[test]
    fn whole_floats_keep_their_decimal_point() {
        let out = dump(&json!({"f": scalar("float", json!(120))})).unwrap();
        assert_eq!(out, "f = 120.0;\n");
    }

    #[test]
    fn bools_are_lowercase() {
        let out = dump(&json!({"b": scalar("bool", json!(true))})).unwrap();
        assert_eq!(out, "b = true;\n");
    }

    #[test]
    fn values_typed_in_the_ui_as_strings_are_coerced() {
        let out = dump(&json!({"i": scalar("int", json!("25")), "f": scalar("float", json!("145.4"))})).unwrap();
        assert_eq!(out, "i = 25;\nf = 145.4;\n");
    }

    #[test]
    fn parses_every_sample_config() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../configs");
        let mut count = 0;
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) != Some("conf") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let tree = parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let again = parse(&dump(&tree).unwrap()).unwrap();
            assert_eq!(again, tree, "{} did not round-trip", path.display());
            count += 1;
        }
        assert!(count > 0);
    }
}
