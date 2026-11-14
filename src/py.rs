//! Small pieces of Python behaviour that the file formats and the printed messages depend on.
//!
//! The Council Loop began as Python scripts. Runs, ledgers and prompt packets written by either
//! implementation must read the same, so the handful of Python conventions that leak into files
//! (how `json.dumps` lays out a document, how `repr` quotes a string, what `str.split` treats as a
//! space, how `round` breaks a tie) are reproduced here once and used everywhere.

use serde_json::Value;
use std::fmt::Write as _;

/// `str.isspace()` for one character: Unicode white space plus the four ASCII separators
/// (FS, GS, RS, US) that Python counts and Unicode does not.
pub fn is_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// `text.split()`: the words of a text.
pub fn split(text: &str) -> impl Iterator<Item = &str> {
    text.split(is_space).filter(|w| !w.is_empty())
}

/// `len(text.split())`.
pub fn words(text: &str) -> usize {
    split(text).count()
}

/// `" ".join(text.split())`: one space between words, none at the ends.
pub fn squash(text: &str) -> String {
    split(text).collect::<Vec<_>>().join(" ")
}

/// `text.strip()`.
pub fn strip(text: &str) -> &str {
    text.trim_matches(is_space)
}

/// `text[:n]`, counted in characters as Python does (never in bytes).
pub fn head(text: &str, n: usize) -> &str {
    match text.char_indices().nth(n) {
        Some((i, _)) => &text[..i],
        None => text,
    }
}

/// `text[n:]`, counted in characters.
pub fn tail(text: &str, n: usize) -> &str {
    match text.char_indices().nth(n) {
        Some((i, _)) => &text[i..],
        None => "",
    }
}

/// `len(text)`: characters, not bytes.
pub fn len(text: &str) -> usize {
    text.chars().count()
}

/// `text.splitlines()` for the line ends the kit meets (`\n`, `\r\n`, `\r`).
pub fn lines(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        match rest.find(['\n', '\r']) {
            Some(i) => {
                out.push(&rest[..i]);
                let step = if rest[i..].starts_with("\r\n") { 2 } else { 1 };
                rest = &rest[i + step..];
            }
            None => {
                out.push(rest);
                break;
            }
        }
    }
    out
}

/// `f"{n:,}"`.
pub fn commas(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 {
        format!("-{out}")
    } else {
        out
    }
}

/// `round(x, digits)`: to the nearest, ties to even on the exact binary value.
pub fn round(x: f64, digits: usize) -> f64 {
    if !x.is_finite() {
        return x;
    }
    format!("{x:.digits$}").parse().unwrap_or(x)
}

/// `repr(x)` of a float: the shortest text that reads back as the same number, with Python's
/// switch to exponent form below 1e-4 and from 1e16.
pub fn float_repr(x: f64) -> String {
    if x.is_nan() {
        return "nan".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "inf".into() } else { "-inf".into() };
    }
    if x == 0.0 {
        return if x.is_sign_negative() {
            "-0.0".into()
        } else {
            "0.0".into()
        };
    }
    let sci = format!("{:e}", x.abs());
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let digits: String = mantissa.chars().filter(|c| c.is_ascii_digit()).collect();
    let decpt = exp + 1;
    let sign = if x < 0.0 { "-" } else { "" };
    if decpt <= -4 || decpt > 16 {
        let body = if digits.len() > 1 {
            format!("{}.{}", &digits[..1], &digits[1..])
        } else {
            digits
        };
        return format!(
            "{sign}{body}e{}{:02}",
            if exp < 0 { '-' } else { '+' },
            exp.abs()
        );
    }
    let n = digits.len() as i32;
    let body = if decpt <= 0 {
        format!("0.{}{}", "0".repeat((-decpt) as usize), digits)
    } else if decpt >= n {
        format!("{}{}.0", digits, "0".repeat((decpt - n) as usize))
    } else {
        format!(
            "{}.{}",
            &digits[..decpt as usize],
            &digits[decpt as usize..]
        )
    };
    format!("{sign}{body}")
}

/// Would `str.isprintable()` say no? Controls, every space but U+0020, format characters (the
/// invisible ones that ride along in pasted text) and private-use code points.
pub fn unprintable(c: char) -> bool {
    c.is_control()
        || (c != ' ' && c.is_whitespace())
        || matches!(c,
            '\u{ad}' | '\u{600}'..='\u{605}' | '\u{61c}' | '\u{6dd}' | '\u{70f}' | '\u{890}'..='\u{891}' | '\u{8e2}' | '\u{180e}'
            | '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{206f}'
            | '\u{e000}'..='\u{f8ff}' | '\u{feff}' | '\u{fff9}'..='\u{fffb}' | '\u{fffe}'..='\u{ffff}'
            | '\u{110bd}' | '\u{110cd}' | '\u{13430}'..='\u{1343f}' | '\u{1bca0}'..='\u{1bca3}' | '\u{1d173}'..='\u{1d17a}'
            | '\u{e0001}' | '\u{e0020}'..='\u{e007f}' | '\u{f0000}'..='\u{ffffd}' | '\u{100000}'..='\u{10fffd}')
}

