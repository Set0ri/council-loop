//! YAML in and out, typed the way PyYAML's `safe_load` types it.
//!
//! Settings, briefs and model data are YAML that people write by hand. The Python reference reads
//! them with PyYAML, which follows YAML 1.1: `yes`, `no`, `on` and `off` are booleans, `010` is
//! octal, `1_000` is a number and `1e3` is a string. To give the same file the same meaning in
//! both implementations, plain scalars are resolved here by those rules and not by the YAML 1.2
//! rules a Rust library would apply.

use regex::Regex;
use saphyr::{ScalarStyle, Yaml, YamlLoader};
use saphyr_parser::Parser;
use serde_json::{Map, Number, Value};
use std::sync::LazyLock;

static INT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:[-+]?0b[0-1_]+|[-+]?0[0-7_]+|[-+]?(?:0|[1-9][0-9_]*)|[-+]?0x[0-9a-fA-F_]+|[-+]?[1-9][0-9_]*(?::[0-5]?[0-9])+)$").unwrap()
});
static FLOAT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?:[-+]?(?:[0-9][0-9_]*)\.[0-9_]*(?:[eE][-+][0-9]+)?|\.[0-9][0-9_]*(?:[eE][-+][0-9]+)?|[-+]?[0-9][0-9_]*(?::[0-5]?[0-9])+\.[0-9_]*|[-+]?\.(?:inf|Inf|INF)|\.(?:nan|NaN|NAN))$",
    )
    .unwrap()
});

/// Read one YAML document. An empty document is `null`.
pub fn load(text: &str) -> Result<Value, String> {
    let mut parser = Parser::new_from_str(text);
    let mut loader: YamlLoader<Yaml> = YamlLoader::default();
    loader.early_parse(false);
    parser.load(&mut loader, true).map_err(|e| e.to_string())?;
    Ok(loader
        .into_documents()
        .first()
        .map(convert)
        .unwrap_or(Value::Null))
}

fn convert(node: &Yaml) -> Value {
    match node {
        Yaml::Representation(text, style, tag) => {
            let plain = matches!(style, ScalarStyle::Plain);
            match tag.as_deref().map(|t| t.suffix.as_str()) {
                Some("str") => Value::String(text.to_string()),
                Some("int" | "float" | "bool" | "null") => resolve(text),
                _ if plain => resolve(text),
                _ => Value::String(text.to_string()),
            }
        }
        Yaml::Value(scalar) => match scalar {
            saphyr::Scalar::Null => Value::Null,
            saphyr::Scalar::Boolean(b) => Value::Bool(*b),
            saphyr::Scalar::Integer(i) => Value::from(*i),
            saphyr::Scalar::FloatingPoint(f) => float(f.into_inner()),
            saphyr::Scalar::String(s) => Value::String(s.to_string()),
        },
        Yaml::Sequence(items) => Value::Array(items.iter().map(convert).collect()),
        Yaml::Mapping(map) => {
            let mut out = Map::new();
            let mut merged: Vec<Value> = Vec::new();
            for (k, v) in map {
                if matches!(k, Yaml::Representation(t, ScalarStyle::Plain, _) if t == "<<") {
                    match convert(v) {
                        Value::Array(parts) => merged.extend(parts),
                        other => merged.push(other),
                    }
                    continue;
                }
                out.insert(key(&convert(k)), convert(v));
            }
            for part in merged {
                if let Value::Object(extra) = part {
                    for (k, v) in extra {
                        out.entry(k).or_insert(v);
                    }
                }
            }
            Value::Object(out)
        }
        Yaml::Tagged(_, inner) => convert(inner),
        Yaml::Alias(_) | Yaml::BadValue => Value::Null,
    }
}

/// A mapping key as JSON spells it (`json.dumps` turns `1`, `true` and `null` keys into text).
fn key(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        other => crate::py::repr(other),
    }
}

