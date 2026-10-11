//! tiny std-only JSON: parse + dump. enough for registry APIs and our own protocol.
use std::fmt::Write;

#[derive(Clone, Debug, PartialEq)]
pub enum J {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

impl From<&str> for J {
    fn from(s: &str) -> J {
        J::Str(s.to_string())
    }
}
impl From<String> for J {
    fn from(s: String) -> J {
        J::Str(s)
    }
}
impl From<bool> for J {
    fn from(b: bool) -> J {
        J::Bool(b)
    }
}
impl From<i64> for J {
    fn from(n: i64) -> J {
        J::Num(n as f64)
    }
}
impl From<usize> for J {
    fn from(n: usize) -> J {
        J::Num(n as f64)
    }
}

pub fn obj(pairs: Vec<(&str, J)>) -> J {
    J::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

impl J {
    pub fn get(&self, k: &str) -> Option<&J> {
        match self {
            J::Obj(v) => v.iter().find(|(kk, _)| kk == k).map(|(_, v)| v),
            _ => None,
        }
    }
    #[allow(dead_code)]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            J::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn items(&self) -> &[J] {
        match self {
            J::Arr(v) => v,
            _ => &[],
        }
    }
    pub fn fields(&self) -> &[(String, J)] {
        match self {
            J::Obj(v) => v,
            _ => &[],
        }
    }
    /// field `k` rendered as text ("" when missing); numbers and bools stringified.
    pub fn text(&self, k: &str) -> String {
        match self.get(k) {
            Some(J::Str(s)) => s.clone(),
            Some(J::Num(n)) => fmt_num(*n),
            Some(J::Bool(b)) => b.to_string(),
            _ => String::new(),
        }
    }
    pub fn dump(&self) -> String {
        let mut s = String::new();
        self.write(&mut s);
        s
    }
    fn write(&self, o: &mut String) {
        match self {
            J::Null => o.push_str("null"),
            J::Bool(b) => o.push_str(if *b { "true" } else { "false" }),
            J::Num(n) => o.push_str(&fmt_num(*n)),
            J::Str(s) => write_str(s, o),
            J::Arr(v) => {
                o.push('[');
                for (i, x) in v.iter().enumerate() {
                    if i > 0 {
                        o.push(',');
                    }
                    x.write(o);
                }
                o.push(']');
            }
            J::Obj(v) => {
                o.push('{');
                for (i, (k, x)) in v.iter().enumerate() {
                    if i > 0 {
                        o.push(',');
                    }
                    write_str(k, o);
                    o.push(':');
                    x.write(o);
                }
                o.push('}');
            }
        }
    }
}

fn fmt_num(n: f64) -> String {
    if n.is_finite() && n.fract() == 0.0 && n.abs() < 9.0e15 {
        format!("{}", n as i64)
    } else if n.is_finite() {
        format!("{}", n)
    } else {
        "null".to_string()
    }
}

fn write_str(s: &str, o: &mut String) {
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(o, "\\u{:04x}", c as u32);
            }
            c => o.push(c),
        }
    }
    o.push('"');
}

pub fn parse(src: &str) -> Result<J, String> {
    let src = src.trim_start_matches('\u{feff}');
    let mut p = P { b: src.as_bytes(), i: 0 };
    p.ws();
    let v = p.val(0)?;
    p.ws();
    if p.i != p.b.len() {
        return Err(format!("trailing data at byte {}", p.i));
    }
    Ok(v)
}