/// `repr(text)`.
pub fn str_repr(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(text.len() + 2);
    out.push(quote);
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if unprintable(c) => {
                let n = c as u32;
                let _ = if n < 0x100 {
                    write!(out, "\\x{n:02x}")
                } else if n < 0x10000 {
                    write!(out, "\\u{n:04x}")
                } else {
                    write!(out, "\\U{n:08x}")
                };
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// `repr(value)` of what a JSON or YAML document can hold.
pub fn repr(value: &Value) -> String {
    match value {
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::Number(n) => number(n),
        Value::String(s) => str_repr(s),
        Value::Array(a) => format!("[{}]", a.iter().map(repr).collect::<Vec<_>>().join(", ")),
        Value::Object(o) => {
            let items: Vec<String> = o
                .iter()
                .map(|(k, v)| format!("{}: {}", str_repr(k), repr(v)))
                .collect();
            format!("{{{}}}", items.join(", "))
        }
    }
}

/// `str(value)`: like `repr`, but a string is shown bare.
pub fn to_str(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => repr(other),
    }
}

/// A tuple of strings as `repr` shows it: `('a', 'b')`.
pub fn tuple_repr(items: &[&str]) -> String {
    let inner: Vec<String> = items.iter().map(|s| str_repr(s)).collect();
    if inner.len() == 1 {
        format!("({},)", inner[0])
    } else {
        format!("({})", inner.join(", "))
    }
}

fn number(n: &serde_json::Number) -> String {
    if let Some(i) = n.as_i64() {
        i.to_string()
    } else if let Some(u) = n.as_u64() {
        u.to_string()
    } else {
        float_repr(n.as_f64().unwrap_or(0.0))
    }
}

/// Is this value "truthy" in Python (`if value:`)?
pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// How `json.dumps` should lay a document out.
#[derive(Clone, Copy, Debug, Default)]
pub struct Json {
    /// Spaces per level; `None` puts everything on one line.
    pub indent: Option<usize>,
    /// Sort the keys of every object.
    pub sort_keys: bool,
    /// Keep characters above ASCII as they are (`ensure_ascii=False`).
    pub unicode: bool,
}

impl Json {
    /// `json.dumps(value)`.
    pub const LINE: Json = Json {
        indent: None,
        sort_keys: false,
        unicode: false,
    };
    /// `json.dumps(value, indent=1)`.
    pub const ONE: Json = Json {
        indent: Some(1),
        sort_keys: false,
        unicode: false,
    };
    /// `json.dumps(value, indent=2)`.
    pub const TWO: Json = Json {
        indent: Some(2),
        sort_keys: false,
        unicode: false,
    };

    /// The same layout with characters above ASCII kept.
    pub const fn unicode(mut self) -> Json {
        self.unicode = true;
        self
    }

    /// The same layout with sorted keys.
    pub const fn sorted(mut self) -> Json {
        self.sort_keys = true;
        self
    }

    /// The text `json.dumps` would produce for `value`.
    pub fn dumps(&self, value: &Value) -> String {
        let mut out = String::new();
        self.write(&mut out, value, 0);
        out
    }

    fn newline(&self, out: &mut String, level: usize) {
        if let Some(n) = self.indent {
            out.push('\n');
            out.extend(std::iter::repeat_n(' ', n * level));
        }
    }

    fn write(&self, out: &mut String, value: &Value, level: usize) {
        let comma = if self.indent.is_some() { "," } else { ", " };
        match value {
            Value::Null => out.push_str("null"),
            Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Value::Number(n) => out.push_str(&number(n)),
            Value::String(s) => self.string(out, s),
            Value::Array(a) if a.is_empty() => out.push_str("[]"),
            Value::Array(a) => {
                out.push('[');
                for (i, item) in a.iter().enumerate() {
                    if i > 0 {
                        out.push_str(comma);
                    }
                    self.newline(out, level + 1);
                    self.write(out, item, level + 1);
                }
                self.newline(out, level);
                out.push(']');
            }
            Value::Object(o) if o.is_empty() => out.push_str("{}"),
            Value::Object(o) => {
                let mut items: Vec<(&String, &Value)> = o.iter().collect();
                if self.sort_keys {
                    items.sort_by(|a, b| a.0.cmp(b.0));
                }
                out.push('{');
                for (i, (k, v)) in items.into_iter().enumerate() {
                    if i > 0 {
                        out.push_str(comma);
                    }
                    self.newline(out, level + 1);
                    self.string(out, k);
                    out.push_str(": ");
                    self.write(out, v, level + 1);
                }
                self.newline(out, level);
                out.push('}');
            }
        }
    }

    fn string(&self, out: &mut String, s: &str) {
        out.push('"');
        for c in s.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                '\u{8}' => out.push_str("\\b"),
                '\u{c}' => out.push_str("\\f"),
                c if (c as u32) < 0x20 => {
                    let _ = write!(out, "\\u{:04x}", c as u32);
                }
                c if !self.unicode && (c as u32) > 0x7e => {
                    let mut units = [0u16; 2];
                    for unit in c.encode_utf16(&mut units) {
                        let _ = write!(out, "\\u{unit:04x}");
                    }
                }
                c => out.push(c),
            }
        }
        out.push('"');
    }
}