fn float(f: f64) -> Value {
    Number::from_f64(f)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

fn sexagesimal(digits: &str) -> f64 {
    digits.split(':').fold(0.0, |acc, part| {
        acc * 60.0 + part.parse::<f64>().unwrap_or(0.0)
    })
}

/// The value of a plain (unquoted) scalar under PyYAML's rules.
fn resolve(text: &str) -> Value {
    match text {
        "" | "~" | "null" | "Null" | "NULL" => return Value::Null,
        "yes" | "Yes" | "YES" | "true" | "True" | "TRUE" | "on" | "On" | "ON" => {
            return Value::Bool(true)
        }
        "no" | "No" | "NO" | "false" | "False" | "FALSE" | "off" | "Off" | "OFF" => {
            return Value::Bool(false)
        }
        _ => {}
    }
    if INT.is_match(text) {
        let clean = text.replace('_', "");
        let (sign, body) = match clean.strip_prefix('-') {
            Some(rest) => (-1i64, rest),
            None => (1, clean.strip_prefix('+').unwrap_or(&clean)),
        };
        let parsed = if let Some(bin) = body.strip_prefix("0b") {
            i64::from_str_radix(bin, 2).ok()
        } else if let Some(hex) = body.strip_prefix("0x") {
            i64::from_str_radix(hex, 16).ok()
        } else if body.contains(':') {
            Some(sexagesimal(body) as i64)
        } else if body.len() > 1 && body.starts_with('0') {
            i64::from_str_radix(body, 8).ok()
        } else {
            body.parse().ok()
        };
        return match parsed {
            Some(n) => Value::from(sign * n),
            None => Value::String(text.to_string()),
        };
    }
    if FLOAT.is_match(text) {
        let clean = text.replace('_', "").to_lowercase();
        let (sign, body) = match clean.strip_prefix('-') {
            Some(rest) => (-1.0, rest),
            None => (1.0, clean.strip_prefix('+').unwrap_or(&clean)),
        };
        let parsed = match body {
            ".inf" => f64::INFINITY,
            ".nan" => f64::NAN,
            _ if body.contains(':') => sexagesimal(body),
            _ => body.parse().unwrap_or(f64::NAN),
        };
        return float(sign * parsed);
    }
    Value::String(text.to_string())
}

// ----------------------------------------------------------------------------- writing
//
// A port of the part of PyYAML's emitter that `yaml.safe_dump(data, sort_keys=False)` uses: block
// style, the same choice between plain, single-quoted and double-quoted scalars, and the same line
// folding at the width. A file written here and a file written by the Python reference are the
// same bytes, so `lock.yaml` and the settings files do not flip between two spellings in a diff.

static TIMESTAMP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?:[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]|[0-9][0-9][0-9][0-9]-[0-9][0-9]?-[0-9][0-9]?(?:[Tt]|[ \t]+)[0-9][0-9]?:[0-9][0-9]:[0-9][0-9](?:\.[0-9]*)?(?:[ \t]*(?:Z|[-+][0-9][0-9]?(?::[0-9][0-9])?))?)$",
    )
    .unwrap()
});

/// Would this text, written plain, be read back as text (and not as a number, a date, a boolean...)?
fn plain_is_text(text: &str) -> bool {
    !(matches!(
        text,
        "" | "~"
            | "null"
            | "Null"
            | "NULL"
            | "<<"
            | "="
            | "yes"
            | "Yes"
            | "YES"
            | "no"
            | "No"
            | "NO"
            | "true"
            | "True"
            | "TRUE"
            | "false"
            | "False"
            | "FALSE"
            | "on"
            | "On"
            | "ON"
            | "off"
            | "Off"
            | "OFF"
    ) || INT.is_match(text)
        || FLOAT.is_match(text)
        || TIMESTAMP.is_match(text))
}