struct P<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> P<'a> {
    fn ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\n' | b'\r' | b'\t') {
            self.i += 1;
        }
    }
    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }
    fn next(&mut self) -> Option<u8> {
        let c = self.peek();
        if c.is_some() {
            self.i += 1;
        }
        c
    }
    fn expect(&mut self, c: u8) -> Result<(), String> {
        if self.next() == Some(c) {
            Ok(())
        } else {
            Err(format!("expected '{}' at byte {}", c as char, self.i))
        }
    }
    fn lit(&mut self, word: &str, v: J) -> Result<J, String> {
        if self.b[self.i..].starts_with(word.as_bytes()) {
            self.i += word.len();
            Ok(v)
        } else {
            Err(format!("bad literal at byte {}", self.i))
        }
    }
    fn val(&mut self, depth: usize) -> Result<J, String> {
        if depth > 64 {
            return Err("nesting too deep".into());
        }
        match self.peek() {
            None => Err("unexpected end of input".into()),
            Some(b'{') => {
                self.i += 1;
                let mut v = Vec::new();
                self.ws();
                if self.peek() == Some(b'}') {
                    self.i += 1;
                    return Ok(J::Obj(v));
                }
                loop {
                    self.ws();
                    let k = self.string()?;
                    self.ws();
                    self.expect(b':')?;
                    self.ws();
                    let x = self.val(depth + 1)?;
                    v.push((k, x));
                    self.ws();
                    match self.next() {
                        Some(b',') => continue,
                        Some(b'}') => break,
                        _ => return Err(format!("expected ',' or '}}' at byte {}", self.i)),
                    }
                }
                Ok(J::Obj(v))
            }
            Some(b'[') => {
                self.i += 1;
                let mut v = Vec::new();
                self.ws();
                if self.peek() == Some(b']') {
                    self.i += 1;
                    return Ok(J::Arr(v));
                }
                loop {
                    self.ws();
                    v.push(self.val(depth + 1)?);
                    self.ws();
                    match self.next() {
                        Some(b',') => continue,
                        Some(b']') => break,
                        _ => return Err(format!("expected ',' or ']' at byte {}", self.i)),
                    }
                }
                Ok(J::Arr(v))
            }
            Some(b'"') => Ok(J::Str(self.string()?)),
            Some(b't') => self.lit("true", J::Bool(true)),
            Some(b'f') => self.lit("false", J::Bool(false)),
            Some(b'n') => self.lit("null", J::Null),
            Some(_) => {
                let st = self.i;
                while self.i < self.b.len() && b"-+0123456789.eE".contains(&self.b[self.i]) {
                    self.i += 1;
                }
                std::str::from_utf8(&self.b[st..self.i])
                    .ok()
                    .and_then(|s| s.parse::<f64>().ok())
                    .map(J::Num)
                    .ok_or_else(|| format!("bad value at byte {}", st))
            }
        }
    }
    fn hex4(&mut self) -> Result<u32, String> {
        let mut n = 0u32;
        for _ in 0..4 {
            let c = self.next().ok_or("bad \\u escape")?;
            let d = (c as char).to_digit(16).ok_or("bad \\u escape")?;
            n = n * 16 + d;
        }
        Ok(n)
    }
    fn string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let c = self.next().ok_or("unterminated string")?;
            match c {
                b'"' => break,
                b'\\' => {
                    let e = self.next().ok_or("bad escape")?;
                    match e {
                        b'"' => out.push(b'"'),
                        b'\\' => out.push(b'\\'),
                        b'/' => out.push(b'/'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'n' => out.push(10),
                        b'r' => out.push(13),
                        b't' => out.push(9),
                        b'u' => {
                            let mut cp = self.hex4()?;
                            if (0xD800..0xDC00).contains(&cp)
                                && self.b.get(self.i) == Some(&b'\\')
                                && self.b.get(self.i + 1) == Some(&b'u')
                            {
                                self.i += 2;
                                let lo = self.hex4()?;
                                if (0xDC00..0xE000).contains(&lo) {
                                    cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                                }
                            }
                            let ch = char::from_u32(cp).unwrap_or('\u{FFFD}');
                            let mut buf = [0u8; 4];
                            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                        }
                        _ => return Err("bad escape".into()),
                    }
                }
                _ => out.push(c),
            }
        }
        Ok(String::from_utf8_lossy(&out).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let src = r#"{"a":[1,2.5,"x\n\u00e9\ud83e\udd8a"],"b":{"c":null,"d":true}}"#;
        let v = parse(src).unwrap();
        assert_eq!(v.get("a").unwrap().items().len(), 3);
        assert_eq!(v.get("a").unwrap().items()[2].as_str().unwrap(), "x\né🦊");
        let again = parse(&v.dump()).unwrap();
        assert_eq!(v, again);
    }

    #[test]
    fn errors() {
        assert!(parse("{").is_err());
        assert!(parse("[1,]").is_err());
        assert!(parse("{} x").is_err());
    }

    #[test]
    fn text_field() {
        let v = parse(r#"{"n":3,"s":"hi","b":false}"#).unwrap();
        assert_eq!(v.text("n"), "3");
        assert_eq!(v.text("s"), "hi");
        assert_eq!(v.text("b"), "false");
        assert_eq!(v.text("zzz"), "");
    }
}