/// `fnmatch.fnmatch(name, pattern)`: `*` any run (slashes too), `?` one character, `[seq]` a set.
/// Names are compared as written on POSIX and without regard to case or slash direction on Windows.
pub fn fnmatch(name: &str, pattern: &str) -> bool {
    fn norm(s: &str) -> Vec<char> {
        if cfg!(windows) {
            s.to_lowercase().replace('/', "\\").chars().collect()
        } else {
            s.chars().collect()
        }
    }
    fn go(n: &[char], p: &[char]) -> bool {
        match p.first() {
            None => n.is_empty(),
            Some('*') => (0..=n.len()).any(|i| go(&n[i..], &p[1..])),
            Some('?') => !n.is_empty() && go(&n[1..], &p[1..]),
            Some('[') => {
                let mut j = 1;
                if p.get(j) == Some(&'!') {
                    j += 1;
                }
                if p.get(j) == Some(&']') {
                    j += 1;
                }
                while j < p.len() && p[j] != ']' {
                    j += 1;
                }
                if j >= p.len() {
                    return !n.is_empty() && n[0] == '[' && go(&n[1..], &p[1..]);
                }
                let Some(&c) = n.first() else { return false };
                let (negate, set) = match p.get(1) {
                    Some('!') => (true, &p[2..j]),
                    _ => (false, &p[1..j]),
                };
                let mut hit = false;
                let mut i = 0;
                while i < set.len() {
                    if i + 2 < set.len() && set[i + 1] == '-' {
                        hit |= set[i] <= c && c <= set[i + 2];
                        i += 3;
                    } else {
                        hit |= set[i] == c;
                        i += 1;
                    }
                }
                hit != negate && go(&n[1..], &p[j + 1..])
            }
            Some(&c) => n.first() == Some(&c) && go(&n[1..], &p[1..]),
        }
    }
    go(&norm(name), &norm(pattern))
}

/// `zlib.crc32(bytes)`.
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// `Path(text).expanduser()`: a leading `~` is the home folder; duplicate slashes, `.` parts and a
/// trailing slash are dropped, as `pathlib` drops them.
pub fn path(text: &str) -> std::path::PathBuf {
    use std::path::PathBuf;
    let expanded: PathBuf = if text == "~" {
        crate::core::home_dir()
    } else if let Some(rest) = text.strip_prefix("~/").or_else(|| {
        if cfg!(windows) {
            text.strip_prefix("~\\")
        } else {
            None
        }
    }) {
        crate::core::home_dir().join(rest)
    } else {
        PathBuf::from(text)
    };
    plain(expanded)
}

/// `Path(text)`: the path as `pathlib` spells it. Duplicate separators, `.` parts and a trailing
/// separator are dropped, and on Windows every `/` becomes `\\`, so `C:/Users/me` (what Git Bash
/// hands a program) and `C:\\Users\\me` are printed and stored as one and the same.
pub fn plain(path: impl AsRef<std::path::Path>) -> std::path::PathBuf {
    use std::path::{Component, PathBuf};
    let clean: PathBuf = path
        .as_ref()
        .components()
        .filter(|c| !matches!(c, Component::CurDir))
        .collect();
    if clean.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        clean
    }
}