fn is_break(c: char) -> bool {
    matches!(c, '\n' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

fn is_blank(c: Option<char>) -> bool {
    matches!(
        c,
        None | Some('\0' | ' ' | '\t' | '\r' | '\n' | '\u{85}' | '\u{2028}' | '\u{2029}')
    )
}

/// What a text allows (PyYAML's `analyze_scalar`, with `allow_unicode` off).
struct Analysis {
    empty: bool,
    multiline: bool,
    block_plain: bool,
    single_quoted: bool,
}

fn analyze(text: &str) -> Analysis {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return Analysis {
            empty: true,
            multiline: false,
            block_plain: true,
            single_quoted: true,
        };
    }
    let (mut block_indicators, mut line_breaks, mut special) = (false, false, false);
    let (mut leading_space, mut leading_break, mut trailing_space, mut trailing_break) =
        (false, false, false, false);
    let (mut break_space, mut space_break) = (false, false);
    if text.starts_with("---") || text.starts_with("...") {
        block_indicators = true;
    }
    let mut preceded_by_whitespace = true;
    let (mut previous_space, mut previous_break) = (false, false);
    let last = chars.len() - 1;
    for (index, &ch) in chars.iter().enumerate() {
        let followed_by_whitespace = is_blank(chars.get(index + 1).copied());
        if index == 0 {
            if "#,[]{}&*!|>'\"%@`".contains(ch)
                || (matches!(ch, '?' | ':' | '-') && followed_by_whitespace)
            {
                block_indicators = true;
            }
        } else if (ch == ':' && followed_by_whitespace) || (ch == '#' && preceded_by_whitespace) {
            block_indicators = true;
        }
        if is_break(ch) {
            line_breaks = true;
        }
        if !(ch == '\n' || (' '..='~').contains(&ch)) {
            special = true; // anything outside printable ASCII is written with an escape
        }
        if ch == ' ' {
            leading_space |= index == 0;
            trailing_space |= index == last;
            break_space |= previous_break;
            (previous_space, previous_break) = (true, false);
        } else if is_break(ch) {
            leading_break |= index == 0;
            trailing_break |= index == last;
            space_break |= previous_space;
            (previous_space, previous_break) = (false, true);
        } else {
            (previous_space, previous_break) = (false, false);
        }
        preceded_by_whitespace = is_blank(Some(ch));
    }
    let mut block_plain = !(leading_space || leading_break || trailing_space || trailing_break);
    let mut single_quoted = true;
    if break_space {
        (block_plain, single_quoted) = (false, false);
    }
    if space_break || special {
        (block_plain, single_quoted) = (false, false);
    }
    if line_breaks || block_indicators {
        block_plain = false;
    }
    Analysis {
        empty: false,
        multiline: line_breaks,
        block_plain,
        single_quoted,
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Place {
    Root,
    Item,
    Value,
    Key,
}

struct Emitter {
    out: String,
    column: usize,
    width: usize,
    whitespace: bool,
    indention: bool,
}

impl Emitter {
    fn put(&mut self, text: &str) {
        self.column += text.chars().count();
        self.out.push_str(text);
    }

    fn put_chars(&mut self, chars: &[char]) {
        self.column += chars.len();
        self.out.extend(chars.iter());
    }

    fn line_break(&mut self, text: &str) {
        self.whitespace = true;
        self.indention = true;
        self.column = 0;
        self.out.push_str(text);
    }

    fn indent(&mut self, indent: usize) {
        if !self.indention || self.column > indent || (self.column == indent && !self.whitespace) {
            self.line_break("\n");
        }
        if self.column < indent {
            self.whitespace = true;
            let pad = " ".repeat(indent - self.column);
            self.put(&pad);
        }
    }

    fn indicator(
        &mut self,
        indicator: &str,
        need_whitespace: bool,
        whitespace: bool,
        indention: bool,
    ) {
        if !(self.whitespace || !need_whitespace) {
            self.put(" ");
        }
        self.put(indicator);
        self.whitespace = whitespace;
        self.indention = self.indention && indention;
    }

    /// The line breaks inside a scalar: one blank line per break, as the reference writes them.
    fn breaks(&mut self, run: &[char], indent: usize) {
        if run.first() == Some(&'\n') {
            self.line_break("\n");
        }
        for br in run {
            if *br == '\n' {
                self.line_break("\n");
            } else {
                self.line_break(&br.to_string());
            }
        }
        self.indent(indent);
    }

    fn plain(&mut self, text: &str, split: bool, indent: usize) {
        if text.is_empty() {
            return;
        }
        if !self.whitespace {
            self.put(" ");
        }
        (self.whitespace, self.indention) = (false, false);
        let chars: Vec<char> = text.chars().collect();
        let (mut spaces, mut breaks) = (false, false);
        let mut start = 0;
        for end in 0..=chars.len() {
            let ch = chars.get(end).copied();
            if spaces {
                if ch != Some(' ') {
                    if start + 1 == end && self.column > self.width && split {
                        self.indent(indent);
                        (self.whitespace, self.indention) = (false, false);
                    } else {
                        self.put_chars(&chars[start..end]);
                    }
                    start = end;
                }
            } else if breaks {
                if !ch.is_some_and(is_break) {
                    self.breaks(&chars[start..end], indent);
                    (self.whitespace, self.indention) = (false, false);
                    start = end;
                }
            } else if ch.is_none() || ch == Some(' ') || ch.is_some_and(is_break) {
                self.put_chars(&chars[start..end]);
                start = end;
            }
            if let Some(c) = ch {
                (spaces, breaks) = (c == ' ', is_break(c));
            }
        }
    }

    fn single_quoted(&mut self, text: &str, split: bool, indent: usize) {
        self.indicator("'", true, false, false);
        let chars: Vec<char> = text.chars().collect();
        let (mut spaces, mut breaks) = (false, false);
        let mut start = 0;
        for end in 0..=chars.len() {
            let ch = chars.get(end).copied();
            if spaces {
                if ch != Some(' ') {
                    if start + 1 == end
                        && self.column > self.width
                        && split
                        && start != 0
                        && end != chars.len()
                    {
                        self.indent(indent);
                    } else {
                        self.put_chars(&chars[start..end]);
                    }
                    start = end;
                }
            } else if breaks {
                if !ch.is_some_and(is_break) {
                    self.breaks(&chars[start..end], indent);
                    start = end;
                }
            } else if (ch.is_none() || matches!(ch, Some(' ' | '\'')) || ch.is_some_and(is_break))
                && start < end
            {
                self.put_chars(&chars[start..end]);
                start = end;
            }
            if ch == Some('\'') {
                self.put("''");
                start = end + 1;
            }
            if let Some(c) = ch {
                (spaces, breaks) = (c == ' ', is_break(c));
            }
        }
        self.indicator("'", false, false, false);
    }

    fn double_quoted(&mut self, text: &str, split: bool, indent: usize) {
        self.indicator("\"", true, false, false);
        let chars: Vec<char> = text.chars().collect();
        let mut start = 0;
        for end in 0..=chars.len() {
            let ch = chars.get(end).copied();
            if ch.is_none_or(|c| matches!(c, '"' | '\\') || !(' '..='~').contains(&c)) {
                if start < end {
                    self.put_chars(&chars[start..end]);
                    start = end;
                }
                if let Some(c) = ch {
                    let escape = match c {
                        '\0' => "\\0".to_string(),
                        '\u{7}' => "\\a".to_string(),
                        '\u{8}' => "\\b".to_string(),
                        '\t' => "\\t".to_string(),
                        '\n' => "\\n".to_string(),
                        '\u{b}' => "\\v".to_string(),
                        '\u{c}' => "\\f".to_string(),
                        '\r' => "\\r".to_string(),
                        '\u{1b}' => "\\e".to_string(),
                        '"' => "\\\"".to_string(),
                        '\\' => "\\\\".to_string(),
                        '\u{85}' => "\\N".to_string(),
                        '\u{a0}' => "\\_".to_string(),
                        '\u{2028}' => "\\L".to_string(),
                        '\u{2029}' => "\\P".to_string(),
                        c if (c as u32) <= 0xff => format!("\\x{:02X}", c as u32),
                        c if (c as u32) <= 0xffff => format!("\\u{:04X}", c as u32),
                        c => format!("\\U{:08X}", c as u32),
                    };
                    self.put(&escape);
                    start = end + 1;
                }
            }
            if 0 < end
                && end + 1 < chars.len()
                && (ch == Some(' ') || start >= end)
                && (self.column + end) as isize - start as isize > self.width as isize
                && split
            {
                let mut piece: String = chars[start.min(end)..end].iter().collect();
                piece.push('\\');
                if start < end {
                    start = end;
                }
                self.put(&piece);
                self.indent(indent);
                (self.whitespace, self.indention) = (false, false);
                if chars.get(start) == Some(&' ') {
                    self.put("\\");
                }
            }
        }
        self.indicator("\"", false, false, false);
    }

    fn scalar(&mut self, text: &str, is_text: bool, place: Place, indent: usize) {
        let key = place == Place::Key;
        let analysis = analyze(text);
        let split = !key;
        let implicit = if is_text { plain_is_text(text) } else { true };
        if implicit && !(key && (analysis.empty || analysis.multiline)) && analysis.block_plain {
            self.plain(text, split, indent);
        } else if analysis.single_quoted && !(key && analysis.multiline) {
            self.single_quoted(text, split, indent);
        } else {
            self.double_quoted(text, split, indent);
        }
    }

    /// One node. `indent` is the indentation of the collection it sits in (`None` at the top).
    fn node(&mut self, value: &Value, place: Place, indent: Option<usize>) {
        let deeper = |flow: bool| match indent {
            None => {
                if flow {
                    2
                } else {
                    0
                }
            }
            Some(i) => i + 2,
        };
        match value {
            Value::Array(items) if items.is_empty() => {
                self.indicator("[", true, true, false);
                self.indicator("]", false, false, false);
            }
            Value::Object(map) if map.is_empty() => {
                self.indicator("{", true, true, false);
                self.indicator("}", false, false, false);
            }
            Value::Array(items) => {
                let indentless = place == Place::Value && !self.indention;
                let own = if indentless {
                    indent.unwrap_or(0)
                } else {
                    deeper(false)
                };
                for item in items {
                    self.indent(own);
                    self.indicator("-", true, false, true);
                    self.node(item, Place::Item, Some(own));
                }
            }
            Value::Object(map) => {
                let own = deeper(false);
                for (k, v) in map {
                    self.indent(own);
                    let simple =
                        k.chars().count() < 128 && !k.is_empty() && !k.chars().any(is_break);
                    if simple {
                        self.scalar(k, true, Place::Key, own + 2);
                        self.indicator(":", false, false, false);
                    } else {
                        self.indicator("?", true, false, true);
                        self.scalar(k, true, Place::Value, own + 2);
                        self.indent(own);
                        self.indicator(":", true, false, true);
                    }
                    self.node(v, Place::Value, Some(own));
                }
            }
            Value::String(s) => self.scalar(s, true, place, deeper(true)),
            Value::Null => self.scalar("null", false, place, deeper(true)),
            Value::Bool(b) => self.scalar(
                if *b { "true" } else { "false" },
                false,
                place,
                deeper(true),
            ),
            Value::Number(n) => {
                let text = match n.as_f64().filter(|_| n.is_f64()) {
                    Some(f) if f.is_nan() => ".nan".to_string(),
                    Some(f) if f.is_infinite() => {
                        if f > 0.0 { ".inf" } else { "-.inf" }.to_string()
                    }
                    Some(f) => {
                        let text = crate::py::float_repr(f).to_lowercase();
                        if !text.contains('.') && text.contains('e') {
                            text.replacen('e', ".0e", 1)
                        } else {
                            text
                        }
                    }
                    None => crate::py::repr(value),
                };
                self.scalar(&text, false, place, deeper(true));
            }
        }
    }
}

/// Write a document in block style, keys in the order given (`yaml.safe_dump(data, sort_keys=False)`).
pub fn dump(value: &Value) -> String {
    dump_width(value, 80)
}

/// [`dump`] with the width at which long texts are folded (`yaml.safe_dump(..., width=n)`).
pub fn dump_width(value: &Value, width: usize) -> String {
    let mut e = Emitter {
        out: String::new(),
        column: 0,
        width,
        whitespace: true,
        indention: true,
    };
    e.node(value, Place::Root, None);
    let open_ended = match value {
        Value::String(s) => plain_is_text(s) && analyze(s).block_plain && !s.is_empty(),
        Value::Array(_) | Value::Object(_) => false,
        _ => true,
    };
    e.line_break("\n");
    if open_ended {
        e.out.push_str("...\n"); // a bare scalar is closed explicitly, as the reference closes it
    }
    e.out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn plain_scalars_are_typed_as_pyyaml_types_them() {
        let doc = "a: yes\nb: 'yes'\nc: 010\nd: 1_000\ne: 1e3\nf: 1.5\ng: ~\nh: 2026-11-13\ni: off\nj: 0x1F\nk: \"12\"\nl: 1:30\nm: -7\nn: .5\no: 1.0e+3\np:\nq: never\n";
        assert_eq!(
            load(doc).unwrap(),
            json!({"a": true, "b": "yes", "c": 8, "d": 1000, "e": "1e3", "f": 1.5, "g": null, "h": "2026-11-13", "i": false,
                   "j": 31, "k": "12", "l": 90, "m": -7, "n": 0.5, "o": 1000.0, "p": null, "q": "never"})
        );
    }

    #[test]
    fn collections_keys_and_merges_load() {
        let doc = "base: &b {x: 1, y: 2}\nuse:\n  <<: *b\n  y: 3\nlist:\n  - a\n  - {k: v}\n1: one\ntext: |\n  two\n  lines\n";
        assert_eq!(
            load(doc).unwrap(),
            json!({"base": {"x": 1, "y": 2}, "use": {"y": 3, "x": 1}, "list": ["a", {"k": "v"}], "1": "one", "text": "two\nlines\n"})
        );
        assert_eq!(load("").unwrap(), Value::Null);
        assert!(load("a: [1, 2").is_err());
    }

    #[test]
    fn documents_are_written_as_pyyaml_writes_them() {
        // tests/make_yaml_fixture.py asks PyYAML how it spells some 1,800 documents: every kind of
        // text that needs quoting, long texts that are folded at the width, nested collections, and
        // the kit's own lock.yaml.
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/yaml_dump.json");
        let text = std::fs::read_to_string(path)
            .expect("tests/fixtures/yaml_dump.json (python3 tests/make_yaml_fixture.py writes it)");
        let cases: Vec<Value> = serde_json::from_str(&text).expect("the fixture is JSON");
        assert!(cases.len() > 1000);
        let mut wrong = Vec::new();
        for case in &cases {
            let width = case["width"].as_u64().unwrap() as usize;
            let ours = dump_width(&case["value"], width);
            if ours != case["text"].as_str().unwrap() {
                wrong.push(format!(
                    "width {width} value {}\n--- pyyaml\n{}--- ours\n{ours}",
                    case["value"],
                    case["text"].as_str().unwrap()
                ));
            }
        }
        assert!(
            wrong.is_empty(),
            "{} of {} differ. The first:\n{}",
            wrong.len(),
            cases.len(),
            wrong[..wrong.len().min(3)].join("\n")
        );
    }

    #[test]
    fn what_is_written_reads_back_the_same() {
        let v = json!({"memory": {"backend": "obsidian", "vault": "C:\\Users\\me\\My Vault", "folder": "Council Loop/shop",
                       "on": "yes", "n": 3, "ratio": 0.5, "none": null, "flag": true, "empty": [], "tags": ["a b", "x: y", "→"]},
                       "members": {"research": {"seats": 1, "beats": [{"name": "Regulation", "text": "What the law says about it, in full."}]}}});
        let text = dump(&v);
        assert_eq!(load(&text).unwrap(), v, "{text}");
        assert!(text.starts_with("memory:\n  backend: obsidian\n"));
        assert!(
            text.contains("  'on': 'yes'\n") && text.contains("  tags:\n  - a b\n  - 'x: y'\n"),
            "{text}"
        );
    }
}