/// A path the way every platform should print it inside a project: with forward slashes.
pub fn posix(path: &std::path::Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn paths_are_spelled_as_pathlib_spells_them() {
        let show = |text: &str| plain(text).display().to_string();
        assert_eq!(show("."), ".");
        assert_eq!(show(""), ".");
        if cfg!(windows) {
            // what Git Bash hands a program, and what Explorer shows, are one and the same folder
            assert_eq!(show("C:/Users/me/vault/"), "C:\\Users\\me\\vault");
            assert_eq!(show("C:\\Users\\me/vault"), "C:\\Users\\me\\vault");
            assert_eq!(show("notes//memory/./x"), "notes\\memory\\x");
        } else {
            assert_eq!(show("/home/me//vault/./notes/"), "/home/me/vault/notes");
            assert_eq!(show("notes//memory/./x"), "notes/memory/x");
            assert_eq!(show("a\\b"), "a\\b"); // a backslash is an ordinary letter here
        }
        assert_eq!(posix(&plain("notes//memory/./x")), "notes/memory/x");
    }

    #[test]
    fn words_follow_python_split() {
        assert_eq!(words("  a\tb\u{a0}c\u{1c}d \n"), 4);
        assert_eq!(squash(" a \n b "), "a b");
        assert_eq!(head("naïve → fine", 6), "naïve ");
        assert_eq!(len("→→"), 2);
        assert_eq!(lines("a\r\nb\nc\rd"), vec!["a", "b", "c", "d"]);
        assert_eq!(lines("a\n"), vec!["a"]);
    }

    #[test]
    fn floats_print_as_python_prints_them() {
        for (x, want) in [
            (1.0, "1.0"),
            (0.5, "0.5"),
            (1.86, "1.86"),
            (0.1 + 0.2, "0.30000000000000004"),
            (1e16, "1e+16"),
            (1e15, "1000000000000000.0"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (1.5e-7, "1.5e-07"),
            (-2.25, "-2.25"),
            (123456.789, "123456.789"),
            (1e22, "1e+22"),
        ] {
            assert_eq!(float_repr(x), want);
        }
        assert_eq!(round(0.125, 2), 0.12);
        assert_eq!(round(2.675, 2), 2.67);
        assert_eq!(round(0.5, 0), 0.0);
        assert_eq!(round(1.5, 0), 2.0);
        assert_eq!(commas(1_269_206), "1,269,206");
        assert_eq!(commas(92), "92");
    }

    #[test]
    fn repr_quotes_as_python_does() {
        assert_eq!(str_repr("plain"), "'plain'");
        assert_eq!(str_repr("it's"), "\"it's\"");
        assert_eq!(str_repr("both ' and \""), "'both \\' and \"'");
        assert_eq!(str_repr("a\nb\\"), "'a\\nb\\\\'");
        assert_eq!(
            repr(&json!(["a", 1, 2.0, true, null, {"k": "v"}])),
            "['a', 1, 2.0, True, None, {'k': 'v'}]"
        );
        assert_eq!(tuple_repr(&["stale", "always"]), "('stale', 'always')");
    }

    #[test]
    fn json_matches_json_dumps() {
        let v =
            json!({"a": 1, "b": [1, 2.5, "x→y"], "c": {}, "d": [], "e": {"f": null, "g": true}});
        assert_eq!(
            Json::LINE.dumps(&v),
            r#"{"a": 1, "b": [1, 2.5, "x\u2192y"], "c": {}, "d": [], "e": {"f": null, "g": true}}"#
        );
        assert_eq!(
            Json::ONE.dumps(&v),
            "{\n \"a\": 1,\n \"b\": [\n  1,\n  2.5,\n  \"x\\u2192y\"\n ],\n \"c\": {},\n \"d\": [],\n \"e\": {\n  \"f\": null,\n  \"g\": true\n }\n}"
        );
        assert_eq!(
            Json::LINE.unicode().dumps(&json!("x→\u{1F600}\u{7f}")),
            "\"x→\u{1F600}\u{7f}\""
        );
        assert_eq!(
            Json::LINE.dumps(&json!("\u{1F600}\u{7f}")),
            r#""\ud83d\ude00\u007f""#
        );
        assert_eq!(
            Json::LINE.sorted().dumps(&json!({"b": 1, "a": 2})),
            r#"{"a": 2, "b": 1}"#
        );
    }

    #[test]
    fn fnmatch_and_crc32_match_the_standard_library() {
        assert!(fnmatch("db/migrations/001.sql", "*/migrations/*"));
        assert!(fnmatch("a/b/c.sql", "*.sql"));
        assert!(fnmatch("Dockerfile.dev", "Dockerfile*"));
        assert!(!fnmatch("src/app.py", "*.tf"));
        assert!(fnmatch("x1", "x[0-9]") && !fnmatch("xa", "x[0-9]") && fnmatch("xa", "x[!0-9]"));
        assert_eq!(crc32(b"261113-183103-take-turn-review/1/vote"), 0x57E3_36D7);
        assert_eq!(crc32(b""), 0);
    }
}
