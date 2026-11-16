//! The rules that never depend on where a run is: tiers and their shapes, settings for the support
//! members, blinding, the vote count, the cut, the protocol check and the test locks.
//!
//! Everything here is a pure function of its inputs (plus the settings files), so it is what the
//! other surfaces (the run loop, the MCP server, CI) call when they need a rule enforced.

use crate::py::{self, Json};
use crate::val::{as_int, items, merged, Get};
use anyhow::{Context, Result};
use fancy_regex::Regex as FancyRegex;
use indexmap::IndexMap;
use regex::Regex;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// The three sizes of work, smallest first.
pub const TIERS: [&str; 3] = ["direct", "lite", "full"];
/// How much to spend on models.
pub const PROFILES: [&str; 3] = ["max", "balanced", "efficient"];
/// Model families a fan-out can draw from.
pub const FAMILIES: [&str; 6] = ["claude", "gemini", "openai", "openrouter", "open", "mix"];

/// Models count words loosely: up to 10% over a cap is a warning, more is an error.
pub const GRACE: f64 = 1.1;

/// Word caps for what seats write during the loops.
pub mod caps {
    /// One idea.
    pub const IDEA: usize = 25;
    /// The body of a candidate card.
    pub const CARD: usize = 120;
    /// A card's title.
    pub const TITLE: usize = 10;
    /// An ELI5 line.
    pub const ELI5: usize = 25;
    /// A red-team finding.
    pub const FINDING: usize = 40;
    /// A proposed fix.
    pub const FIX: usize = 30;
}

/// An error the person at the keyboard can act on. Printed as `error: ...`, exit code 2.
#[derive(Debug)]
pub struct UserError(pub String);

impl std::fmt::Display for UserError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for UserError {}

/// Stop with a message for the user.
pub fn fail<T>(message: impl Into<String>) -> Result<T> {
    Err(UserError(message.into()).into())
}

/// The shape of a council tier. `start` = candidates fused in loop 1; `keep` = cards each expert keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shape {
    /// Expert seats.
    pub experts: usize,
    /// Fewest ideas each expert must list.
    pub ideas_min: usize,
    /// Cards each expert keeps.
    pub keep: usize,
    /// Elder seats.
    pub elders: usize,
    /// Candidates fused in the first loop.
    pub start: usize,
    /// Default loop cap.
    pub loops: usize,
    /// Fewest and most rubric criteria.
    pub rubric: (usize, usize),
}

impl Shape {
    /// The shape as the run file stores it.
    pub fn to_value(&self) -> Value {
        json!({"experts": self.experts, "ideas_min": self.ideas_min, "keep": self.keep, "elders": self.elders,
               "start": self.start, "loops": self.loops, "rubric": [self.rubric.0, self.rubric.1]})
    }
}

/// The shape of `tier`; `None` for the direct tier, which has no council.
pub fn shape(tier: &str) -> Option<Shape> {
    match tier {
        "full" => Some(Shape {
            experts: 7,
            ideas_min: 13,
            keep: 3,
            elders: 3,
            start: 7,
            loops: 3,
            rubric: (5, 7),
        }),
        "lite" => Some(Shape {
            experts: 3,
            ideas_min: 5,
            keep: 2,
            elders: 1,
            start: 3,
            loops: 1,
            rubric: (3, 7),
        }),
        _ => None,
    }
}

/// Text tripwires. They can only RAISE the tier. Add your own in `.council/config.yaml`
/// (`tripwires: {name: regex}`, `tripwire_paths: {name: [globs]}`).
pub const TRIPWIRES: [(&str, &str); 7] = [
    (
        "migration",
        concat!(
            r"\b(schema|database|db|data|sql)\s+migrat\w*|\bmigrations?\b|/migrations?/|\balter\s+table\b|\bbackfill\w*",
            r"|\bmigrat\w*\s+(\w+\s+){0,3}(databases?|db|schemas?|data|tables?)\b"
        ),
    ),
    (
        "auth/security",
        concat!(
            r"\bauth(n|z)?\b|\bauthenticat\w*|\bauthoriz\w*|\boauth\w*|\bsso\b|\bsecrets?\b|\bcredentials?\b",
            r"|\bpermissions?\b|\bencrypt\w*|\bpasswords?\b|\bapi\s+keys?\b"
        ),
    ),
    (
        "infra/deploy",
        concat!(
            r"\binfra(structure)?\b|\bterraform\b|\bk8s\b|\bkubernetes\b|\bhelm\b|\.github/workflows|\bdns\b",
            r"|\bdeploy\w*\s+(\w+\s+){0,2}prod\w*|\bprod(uction)?\s+(deploy|rollout|config|database)\w*"
        ),
    ),
    (
        "data loss/irreversible",
        concat!(
            r"\b(delete|drop|truncate|purge|wipe|destroy)\w*\s+(\w+\s+){0,3}",
            r"(tables?|databases?|db|records?|rows?|buckets?|volumes?|backups?|customers?|users?|accounts?|data|prod\w*)\b",
            r"|\brm\s+-rf\b|\birreversibl\w*|\bforce[- ]push\w*"
        ),
    ),
    (
        "public API/contract",
        concat!(
            r"\bpublic\s+api\b|\bopenapi\b|\bbreaking\s+change\w*|\bapi\s+contract\b|\bsdk\s+release\b",
            r"|\bdeprecat\w*\s+(\w+\s+){0,2}(endpoints?|apis?)\b"
        ),
    ),
    (
        "external publication",
        concat!(
            r"\bpress\s+release\b|\bblog\s+post\b|\bcustomer[- ]facing\b|\bannounce\w*\b|\bnpm\s+publish\b",
            r"|\bpublic\s+(launch|announcement|release)\b|\bpublish\w*\s+(\w+\s+){0,3}(publicly|externally|package|release|blog|site)\b"
        ),
    ),
    (
        "architecture/strategy",
        concat!(
            r"\barchitectur\w*|\brfc\b|\bstrateg\w*|\bproposal\b|\bdesign\s+doc\b|\bplatform\s+choice\b",
            r"|\bvendor\s+(choice|selection)\b|\broadmap\b"
        ),
    ),
];

/// Path tripwires: touching these files raises the tier whatever the ask says.
pub const TRIPWIRE_PATHS: [(&str, &[&str]); 3] = [
    ("migration", &["*/migrations/*", "migrations/*", "*.sql"]),
    (
        "auth/security",
        &["*/auth/*", "auth/*", "*secret*", "*.pem", "*credentials*"],
    ),
    (
        "infra/deploy",
        &[
            "infra/*",
            "terraform/*",
            "*.tf",
            ".github/workflows/*",
            "k8s/*",
            "helm/*",
            "Dockerfile*",
        ],
    ),
];

// ----------------------------------------------------------------------------- helpers

/// A text file as UTF-8 with `\n` line ends, whatever the machine that wrote it. A byte-order mark is
/// tolerated (Windows PowerShell's `>` writes UTF-16, some editors put a mark before UTF-8) and a
/// stray byte is replaced, so a file made on another machine loads instead of ending the run.
///
/// Only a plain file of a sane size is read (links are followed): a device or a pipe never ends,
/// and a settings file that is one would hang the tool that opens it.
pub fn read_text(path: &Path) -> std::io::Result<String> {
    use std::io::{Error, ErrorKind};
    let meta = std::fs::metadata(path)?;
    if !meta.is_file() {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "it is not a plain file",
        ));
    }
    if meta.len() > crate::safe::MAX_READ {
        return Err(Error::new(
            ErrorKind::InvalidData,
            format!(
                "it is {} bytes; the limit is {}",
                py::commas(meta.len() as i64),
                py::commas(crate::safe::MAX_READ as i64)
            ),
        ));
    }
    Ok(decode(&std::fs::read(path)?))
}

/// A file the person named on the command line: read whatever it is (a pipe from the shell, say).
pub fn read_named(path: &Path) -> std::io::Result<String> {
    Ok(decode(&std::fs::read(path)?))
}

/// The text in `data` (see [`read_text`]).
pub fn decode(data: &[u8]) -> String {
    let text = match data {
        [0xff, 0xfe, rest @ ..] => utf16(rest, u16::from_le_bytes),
        [0xfe, 0xff, rest @ ..] => utf16(rest, u16::from_be_bytes),
        [0xef, 0xbb, 0xbf, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        _ => String::from_utf8_lossy(data).into_owned(),
    };
    if text.contains("\r\n") {
        text.replace("\r\n", "\n")
    } else {
        text
    }
}

fn utf16(data: &[u8], unit: fn([u8; 2]) -> u16) -> String {
    let units = data.chunks_exact(2).map(|pair| unit([pair[0], pair[1]]));
    char::decode_utf16(units)
        .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

/// Write a text file of this tool's own as UTF-8, creating the folders above it. Only where this
/// tool may write, and never through a link (see [`crate::safe::guard`]).
pub fn write_text(path: &Path, text: &str) -> Result<()> {
    crate::safe::write_text(path, text)
}

/// Read a YAML file.
pub fn load_yaml(path: &Path) -> Result<Value> {
    let text = read_text(path).with_context(|| format!("cannot read {}", path.display()))?;
    crate::yaml::load(&text).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))
}

/// `path.resolve()`: absolute, links followed, `.` and `..` folded. Works for paths that do not exist yet.
pub fn resolve(path: &Path) -> PathBuf {
    if let Ok(real) = dunce::canonicalize(path) {
        return real;
    }
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut out = PathBuf::new();
    for part in absolute.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    match (out.parent().map(dunce::canonicalize), out.file_name()) {
        (Some(Ok(parent)), Some(name)) => parent.join(name),
        _ => out,
    }
}

/// The current directory, resolved.
pub fn cwd() -> PathBuf {
    resolve(&std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

/// Candidate labels: A..Z, then AA, AB, ...
pub fn labels_for(n: usize) -> Vec<String> {
    (0..n)
        .map(|i| {
            let (q, r) = (i / 26, i % 26);
            let mut label = String::new();
            if q > 0 {
                label.push((b'A' + (q as u8) - 1) as char);
            }
            label.push((b'A' + r as u8) as char);
            label
        })
        .collect()
}

/// Your own settings file: `~/.council/config.yaml`, or under `COUNCIL_HOME`.
pub fn user_config_path() -> PathBuf {
    let home = match std::env::var_os("COUNCIL_HOME").filter(|v| !v.is_empty()) {
        Some(dir) => py::plain(dir),
        None => home_dir().join(".council"),
    };
    home.join("config.yaml")
}

/// The user's home folder.
pub fn home_dir() -> PathBuf {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Nearest ancestor of `start` holding `.council/` or `.git`; else `start` itself. Your own settings
/// folder (`~/.council`) does not count: it must not turn the home directory into the project for
/// every folder beneath it that has no `.git` yet.
pub fn project_root_from(start: &Path) -> PathBuf {
    let start = resolve(start);
    let own = user_config_path().parent().map(resolve);
    let home_own = resolve(&home_dir().join(".council"));
    for cand in start.ancestors() {
        let council = cand.join(".council");
        let is_project_council = council.is_dir() && {
            let resolved = resolve(&council);
            Some(&resolved) != own.as_ref() && resolved != home_own
        };
        if cand.join(".git").exists() || is_project_council {
            return cand.to_path_buf();
        }
    }
    start
}

/// The project the command was run in.
pub fn project_root() -> PathBuf {
    project_root_from(&cwd())
}

/// The project's `.council/config.yaml` as an object; empty when it is missing or broken (a broken
/// optional file must not break the gate).
pub fn config(root: &Path) -> Value {
    let path = root.join(".council").join("config.yaml");
    if !path.exists() {
        return json!({});
    }
    match load_yaml(&path) {
        Ok(value) if py::truthy(&value) => value,
        _ => json!({}),
    }
}

/// Who wrote a card: `authors` (a list) or `author` (one name).
pub fn authors_of(card: &Value) -> Vec<String> {
    let a = match card.get("authors") {
        Some(v) => v,
        None => card.at("author"),
    };
    match a {
        Value::Null => Vec::new(),
        Value::String(s) => vec![s.clone()],
        Value::Array(list) => list.iter().map(py::to_str).collect(),
        other => vec![py::to_str(other)],
    }
}

// ----------------------------------------------------------------------------- gate pieces

/// The built-in tripwires plus the project's own.
pub struct Tripwires {
    /// Name -> pattern, matched against the ask with letter case ignored.
    pub text: IndexMap<String, Regex>,
    /// The names in `text` whose pattern is a built-in one. Those are built as ASCII patterns and
    /// are matched against [`ascii_reading`] of the ask.
    pub built_in: BTreeSet<String>,
    /// Name -> globs, matched against the touched paths.
    pub paths: IndexMap<String, Vec<String>>,
    /// What was wrong with the project's own entries (each was ignored).
    pub problems: Vec<String>,
}

/// The built-in tripwires plus the ones in the project's `.council/config.yaml`, and what was
/// wrong with the project's. A pattern from a settings file must be one that matches in linear
/// time (see [`crate::safe::simple_pattern`]): the file comes with the repository, and its author
/// is not always a friend.
pub fn tripwire_rules(root: &Path) -> Tripwires {
    // The built-in patterns are compiled once for the life of the process (a server answers many
    // calls), and as ASCII patterns: `\w` over all of Unicode, some thirty times over, cost more to
    // build than everything else the gate does. They still mean what they would mean over all of
    // Unicode, because the ask is read through `ascii_reading` first.
    static BUILT_IN: LazyLock<Vec<(&'static str, Regex)>> = LazyLock::new(|| {
        TRIPWIRES
            .iter()
            .map(|(name, rx)| {
                let built = regex::RegexBuilder::new(rx)
                    .case_insensitive(true)
                    .unicode(false)
                    .build()
                    .expect("the built-in tripwires are valid");
                (*name, built)
            })
            .collect()
    });
    let cfg = config(root);
    let mut text: IndexMap<String, Regex> = BUILT_IN
        .iter()
        .map(|(name, rx)| ((*name).to_string(), rx.clone()))
        .collect();
    let mut built_in: BTreeSet<String> = text.keys().cloned().collect();
    let mut paths: IndexMap<String, Vec<String>> = TRIPWIRE_PATHS
        .iter()
        .map(|(k, v)| {
            (
                (*k).to_string(),
                v.iter().map(|g| (*g).to_string()).collect(),
            )
        })
        .collect();
    let mut problems: Vec<String> = Vec::new();
    match cfg.at("tripwires") {
        Value::Null => {}
        Value::Object(own) => {
            for (name, rx) in own {
                let found = match crate::safe::simple_pattern(rx) {
                    Some(why) => Err(why),
                    None => crate::safe::project_pattern(rx.as_str().unwrap_or_default())
                        .map_err(|_| "it is not a valid pattern".to_string()),
                };
                match found {
                    Ok(re) => {
                        built_in.remove(name); // the project's own pattern replaces ours
                        text.insert(name.clone(), re);
                    }
                    Err(why) => problems.push(format!(
                        "tripwire {} in .council/config.yaml was ignored: {why}",
                        py::str_repr(&crate::safe::shown(name))
                    )),
                }
            }
        }
        _ => problems.push(
            "tripwires in .council/config.yaml must map a name to a pattern: ignored".to_string(),
        ),
    }
    match cfg.at("tripwire_paths") {
        Value::Null => {}
        Value::Object(own) => {
            for (name, globs) in own {
                let list: Option<Vec<String>> = match globs {
                    Value::String(one) => Some(vec![one.clone()]),
                    Value::Array(many) if !many.is_empty() => many
                        .iter()
                        .map(|g| g.as_str().map(str::to_string))
                        .collect(),
                    _ => None,
                };
                match list {
                    Some(list) if list.iter().all(|g| g.len() <= 200) && list.len() <= 100 => {
                        paths.insert(name.clone(), list);
                    }
                    _ => problems.push(format!(
                        "tripwire_paths {} in .council/config.yaml was ignored: it must be a list of globs",
                        py::str_repr(&crate::safe::shown(name))
                    )),
                }
            }
        }
        _ => problems.push(
            "tripwire_paths in .council/config.yaml must map a name to a list of globs: ignored"
                .to_string(),
        ),
    }
    Tripwires {
        text,
        built_in,
        paths,
        problems,
    }
}

/// A text as the built-in tripwires read it. Their patterns are ASCII patterns: ASCII words, `\w`,
/// `\s` and `\b`. Read this way they match exactly where the same patterns over all of Unicode
/// would match the text itself. Every word character outside ASCII (a letter, a digit, a mark) is
/// read as `_`, and every space outside ASCII as a tab: a word character and a space that no
/// pattern spells out. The two characters outside ASCII that are an ASCII letter when case is
/// ignored (the Kelvin sign, the long s) are read as that letter. Whatever else lies outside
/// ASCII is neither a word character nor a space in either reading, and stays.
pub fn ascii_reading(text: &str) -> std::borrow::Cow<'_, str> {
    if text.is_ascii() {
        return std::borrow::Cow::Borrowed(text);
    }
    let read = |c: char| match c {
        c if c.is_ascii() => c,
        '\u{212A}' => 'k',
        '\u{017F}' => 's',
        c if c.is_whitespace() => '\t',
        c if regex_syntax::try_is_word_character(c).unwrap_or(false) => '_',
        c => c,
    };
    std::borrow::Cow::Owned(text.chars().map(read).collect())
}

/// Names of the tripwires the ask or the touched paths set off.
pub fn tripwires_hit(task: &str, paths: &[String], root: &Path) -> Vec<String> {
    let rules = tripwire_rules(root);
    let plain = ascii_reading(task);
    let mut hits: Vec<String> = Vec::new();
    for (name, re) in &rules.text {
        let text = if rules.built_in.contains(name) {
            &*plain
        } else {
            task
        };
        if re.is_match(text) {
            hits.push(name.clone());
        }
    }
    for (name, globs) in &rules.paths {
        if !hits.contains(name)
            && paths
                .iter()
                .any(|p| globs.iter().any(|g| py::fnmatch(p, g)))
        {
            hits.push(name.clone());
        }
    }
    hits
}

/// Candidates at the start, then survivors after each loop: cut 2 per loop, end at 1.
pub fn schedule_for(start: usize, loops: usize) -> Vec<usize> {
    let mut out = vec![start];
    let mut cur = start;
    for i in 0..loops {
        let remaining = loops - i - 1;
        cur = if remaining == 0 {
            1
        } else {
            (remaining + 1).max(cur.saturating_sub(2))
        };
        out.push(cur);
    }
    out
}

// ----------------------------------------------------------------------------- support members (settings)
// The researchers and the mapper are options, not fixtures. Every value below can be set by a flag,
// by `members:` in the project's .council/config.yaml, or by `members:` in your own
// ~/.council/config.yaml. A flag beats the project, the project beats your defaults.

/// Seat ids for researchers.
pub const RESEARCH_IDS: [&str; 4] = ["RA", "RB", "RC", "RD"];

/// The default beats, in the order seats are filled.
pub const BEATS: [(&str, &str); 4] = [
    ("Papers and primary research",
     "Peer-reviewed papers and preprints (arXiv, ACL Anthology, OpenReview, Semantic Scholar), benchmarks and \
      their leaderboards, replications, surveys and standards. Find what was measured, on what data, and \
      whether it held up when others tried it."),
    ("Practice in the field",
     "How teams really do it: engineering blogs and architecture write-ups from companies, postmortems, \
      official documentation and changelogs, conference talks, and open-source repositories. Find what runs \
      in production, what broke, and what it cost."),
    ("The case against",
     "How this goes wrong: incident reports and postmortems, critical reviews, negative results, replications \
      that did not hold, security advisories, and the best arguments of people who chose otherwise. Find how \
      often it fails, how badly, and what the failure cost."),
    ("The project's own record",
     "What this project already knows: its repository history, issues and pull requests, design notes, \
      runbooks, the data and dashboards you can reach, and earlier council runs in the shared memory. Find \
      what was tried here before and what happened."),
];

/// The beat of a lone researcher.
pub const ONE_BEAT: (&str, &str) = (
    "Papers and practice",
    "Both the research literature (papers, benchmarks, replications) and how teams really do it \
     (engineering posts, official documentation, postmortems, open-source repositories). Find what was \
     measured and what runs in production, and where the two disagree.",
);

const MEMBER_CHOICES: [(&str, &[&str]); 4] = [
    ("research.later_loops", &["on_request", "always", "never"]),
    ("research.model", &["auto", "chair"]),
    ("map.when", &["stale", "always", "never"]),
    ("map.model", &["chair", "expert"]),
];
const MEMBER_RANGES: [(&str, (i64, i64)); 6] = [
    ("research.seats", (0, 4)),
    ("research.budget.lite", (1, 40)),
    ("research.budget.full", (1, 40)),
    ("map.focus_tokens", (0, 4000)),
    ("map.skeleton_lines", (100, 2000)),
    ("digest", (0, 40)),
];

fn member_defaults() -> IndexMap<String, Value> {
    [
        ("research.seats", json!(2)),
        ("research.budget.lite", json!(10)),
        ("research.budget.full", json!(15)),
        ("research.later_loops", json!("on_request")),
        ("research.model", json!("auto")),
        ("research.beats", json!([])),
        ("map.when", json!("stale")),
        ("map.model", json!("chair")),
        ("map.focus_tokens", json!(700)),
        ("map.skeleton_lines", json!(420)),
        ("map.path", json!("")),
        ("digest", json!(14)),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

/// One layer's `members:` section as flat `{dotted key: value}`, shorthands spelled out.
fn members_layer(raw: &Value) -> IndexMap<String, Value> {
    let mut out = IndexMap::new();
    if !raw.is_object() {
        return out;
    }
    match raw.at("research") {
        Value::Bool(b) => {
            out.insert("research.seats".to_string(), json!(if *b { 2 } else { 0 }));
        }
        r if as_int(r).is_some() => {
            out.insert("research.seats".to_string(), r.clone());
        }
        r @ Value::Object(_) => {
            for k in ["seats", "later_loops", "model", "beats"] {
                if let Some(v) = r.get(k) {
                    out.insert(format!("research.{k}"), v.clone());
                }
            }
            match r.at("budget") {
                b if as_int(b).is_some() => {
                    out.insert("research.budget.lite".to_string(), b.clone());
                    out.insert("research.budget.full".to_string(), b.clone());
                }
                b @ Value::Object(_) => {
                    for (k, v) in items(b) {
                        if k == "lite" || k == "full" {
                            out.insert(format!("research.budget.{k}"), v.clone());
                        }
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }
    match raw.at("map") {
        Value::Bool(b) => {
            out.insert(
                "map.when".to_string(),
                json!(if *b { "stale" } else { "never" }),
            );
        }
        m @ Value::String(_) => {
            out.insert("map.when".to_string(), m.clone());
        }
        m @ Value::Object(_) => {
            for k in ["when", "model", "focus_tokens", "skeleton_lines", "path"] {
                if let Some(v) = m.get(k) {
                    out.insert(format!("map.{k}"), v.clone());
                }
            }
        }
        _ => {}
    }
    if let Some(v) = raw.get("digest") {
        out.insert("digest".to_string(), v.clone());
    }
    out
}

/// What a flag asked for the researchers: a count, or simply on / off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResearchFlag {
    /// `--research N`.
    Seats(i64),
    /// On (two researchers) or off.
    On(bool),
}

/// Settings given on the command line (or through the MCP tools). `None` = as configured.
#[derive(Clone, Debug, Default)]
pub struct MemberFlags {
    /// How many researchers, or on / off.
    pub research: Option<ResearchFlag>,
    /// When the mapper runs: `stale`, `always` or `never`.
    pub mapper: Option<String>,
    /// Searches per researcher, for both tiers.
    pub research_budget: Option<i64>,
    /// Research before later loops: `on_request`, `always` or `never`.
    pub research_later: Option<String>,
    /// Size of the ranked code list in packets.
    pub focus_tokens: Option<i64>,
}

/// The settings for the support members of a new run, each with where it came from.
///
/// Returns `{"research": {...}, "map": {...}, "digest": n, "origin": {dotted key: who set it},
/// "problems": [values that were refused, with the default used instead]}`.
pub fn members(root: &Path, flags: &MemberFlags) -> Value {
    let mut flat = member_defaults();
    let mut origin: IndexMap<String, String> = flat
        .keys()
        .map(|k| (k.clone(), "default".to_string()))
        .collect();
    let mut problems: Vec<String> = Vec::new();
    let user_path = user_config_path();
    let mut user = Value::Null;
    if user_path.exists() {
        match load_yaml(&user_path) {
            Ok(doc) if doc.is_object() => user = doc.at("members").clone(),
            Ok(doc) if !py::truthy(&doc) => {}
            _ => problems.push(format!(
                "{} is not valid YAML: your own defaults were ignored",
                user_path.display()
            )),
        }
    }
    let project_path = root.join(".council").join("config.yaml");
    if project_path.exists() {
        // a settings file that cannot be read is ignored, but never silently
        match load_yaml(&project_path) {
            Ok(doc) if doc.is_object() || !py::truthy(&doc) => {}
            Ok(_) => problems.push(
                ".council/config.yaml is not a mapping (key: value lines): the project's settings were ignored"
                    .to_string(),
            ),
            Err(err) => problems.push(format!(
                ".council/config.yaml could not be read ({}): the project's settings were ignored",
                py::head(&py::squash(&format!("{err:#}")), 160)
            )),
        }
        problems.extend(tripwire_rules(root).problems);
    }
    let mut flag_layer: IndexMap<String, Value> = IndexMap::new();
    match flags.research {
        Some(ResearchFlag::Seats(n)) => {
            flag_layer.insert("research.seats".into(), json!(n));
        }
        Some(ResearchFlag::On(on)) => {
            flag_layer.insert("research.seats".into(), json!(if on { 2 } else { 0 }));
        }
        None => {}
    }
    if let Some(when) = &flags.mapper {
        flag_layer.insert("map.when".into(), json!(when));
    }
    if let Some(n) = flags.research_budget {
        flag_layer.insert("research.budget.lite".into(), json!(n));
        flag_layer.insert("research.budget.full".into(), json!(n));
    }
    if let Some(later) = &flags.research_later {
        flag_layer.insert("research.later_loops".into(), json!(later));
    }
    if let Some(n) = flags.focus_tokens {
        flag_layer.insert("map.focus_tokens".into(), json!(n));
    }
    let cfg = config(root);
    let mut project: IndexMap<String, Value> = IndexMap::new();
    let legacy = cfg.at("map").at("path"); // the older place for the map's path
    if py::truthy(legacy) {
        project.insert("map.path".into(), legacy.clone());
    }
    project.extend(members_layer(cfg.at("members")));
    let layers = [
        ("your settings", members_layer(&user)),
        ("project config", project),
        ("flag", flag_layer),
    ];
    for (name, layer) in layers {
        for (key, raw) in layer {
            let mut value = raw;
            let choices = MEMBER_CHOICES
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| *v);
            let range = MEMBER_RANGES
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| *v);
            let mut ok = true;
            if let Some(allowed) = choices {
                ok = value.as_str().is_some_and(|s| allowed.contains(&s));
            } else if let Some((lo, hi)) = range {
                ok = as_int(&value).is_some_and(|n| lo <= n && n <= hi);
            } else if key == "research.beats" {
                let beats: Vec<Value> = value
                    .as_array()
                    .map(|list| {
                        list.iter()
                            .filter(|b| b.is_object())
                            .map(|b| {
                                let text = |k: &str| {
                                    if py::truthy(b.at(k)) {
                                        py::squash(&b.show(k))
                                    } else {
                                        String::new()
                                    }
                                };
                                json!({"name": text("name"), "text": text("text")})
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                ok = !beats.is_empty()
                    && beats
                        .iter()
                        .all(|b| !b.s("name").is_empty() && py::words(b.s("text")) >= 5);
                value = Value::Array(beats);
            } else if key == "map.path" {
                ok = value.as_str().is_some_and(|s| {
                    s.ends_with(".md")
                        && !Path::new(s).is_absolute()
                        && !Path::new(s).components().any(|c| c.as_os_str() == "..")
                });
            }
            if !flat.contains_key(&key) || !ok {
                let allowed = match (choices, range) {
                    (Some(c), _) => py::tuple_repr(c),
                    (_, Some((lo, hi))) => format!("({lo}, {hi})"),
                    _ => "see `council config --example`".to_string(),
                };
                let default = flat
                    .get(&key)
                    .map(py::repr)
                    .unwrap_or_else(|| "None".to_string());
                problems.push(format!(
                    "members.{key} = {} ({name}) was refused (allowed: {allowed}); using {default}",
                    py::repr(&value)
                ));
                continue;
            }
            flat.insert(key.clone(), value);
            origin.insert(key, name.to_string());
        }
    }
    let mut out = json!({"research": {"budget": {}}, "map": {}});
    for (key, value) in flat {
        let parts: Vec<&str> = key.split('.').collect();
        let mut node = &mut out;
        for part in &parts[..parts.len() - 1] {
            if !node.at(part).is_object() {
                node[*part] = json!({});
            }
            node = &mut node[*part];
        }
        node[parts[parts.len() - 1]] = value;
    }
    out["origin"] = json!(origin);
    out["problems"] = json!(problems);
    out
}

/// One beat per researcher: the project's own beats first, then the defaults. A lone researcher
/// covers papers and practice together.
pub fn beats_for(settings: &Value) -> Vec<(String, String)> {
    let research = settings.at("research");
    let n = research.int("seats").unwrap_or(0).max(0) as usize;
    let own: Vec<(String, String)> = research
        .list("beats")
        .iter()
        .map(|b| (b.s("name").to_string(), b.s("text").to_string()))
        .collect();
    if n == 1 && own.is_empty() {
        return vec![(ONE_BEAT.0.to_string(), ONE_BEAT.1.to_string())];
    }
    let mine: HashSet<&str> = own.iter().map(|b| b.0.as_str()).collect();
    let defaults: Vec<(String, String)> = BEATS
        .iter()
        .filter(|b| !mine.contains(b.0))
        .map(|b| (b.0.to_string(), b.1.to_string()))
        .collect();
    own.iter().cloned().chain(defaults).take(n).collect()
}

/// `council config`: every setting, its value and who set it.
pub fn members_report(settings: &Value, mem: &Value, root: &Path) -> Vec<String> {
    let mut lines = vec![
        "# Support members (flags beat the project's .council/config.yaml, which beats your ~/.council/config.yaml)".to_string(),
    ];
    for (key, who) in items(settings.at("origin")) {
        let mut node = settings;
        for part in key.split('.') {
            node = node.at(part);
        }
        let mut shown = if key == "research.beats" {
            match node.as_array().map(Vec::len).unwrap_or(0) {
                0 => "built in".to_string(),
                n => format!("{n} of your own"),
            }
        } else {
            py::to_str(node)
        };
        if key == "map.path" && !py::truthy(node) {
            let path = crate::codemap::map_path(root);
            shown = py::posix(path.strip_prefix(root).unwrap_or(&path));
        }
        lines.push(format!("members.{key} = {shown}    ({})", py::to_str(who)));
    }
    let who = match mem.s("source") {
        "user" => "your settings",
        "project" => "project config",
        "env" => "environment variable",
        other => other,
    };
    let beats: Vec<String> = beats_for(settings).into_iter().map(|b| b.0).collect();
    lines.push(format!(
        "beats this would seat: {}",
        if beats.is_empty() {
            "none".to_string()
        } else {
            beats.join("; ")
        }
    ));
    lines.push(String::new());
    lines.push(
        "# Shared memory (your own settings beat the project's here: a vault path is personal)"
            .to_string(),
    );
    lines.push(format!(
        "memory.backend = {}    ({who})",
        mem.show("backend")
    ));
    lines.push(format!("memory.dir = {}    ({who})", mem.show("dir")));
    if mem.has("ok") && !mem.truthy("ok") {
        lines.push(format!("PROBLEM: {}", mem.show("problem")));
    }
    lines.extend(
        settings
            .list("problems")
            .iter()
            .map(|p| format!("PROBLEM: {}", py::to_str(p))),
    );
    lines
}

/// Seat calls a run of this size makes.
pub fn estimate_calls(tier: &str, loops: usize, researchers: usize, mapper: bool) -> usize {
    let Some(s) = shape(tier) else { return 1 };
    let vote = if tier == "full" { s.experts } else { 0 }; // lite skips the shortlist vote
    let per_loop = s.experts + vote + 1 + s.experts + s.elders; // ideate, vote, red team, rank, elders
    let support = researchers + usize::from(mapper); // once, before the first ideation
    per_loop * loops + 1 + support // + the control arm
}

/// Do the elders also verify claims in this tier and profile?
pub fn elders_verify(tier: &str, profile: &str) -> bool {
    tier == "lite" || profile == "max"
}

/// The model policy (`models/policy.yaml`).
pub fn policy() -> Value {
    crate::assets::read_model("policy.yaml")
        .map(|text| (*parsed_once(&POLICY_CACHE, &text)).clone())
        .unwrap_or(Value::Null)
}

type Parsed = std::sync::Mutex<Option<(String, std::sync::Arc<Value>)>>;
static LOCK_CACHE: Parsed = std::sync::Mutex::new(None);
static POLICY_CACHE: Parsed = std::sync::Mutex::new(None);

/// A YAML text as a value, parsed once for as long as the text stays the same. The gate asks for
/// the model data several times in one command; a long-lived server sees a refreshed file at once.
fn parsed_once(cache: &Parsed, text: &str) -> std::sync::Arc<Value> {
    let mut held = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match &*held {
        Some((seen, value)) if seen == text => value.clone(),
        _ => {
            let value = std::sync::Arc::new(crate::yaml::load(text).unwrap_or(Value::Null));
            *held = Some((text.to_string(), value.clone()));
            value
        }
    }
}

/// Rough tokens for the seat calls of a run, from MEASURED per-call numbers in `models/policy.yaml`
/// (`measured_tokens_per_call`). `None` when there are no measurements. The chair's own tokens are
/// extra. The researchers and the mapper are not measured yet: they come from `assumed_tokens_per_call`.
pub fn estimate_tokens(
    tier: &str,
    loops: usize,
    policy: &Value,
    profile: &str,
    researchers: usize,
    mapper: bool,
) -> Option<i64> {
    let s = shape(tier)?;
    let m = policy.at("measured_tokens_per_call");
    let get = |k: &str| m.num(k);
    let (ideate, vote, rank, redteam, elders, control) = (
        get("ideate")?,
        get("vote")?,
        get("rank")?,
        get("redteam")?,
        get("elders")?,
        get("control")?,
    );
    let vote = if tier == "full" {
        s.experts as f64 * vote
    } else {
        0.0
    };
    let elder = if elders_verify(tier, profile) {
        m.get("elders_verify")
            .and_then(Value::as_f64)
            .unwrap_or(elders)
    } else {
        elders
    };
    let per_loop = s.experts as f64 * (ideate + rank) + vote + redteam + s.elders as f64 * elder;
    let guess = policy.at("assumed_tokens_per_call");
    let support = researchers as f64 * guess.num("research").unwrap_or(0.0)
        + if mapper {
            guess.num("map").unwrap_or(0.0)
        } else {
            0.0
        };
    Some((per_loop * loops as f64 + control + support) as i64)
}

/// `7.8M`, `870k` or `n/a`.
pub fn short_tokens(n: Option<i64>) -> String {
    match n {
        None => "n/a".to_string(),
        Some(n) if n as f64 >= 1e6 => format!("{:.1}M", n as f64 / 1e6),
        Some(n) => format!("{:.0}k", n as f64 / 1e3),
    }
}

/// Model picks for a mode and profile, from `models/lock.yaml`.
pub fn models_for(mode: &str, profile: &str) -> Value {
    let Some(text) = crate::assets::read_model("lock.yaml") else {
        return json!({"error": format!("{} missing: run `council refresh`", crate::assets::models_dir().join("lock.yaml").display())});
    };
    let lock = parsed_once(&LOCK_CACHE, &text);
    let key = if mode == "solo" || mode == "crosslab" || mode.starts_with("fanout:") {
        mode.to_string()
    } else {
        format!("fanout:{mode}")
    };
    let cfg = lock.at("configs").at(profile).at(&key);
    if cfg.is_null() {
        return json!({"error": format!("no config for profile={profile} mode={key}")});
    }
    let data = lock.at("data");
    let warnings = lock.get("warnings").cloned().unwrap_or_else(|| json!([]));
    let base = json!({"mode": key, "profile": profile, "data_collected": data.show("collected"),
                      "attribution": data.at("attribution"), "warnings": warnings});
    Value::Object(merged(&base, cfg))
}

// ----------------------------------------------------------------------------- blind
// Identity leaks: a card that says who wrote it cannot be judged blind. Three shapes only, so that
// ordinary prose ("a seat call costs", "use the rubric as a lens") does not trip the check:
//   first person ("my lens", "our card"), a claimed role ("as the cost expert"), a real seat id ("seat L3").
static LEAK_RE: LazyLock<FancyRegex> = LazyLock::new(|| {
    FancyRegex::new(concat!(
        r"(?i)\b(?:my|our)\s+(?:own\s+)?(?:lens|seat|cards?|ideas?|proposals?)\b",
        r"|\bas\s+(?:the|an?)\s+(?:[\w-]+\s+){1,3}(?:expert|seat)\b",
        r"|\b(?:seat|expert|elder|voter|lens)\s+(?-i:L\d{1,2}|RT|E[A-G]|CTRL)(?![\w-])"
    ))
    .expect("the leak pattern is valid")
});

fn wordish(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-'
}

/// Does `text` contain `token` standing on its own (not inside a longer word or a hyphenated name)?
pub fn names_token(text: &str, token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    let mut from = 0;
    while let Some(at) = text[from..].find(token) {
        let start = from + at;
        let end = start + token.len();
        let before = text[..start].chars().next_back().is_some_and(wordish);
        let after = text[end..].chars().next().is_some_and(wordish);
        if !before && !after {
            return true;
        }
        from = start + text[start..].chars().next().map_or(1, char::len_utf8);
    }
    false
}

/// The first identity leak in `text`, if any.
pub fn leak_in(text: &str) -> Option<String> {
    LEAK_RE
        .find(text)
        .ok()
        .flatten()
        .map(|m| m.as_str().to_string())
}

fn field(card: &Value, key: &str) -> String {
    match card.get(key) {
        None => String::new(),
        Some(v) => py::to_str(v),
    }
}

/// Anonymise and shuffle candidate cards. Returns the judge packet and a private mapping.
///
/// Card: `{"id", "title", "body", "eli5", "author" | "authors"}`. Authors never reach the judges.
/// `forbidden` = extra strings (seat ids, model names) that must not appear in a card.
/// `spread` = how much longer the longest body may be than the shortest (long cards win votes).
pub fn blind(
    cards: &[Value],
    seed: u64,
    max_words: usize,
    forbidden: &[String],
    spread: f64,
) -> Value {
    // a card's text goes to judges and to a terminal: control characters do not
    let cards: Vec<Value> = cards.iter().cloned().map(crate::safe::cleaned).collect();
    let cards = &cards[..];
    let (mut errors, mut warnings): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    for c in cards {
        let id = c.show("id");
        let n = if py::truthy(c.at("body")) {
            py::words(&c.show("body"))
        } else {
            0
        };
        if n as f64 > max_words as f64 * GRACE {
            errors.push(format!("{id}: body {n} words > cap {max_words}"));
        } else if n > max_words {
            warnings.push(format!("{id}: body {n} words (cap {max_words})"));
        }
        if !py::truthy(c.at("eli5")) || py::strip(&c.show("eli5")).is_empty() {
            errors.push(format!("{id}: missing ELI5"));
        }
        let text = format!(
            "{} {} {}",
            field(c, "title"),
            field(c, "body"),
            field(c, "eli5")
        );
        for token in authors_of(c).iter().chain(forbidden.iter()) {
            if names_token(&text, token) {
                errors.push(format!(
                    "{id}: identity leak: the card names '{token}'. Reword it (\"one expert\", \"another card\"): cards are judged blind"
                ));
            }
        }
        if let Some(leak) = leak_in(&text) {
            errors.push(format!("{id}: identity leak: '{leak}'. Reword it without saying who wrote or holds what: cards are judged blind"));
        }
    }
    let order = crate::pyrand::shuffled(cards.len(), seed);
    let mut mapping = Map::new();
    let mut packet_cards: Vec<Value> = Vec::new();
    for (label, idx) in labels_for(cards.len()).into_iter().zip(order) {
        let c = &cards[idx];
        mapping.insert(
            label.clone(),
            json!({"id": c.at("id"), "authors": authors_of(c)}),
        );
        packet_cards.push(
            json!({"label": label, "title": c.get("title").cloned().unwrap_or_else(|| json!("")),
                                 "body": c.get("body").cloned().unwrap_or_else(|| json!("")),
                                 "eli5": c.get("eli5").cloned().unwrap_or_else(|| json!(""))}),
        );
    }
    let lengths: Vec<usize> = packet_cards
        .iter()
        .map(|c| py::words(&c.show("body")))
        .collect();
    if let (Some(&lo), Some(&hi)) = (lengths.iter().min(), lengths.iter().max()) {
        if hi as f64 > spread * lo.max(1) as f64 {
            errors.push(format!(
                "length spread {lo}-{hi} words: equalise the cards (long ones win votes)"
            ));
        }
    }
    let text = render_packet(&packet_cards, None);
    json!({"ok": errors.is_empty(), "errors": errors, "warnings": warnings, "seed": seed, "packet": text,
           "cards": packet_cards, "mapping": mapping})
}

/// The order in which each voter reads the cards. Same labels for everyone; each voter starts at
/// its own evenly spaced offset (voter index x cards // voters), so no card is first on every ballot
/// and every card's mean position stays near the middle (LLM judges favour early positions).
pub fn seat_orders(labels: &[String], seats: &[String]) -> IndexMap<String, Vec<String>> {
    let (k, m) = (labels.len(), seats.len().max(1));
    let mut sorted: Vec<&String> = seats.iter().collect();
    sorted.sort();
    let mut out = IndexMap::new();
    for (i, seat) in sorted.into_iter().enumerate() {
        let off = if k > 0 { (i * k) / m } else { 0 };
        let order: Vec<String> = labels[off..]
            .iter()
            .chain(labels[..off].iter())
            .cloned()
            .collect();
        out.insert(seat.clone(), order);
    }
    out
}

/// The cards as the judges read them, in `order` (or as given).
pub fn render_packet(cards: &[Value], order: Option<&[String]>) -> String {
    let mut by: IndexMap<String, &Value> = IndexMap::new();
    for c in cards {
        by.insert(c.show("label"), c);
    }
    let labels: Vec<String> = match order {
        Some(o) if !o.is_empty() => o.to_vec(),
        _ => by.keys().cloned().collect(),
    };
    labels
        .iter()
        .filter_map(|l| {
            by.get(l).map(|c| {
                format!(
                    "### Candidate {l}: {}\n{}\nELI5: {}",
                    c.show("title"),
                    c.show("body"),
                    c.show("eli5")
                )
            })
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

// ----------------------------------------------------------------------------- tally + cut
static LABEL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:candidate|card|option)?\s*[:#-]?\s*([A-Za-z]{1,2})\s*[:.)-]?$")
        .expect("valid")
});

/// `'Candidate B'`, `'b'`, `'B.'` -> `'B'`. `None` when it names no card.
pub fn clean_label(raw: &Value, labels: &[String]) -> Option<String> {
    let shown = if raw.is_null() {
        String::new()
    } else {
        py::to_str(raw)
    };
    let s = py::strip(&shown);
    if labels.iter().any(|l| l == s) {
        return Some(s.to_string());
    }
    let label = LABEL_RE.captures(s)?.get(1)?.as_str().to_uppercase();
    labels.contains(&label).then_some(label)
}

/// How much of a card is the voter's: 1 for a sole author, 1/n for one of n authors, else 0.
pub fn stake(voter: &str, card: &Value) -> f64 {
    let authors = authors_of(card);
    if authors.iter().any(|a| a == voter) {
        1.0 / authors.len() as f64
    } else {
        0.0
    }
}

fn number(x: f64) -> Value {
    serde_json::Number::from_f64(x)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

/// One voter's head-to-heads: (card, card) in label order -> (+1 or -1 for which it put first, the weight).
type Judged = IndexMap<(usize, usize), (f64, f64)>;

/// What the tally needs besides the ballots.
#[derive(Default)]
pub struct TallyOptions<'a> {
    /// label -> {criterion: score}, from machine checks or judges.
    pub scores: Option<&'a Value>,
    /// label -> "pass" | "fail". A failed machine check ranks last: verify beats vote.
    pub checks: Option<&'a Value>,
    /// How many to keep; `None` = rank only.
    pub keep: Option<usize>,
    /// Labels the elders killed by majority; they rank last too.
    pub kills: &'a [String],
    /// criterion -> weight, to order wildcard claims.
    pub weights: Option<&'a Value>,
}

/// Count blind ballots as head-to-heads, weighted by stake. With complete ballots and no stakes
/// this is a Borda count.
///
/// votes:   `[{"voter": "L3", "ranking": ["B", "A", ...], "best_on": {"R1": "B"}}]`
/// mapping: label -> `{"id", "authors"}`.
///
/// A ballot is read as a set of head-to-heads ("this card above that card"). Each one counts with
/// weight 1 - |stake in one card - stake in the other|: full weight when the voter wrote neither card
/// or an equal share of both, nothing when the voter alone wrote one of them, part weight in between.
/// So nobody can vote their own card up, and a card that several voters fed still gets judged.
/// A card the voter left off the ballot is in none of that voter's head-to-heads: a gap is no opinion,
/// never a last place.
///
/// A card's score is its mean win share over every opponent. Each head-to-head starts as one win each
/// ((wins + 1) / (weight + 2)), so a card that few voters could judge cannot win by default.
/// `thin` lists head-to-heads that got less than one full vote: the vote could not judge them, so it
/// must not be what cuts either card (see [`protected`]).
///
/// agreement: Kendall's tau between voters, pooled over the head-to-heads both judged (1 = same order,
/// 0 = unrelated, -1 = opposite). `null` when no two voters judged the same head-to-head.
pub fn tally(votes: &[Value], mapping: &Value, opt: &TallyOptions) -> Value {
    let labels: Vec<String> = items(mapping).map(|(k, _)| k.clone()).collect();
    let index: IndexMap<&str, usize> = labels
        .iter()
        .enumerate()
        .map(|(i, l)| (l.as_str(), i))
        .collect();
    let k = labels.len();
    let mut wins = vec![vec![0.0f64; k]; k];
    let mut prefs: IndexMap<String, Judged> = IndexMap::new();
    let mut problems: Vec<String> = Vec::new();
    let (mut counted, mut recused, mut gaps) = (0usize, 0usize, 0usize);
    let mut best_votes: IndexMap<String, IndexMap<String, usize>> = IndexMap::new();
    for v in votes {
        let voter = v.show("voter");
        let mut ranking: Vec<usize> = Vec::new();
        for raw in v.list("ranking") {
            match clean_label(raw, &labels) {
                None => problems.push(format!("{voter}: unknown label {}", py::repr(raw))),
                Some(l) if ranking.contains(&index[l.as_str()]) => {
                    problems.push(format!("{voter}: {l} ranked twice"))
                }
                Some(l) => ranking.push(index[l.as_str()]),
            }
        }
        let mine: Vec<f64> = labels
            .iter()
            .map(|l| stake(&voter, mapping.at(l)))
            .collect();
        gaps += k - ranking.len();
        let seen = prefs.entry(voter.clone()).or_default();
        for (i, &a) in ranking.iter().enumerate() {
            for &b in &ranking[i + 1..] {
                let w = 1.0 - (mine[a] - mine[b]).abs();
                if w <= 1e-9 {
                    recused += 1; // the voter alone wrote one side: sit it out
                    continue;
                }
                wins[a][b] += w;
                counted += 1;
                let in_order = labels[a] < labels[b];
                let key = if in_order { (a, b) } else { (b, a) };
                seen.insert(key, (if in_order { 1.0 } else { -1.0 }, w));
            }
        }
        for (crit, raw) in items(v.at("best_on")) {
            if let Some(l) = clean_label(raw, &labels) {
                if mine[index[l.as_str()]] == 0.0 {
                    *best_votes
                        .entry(crit.clone())
                        .or_default()
                        .entry(l)
                        .or_insert(0) += 1;
                }
            }
        }
    }
    let weight = |a: usize, b: usize| wins[a][b] + wins[b][a];
    let m = votes.len();
    let score: Vec<f64> = (0..k)
        .map(|a| {
            if k > 1 {
                (0..k)
                    .filter(|&b| b != a)
                    .map(|b| (wins[a][b] + 1.0) / (weight(a, b) + 2.0))
                    .sum::<f64>()
                    / (k - 1) as f64
            } else {
                0.5
            }
        })
        .collect();
    let mut thin: Vec<Value> = Vec::new();
    for a in 0..k {
        for b in a + 1..k {
            if weight(a, b) < 1.0 - 1e-9 {
                thin.push(json!([labels[a], labels[b]]));
            }
        }
    }
    let failed: HashSet<usize> = opt
        .checks
        .map(|c| {
            items(c)
                .filter(|(_, r)| py::to_str(r).to_lowercase() == "fail")
                .filter_map(|(l, _)| index.get(l.as_str()).copied())
                .collect()
        })
        .unwrap_or_default();
    let failed_labels: HashSet<String> = opt
        .checks
        .map(|c| {
            items(c)
                .filter(|(_, r)| py::to_str(r).to_lowercase() == "fail")
                .map(|(l, _)| l.clone())
                .collect()
        })
        .unwrap_or_default();
    let killed: HashSet<usize> = opt
        .kills
        .iter()
        .filter_map(|l| index.get(l.as_str()).copied())
        .collect();
    let out_of_it = |l: usize| failed.contains(&l) || killed.contains(&l);
    let mut ranked: Vec<usize> = (0..k).collect();
    ranked.sort_by(|&a, &b| {
        out_of_it(a)
            .cmp(&out_of_it(b))
            .then(
                score[b]
                    .partial_cmp(&score[a])
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
            .then(labels[a].cmp(&labels[b]))
    });
    let ties: Vec<Value> = ranked
        .windows(2)
        .filter(|w| (score[w[0]] - score[w[1]]).abs() < 1e-9 && out_of_it(w[0]) == out_of_it(w[1]))
        .map(|w| json!([labels[w[0]], labels[w[1]]]))
        .collect();
    let (mut num, mut den) = (0.0f64, 0.0f64);
    let names: Vec<&String> = prefs.keys().collect();
    for (i, u) in names.iter().enumerate() {
        for w_ in &names[i + 1..] {
            for (key, (su, wu)) in &prefs[*u] {
                if let Some((sw, ww)) = prefs[*w_].get(key) {
                    let both = wu.min(*ww);
                    num += su * sw * both;
                    den += both;
                }
            }
        }
    }
    let agreement = if den != 0.0 {
        Some(py::round(num / den, 2))
    } else {
        None
    };
    let mut best_on = Map::new();
    for (crit, counts) in &best_votes {
        let top = counts.values().copied().max().unwrap_or(0);
        let leaders: Vec<&String> = counts
            .iter()
            .filter(|(_, c)| **c == top)
            .map(|(l, _)| l)
            .collect();
        if leaders.len() == 1 && top >= 2.max((0.4 * m as f64).ceil() as usize) {
            best_on.insert(crit.clone(), json!(leaders[0]));
        }
    }
    let ranking: Vec<Value> = ranked
        .iter()
        .map(|&l| {
            let label = &labels[l];
            let check = opt.checks.and_then(|c| c.get(label)).cloned().unwrap_or_else(|| json!("n/a"));
            let mut row = json!({"label": label, "id": mapping.at(label).at("id"), "score": number(py::round(score[l], 3)), "check": check});
            if killed.contains(&l) {
                row["killed"] = json!(true);
            }
            row
        })
        .collect();
    let note = match agreement {
        None => "not measurable: no two voters judged the same head-to-head",
        Some(a) if a >= 0.5 => "strong",
        Some(a) if a >= 0.2 => "moderate",
        Some(_) => "weak: treat close ranks as ties",
    };
    let mut result = json!({"ranking": ranking, "ballots": m, "head_to_heads": counted, "recused": recused, "gaps": gaps,
                            "thin": thin, "ties": ties, "agreement": agreement.map(number), "agreement_note": note,
                            "best_on": best_on, "problems": problems});
    if let Some(keep) = opt.keep {
        let order: Vec<String> = ranked.iter().map(|&l| labels[l].clone()).collect();
        let mut barred = failed_labels;
        barred.extend(opt.kills.iter().cloned());
        let empty = json!({});
        let cut = cut(
            &order,
            keep,
            opt.scores.unwrap_or(&empty),
            &barred,
            &result["best_on"].clone(),
            opt.weights,
        );
        for (key, value) in items(&cut) {
            result[key.as_str()] = value.clone();
        }
    }
    result
}

/// Cards below a cut line that the vote could not judge against a card above it. A vote may never
/// cut what it could not judge: these stay in (on the shortlist, in the elder packet, among the
/// survivors) and the run logs a deviation. `barred` = cards that failed a check or were killed.
pub fn protected(
    order: &[String],
    keep: usize,
    thin: &Value,
    barred: &HashSet<String>,
) -> Vec<String> {
    let keep = keep.min(order.len());
    let above: HashSet<&String> = order[..keep].iter().collect();
    let pairs: HashSet<BTreeSet<String>> = thin
        .as_array()
        .into_iter()
        .flatten()
        .map(|p| {
            crate::val::strings(p)
                .into_iter()
                .collect::<BTreeSet<String>>()
        })
        .collect();
    order[keep..]
        .iter()
        .filter(|c| !barred.contains(*c))
        .filter(|c| {
            above.iter().any(|a| {
                pairs.contains(
                    &[(*c).clone(), (*a).clone()]
                        .into_iter()
                        .collect::<BTreeSet<String>>(),
                )
            })
        })
        .cloned()
        .collect()
}

/// Keep the top `keep`. When keep >= 3, swap in ONE wildcard: a cut candidate that is the single
/// best on some rubric criterion (keeps diversity alive, as in GEPA / MAP-Elites). The wildcard
/// replaces the weakest survivor, never the top two. Failed or killed candidates never return.
/// The final cut (keep < 3) is never touched: a wildcard must not replace the winner.
pub fn cut(
    ranked: &[String],
    keep: usize,
    scores: &Value,
    failed: &HashSet<String>,
    best_on: &Value,
    weights: Option<&Value>,
) -> Value {
    let keep = keep.min(ranked.len());
    let mut survivors: Vec<String> = ranked[..keep].to_vec();
    let mut dropped: Vec<String> = ranked[keep..].to_vec();
    let mut wildcard = Value::Null;
    if keep >= 3 && !dropped.is_empty() {
        let mut claims: IndexMap<String, String> = items(best_on)
            .map(|(k, v)| (k.clone(), py::to_str(v)))
            .collect(); // criterion -> label
        let criteria: BTreeSet<String> = items(scores)
            .flat_map(|(_, s)| items(s).map(|(c, _)| c.clone()))
            .collect();
        for crit in criteria {
            let vals: Vec<(&String, f64)> = items(scores)
                .filter_map(|(l, s)| s.at(&crit).as_f64().map(|v| (l, v)))
                .collect();
            if let Some(top) = vals
                .iter()
                .map(|(_, v)| *v)
                .fold(None, |best: Option<f64>, v| {
                    Some(best.map_or(v, |b| b.max(v)))
                })
            {
                let leaders: Vec<&String> = vals
                    .iter()
                    .filter(|(_, v)| *v == top)
                    .map(|(l, _)| *l)
                    .collect();
                if leaders.len() == 1 {
                    claims.entry(crit).or_insert_with(|| leaders[0].clone());
                }
            }
        }
        let weight_of = |c: &str| weights.and_then(|w| w.at(c).as_f64()).unwrap_or(0.0);
        let mut order: Vec<&String> = claims.keys().collect();
        order.sort_by(|a, b| {
            weight_of(b)
                .partial_cmp(&weight_of(a))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.cmp(b))
        });
        for crit in order {
            let label = &claims[crit];
            if dropped.contains(label) && !failed.contains(label) {
                wildcard = json!({"label": label, "criterion": crit, "replaces": survivors[survivors.len() - 1]});
                dropped.retain(|l| l != label);
                if let Some(last) = survivors.pop() {
                    dropped.push(last);
                }
                survivors.push(label.clone());
                break;
            }
        }
    }
    json!({"survivors": survivors, "eliminated": dropped, "wildcard": wildcard})
}

// ----------------------------------------------------------------------------- check a run

/// Validate a run ledger (`run.json`) against the protocol.
pub fn check_run(run: &Value) -> Value {
    let (mut errs, mut warns): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    let tier = run.get("tier").and_then(Value::as_str).unwrap_or("full");
    if tier == "direct" {
        return json!({"ok": true, "errors": [], "warnings": []});
    }
    let base = shape(tier)
        .or_else(|| shape("full"))
        .expect("the full tier has a shape");
    let own = run.at("shape");
    let shape_n =
        |key: &str, default: usize| own.int(key).map(|n| n.max(0) as usize).unwrap_or(default);
    let (ideas_min, elders, start, loops_default) = (
        shape_n("ideas_min", base.ideas_min),
        shape_n("elders", base.elders),
        shape_n("start", base.start),
        shape_n("loops", base.loops),
    );
    let max_loops = run
        .int("max_loops")
        .map(|n| n.max(0) as usize)
        .unwrap_or(loops_default);
    let loops = run.list("loops");
    if loops.len() > max_loops {
        errs.push(format!(
            "{} loops > max {max_loops} (only the requester can raise the cap)",
            loops.len()
        ));
    }
    if !run.truthy("ask") && !run.truthy("original_ask") {
        errs.push(
            "missing ask (elders must see the requester's words, not only the brief)".to_string(),
        );
    }
    let brief = run.at("brief");
    if !py::truthy(brief) && !run.truthy("intent_brief") {
        errs.push("missing intent brief".to_string());
    }
    if !brief.truthy("rubric") && !run.truthy("rubric") {
        errs.push("missing rubric".to_string());
    }
    let schedule: Vec<usize> = if run.truthy("schedule") {
        run.list("schedule")
            .iter()
            .filter_map(|n| n.as_u64().map(|n| n as usize))
            .collect()
    } else {
        schedule_for(start, max_loops)
    };
    for (i, lp) in loops.iter().enumerate() {
        let tag = format!("loop {}", i + 1);
        for (expert, ideas) in items(lp.at("ideas")) {
            let ideas = ideas.as_array().cloned().unwrap_or_default();
            if ideas.len() < ideas_min {
                errs.push(format!(
                    "{tag}: {expert} has {} ideas < {ideas_min}",
                    ideas.len()
                ));
            }
            let long = ideas
                .iter()
                .filter(|x| {
                    py::truthy(x) && py::words(&py::to_str(x)) as f64 > caps::IDEA as f64 * GRACE
                })
                .count();
            if long > 0 {
                warns.push(format!(
                    "{tag}: {expert} has {long} ideas over {} words",
                    caps::IDEA
                ));
            }
        }
        for card in lp.list("candidates") {
            if !card.truthy("eli5") {
                errs.push(format!("{tag}: candidate {} missing ELI5", card.show("id")));
            }
            if card.truthy("body")
                && py::words(&card.show("body")) as f64 > caps::CARD as f64 * GRACE
            {
                errs.push(format!(
                    "{tag}: candidate {} over {} words",
                    card.show("id"),
                    caps::CARD
                ));
            }
        }
        let done = lp.has("survivors");
        if done && (!lp.has("red_team") || lp.has("red_team_skipped")) {
            errs.push(format!("{tag}: no red-team step recorded"));
        }
        if done && !lp.truthy("eli5") {
            errs.push(format!("{tag}: ledger missing ELI5"));
        }
        let open = lp
            .list("red_team")
            .iter()
            .filter(|f| matches!(f.s("severity"), "S1" | "S2") && !f.truthy("response"))
            .count();
        if done && open > 0 {
            errs.push(format!("{tag}: {open} S1/S2 finding(s) without a response"));
        }
        let n_surv = lp.list("survivors").len();
        let expected = schedule
            .get((i + 1).min(schedule.len().saturating_sub(1)))
            .copied()
            .unwrap_or(0);
        if n_surv > 0 && n_surv != expected && !lp.truthy("stopped_early") {
            warns.push(format!(
                "{tag}: {n_surv} survivors, schedule expects {expected}"
            ));
        }
        let reviews = lp.list("elder_reviews");
        if done && reviews.len() < elders {
            warns.push(format!(
                "{tag}: {} elder review(s), expected {elders}",
                reviews.len()
            ));
        }
        let standins: Vec<Value> = reviews
            .iter()
            .filter(|r| r.truthy("stand_in_for"))
            .map(|r| r.at("seat").clone())
            .collect();
        if !standins.is_empty() {
            warns.push(format!(
                "{tag}: stand-in elders {} (label them in the write-up)",
                py::repr(&Value::Array(standins))
            ));
        }
    }
    let empty = json!({});
    let first = loops.first().unwrap_or(&empty);
    if items(run.at("seats")).any(|(_, v)| v.s("role") == "researcher") && first.has("ideas") {
        let reported = first
            .at("research")
            .list("seats")
            .iter()
            .any(|x| x.as_str() != Some("MAP"));
        if !reported {
            warns.push(
                "loop 1: no researcher reported (the council worked from the brief's facts only)"
                    .to_string(),
            );
        }
    }
    if !run.at("map").at("left_out").is_null() {
        warns.push("the codebase map was due but the mapper was left out".to_string());
    }
    if run.s("phase") == "done" && !run.truthy("retro") {
        warns.push("no retro / lessons recorded".to_string());
    }
    for d in run.list("deviations") {
        warns.push(format!("deviation: {}", py::to_str(d)));
    }
    json!({"ok": errs.is_empty(), "errors": errs, "warnings": warns})
}

// ----------------------------------------------------------------------------- locks

/// Hash of a file with line ends normalised, so a lock taken on Windows (CRLF in the working copy)
/// still matches the same file checked in CI on Linux (LF).
pub fn sha_file(path: &Path) -> std::io::Result<String> {
    let data = std::fs::read(path)?;
    Ok(sha256_hex(&normalise_line_ends(&data)))
}

/// `\r\n` -> `\n` in raw bytes.
pub fn normalise_line_ends(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        if data[i] == b'\r' && data.get(i + 1) == Some(&b'\n') {
            i += 1;
        }
        out.push(data[i]);
        i += 1;
    }
    out
}

/// SHA-256 of `data`, in hex.
pub fn sha256_hex(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

fn lock_path(root: &Path) -> PathBuf {
    root.join(".council").join("locks.json")
}

/// A plain file of a sane size: not a link, not a device or a pipe (those never stop giving bytes).
pub fn lockable(path: &Path) -> bool {
    crate::safe::plain_file(path, crate::safe::MAX_LOCKED).is_some()
}

/// Every lockable file under a folder. Links are not followed: one that points at a folder above
/// would go round for ever, and one that points outside is not the project's to lock.
fn files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => files_under(&path, out),
            Ok(kind) if kind.is_file() && lockable(&path) => out.push(path),
            _ => {}
        }
    }
}

/// A lock manifest as `{path: hash}`, and the entries that were refused. A path must stay inside
/// the project: relative, no `..`, no drive, no root. The manifest travels with the repository, so
/// it is checked before it is believed.
pub fn lock_entries(data: &Value) -> (Map<String, Value>, Vec<String>) {
    let listed: Vec<(String, Value)> = match data {
        Value::Object(map) => map.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        // an older form: a list of paths
        Value::Array(list) => list
            .iter()
            .filter_map(Value::as_str)
            .map(|p| (p.to_string(), Value::Null))
            .collect(),
        _ => Vec::new(),
    };
    let (mut good, mut bad) = (Map::new(), Vec::new());
    for (path, digest) in listed {
        let slashed = path.replace('\\', "/");
        let drive =
            path.as_bytes().get(1) == Some(&b':') && path.as_bytes()[0].is_ascii_alphabetic();
        if py::strip(&path).is_empty()
            || slashed.starts_with('/')
            || slashed.split('/').any(|part| part == "..")
            || drive
            || path.contains('\0')
            || path.len() > 4096
        {
            bad.push(crate::safe::shown(&path).into_owned());
        } else {
            good.insert(path, digest);
        }
    }
    bad.sort();
    (good, bad)
}

/// Record sha256 for files (or every file under a directory). Paths are stored relative to the
/// project root, so the lock means the same thing from any working directory and in CI.
pub fn lock_files(paths: &[String], root: &Path) -> Result<Value> {
    let root = resolve(root);
    let lock = lock_path(&root);
    let mut data: Map<String, Value> = if lock.exists() {
        let parsed = crate::safe::read_own(&lock)
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok());
        match parsed {
            Some(parsed) => lock_entries(&parsed).0,
            None => {
                return fail(format!(
                    "{} is not valid JSON: put it right or delete it, then lock again",
                    lock.display()
                ))
            }
        }
    } else {
        Map::new()
    };
    for p in paths {
        let given = cwd().join(p);
        let mut files = Vec::new();
        if given.is_dir() && !crate::safe::is_link(&given) {
            files_under(&given, &mut files);
            files.sort();
        } else {
            files.push(given);
        }
        for f in files {
            if !lockable(&f) {
                return fail(format!(
                    "cannot lock {}: it is not a plain file (or is over {} MB)",
                    f.display(),
                    crate::safe::MAX_LOCKED / 1_000_000
                ));
            }
            let full = resolve(&f);
            let Ok(rel) = full.strip_prefix(&root) else {
                return fail(format!(
                    "cannot lock {}: it is outside the project ({})",
                    f.display(),
                    root.display()
                ));
            };
            let hash = sha_file(&f).with_context(|| format!("cannot read {}", f.display()))?;
            data.insert(py::posix(rel), json!(hash));
        }
    }
    write_text(
        &lock,
        &format!(
            "{}\n",
            Json::ONE.sorted().dumps(&Value::Object(data.clone()))
        ),
    )?;
    Ok(json!({"locked": data.len(), "file": lock.display().to_string()}))
}

/// Compare locked files with their recorded hashes.
///
/// `against` = a git ref: read the lock manifest from that ref (e.g. `origin/main`), so a change that
/// edits a test AND its hash in the same PR is still caught. Use that form in CI. A ref that cannot
/// be read fails the check: a check that could not look must not pass.
pub fn verify_locks(root: &Path, against: Option<&str>) -> Result<Value> {
    let root = resolve(root);
    let mut note: Option<String> = None;
    let broken =
        || json!({"ok": false, "changed": [], "note": "the lock manifest is not valid JSON"});
    let text = if let Some(reference) = against {
        if !crate::safe::plain_arg(reference) {
            return fail(format!(
                "--against takes a git ref such as origin/main, not {}",
                py::str_repr(&crate::safe::shown(reference))
            ));
        }
        let git = |args: &[&str]| crate::safe::git(&root).args(args).output();
        match git(&[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{reference}^{{commit}}"),
        ]) {
            Ok(out) if out.status.success() => {}
            Ok(_) => {
                return Ok(json!({"ok": false, "changed": [], "note": format!(
                    "cannot read {reference}: git does not know that ref here. In CI, fetch it first (actions/checkout with fetch-depth: 0)")}))
            }
            Err(err) => {
                return Ok(
                    json!({"ok": false, "changed": [], "note": format!("cannot run git to read {reference}: {err}")}),
                )
            }
        }
        match git(&["show", &format!("{reference}:.council/locks.json")]) {
            Ok(out) if out.status.success() => {
                note = Some(format!("manifest from {reference}"));
                decode(&out.stdout)
            }
            _ => {
                return Ok(
                    json!({"ok": true, "changed": [], "note": format!("no lock manifest at {reference}")}),
                )
            }
        }
    } else {
        let lock = lock_path(&root);
        if !lock.exists() && !crate::safe::is_link(&lock) {
            return Ok(json!({"ok": true, "changed": [], "note": "no locks"}));
        }
        match crate::safe::read_own(&lock) {
            Ok(text) => text,
            Err(_) => return Ok(broken()),
        }
    };
    let Ok(data) = serde_json::from_str::<Value>(&text) else {
        return Ok(broken());
    };
    let (good, bad) = lock_entries(&data);
    let changed: Vec<&String> = good
        .iter()
        .filter(|(p, h)| {
            let file = root.join(p.as_str());
            !lockable(&file)
                || !crate::safe::within(&file, &root)
                || sha_file(&file).ok().as_deref() != h.as_str()
        })
        .map(|(p, _)| p)
        .collect();
    let mut out = json!({"ok": changed.is_empty() && bad.is_empty(), "changed": changed});
    if !bad.is_empty() {
        out["refused"] = json!(bad);
    }
    if let Some(note) = note {
        out["note"] = json!(note);
    }
    Ok(out)
}

/// A JSON argument: a file holding JSON, or the JSON itself.
pub fn read_json_arg(arg: &str) -> Result<Value> {
    let path = Path::new(arg);
    let text = if path.exists() {
        read_named(path)?
    } else {
        arg.to_string()
    };
    serde_json::from_str(&text).with_context(|| format!("not valid JSON: {}", py::head(arg, 80)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(id: &str, author: &str, body_words: usize) -> Value {
        json!({"id": id, "title": format!("Title {id}"), "body": vec!["word"; body_words].join(" "), "eli5": "simple", "author": author})
    }

    /// The built-in tripwires are built as ASCII patterns and matched against `ascii_reading` of
    /// the ask. That must give the answer the same patterns give over all of Unicode on the ask
    /// itself: on every text here, hand-picked and made up by chance alike.
    #[test]
    fn the_ascii_reading_of_an_ask_trips_what_the_ask_trips() {
        let over_unicode: Vec<Regex> = TRIPWIRES
            .iter()
            .map(|(_, rx)| {
                regex::RegexBuilder::new(rx)
                    .case_insensitive(true)
                    .build()
                    .unwrap()
            })
            .collect();
        let rules = tripwire_rules(Path::new("/nowhere/at/all"));
        assert_eq!(rules.built_in.len(), TRIPWIRES.len());
        for (name, rx) in TRIPWIRES {
            // the stand-ins must be characters no pattern spells out, and the patterns must use
            // nothing whose meaning the reading does not keep
            for not_kept in [
                "_", "\t", "\\t", "\\d", "\\D", "\\W", "\\S", "\\B", "[^", "\\p", "\\x", "\\u",
            ] {
                assert!(
                    !rx.contains(not_kept),
                    "{name}: the pattern uses {not_kept:?}"
                );
            }
            assert!(rx.is_ascii(), "{name}");
        }
        let same = |text: &str| {
            let plain = ascii_reading(text);
            assert_eq!(plain.chars().count(), text.chars().count(), "{text:?}");
            for ((name, _), unicode) in TRIPWIRES.iter().zip(&over_unicode) {
                assert_eq!(
                    rules.text[*name].is_match(&plain),
                    unicode.is_match(text),
                    "{name}: {text:?} read as {plain:?}"
                );
            }
        };
        let picked = [
            "drop the café table",
            "drop the cafe\u{301} table",
            "alter\u{a0}table users",
            "auth—flow",
            "réauth the user",
            "auth² levels",
            "the \u{17f}so login",
            "a \u{212a}8s cluster",
            "\u{212a}ubernetes",
            "migrate données database",
            "ｍｉｇｒａｔｉｏｎ",
            "delete all 顧客 records",
            "delete\u{2003}users",
            "force‑push it",
            "publish über blog",
            "deprecate the ‿ endpoints",
            "api\u{200d}keys",
            "delete x\u{200d}y rows",
            "password²",
            "ßecrets",
            "se\u{301}crets",
            "prod\u{a0}deploy",
            "…sso…",
            "«rfc»",
            "the migration’s plan",
            "ALTER\u{2028}TABLE",
            "truncate les données rows",
            "purge ０１２ buckets",
            "wipe\u{feff}data",
            "announce™",
            "Ⅳ strategy",
            "roadmap№",
            "\u{1d4ea}pi contract",
        ];
        for text in picked {
            same(text);
        }
        // made up by chance, from the words the patterns know and the characters that could fool them
        let words = [
            "delete",
            "drop",
            "truncate",
            "users",
            "tables",
            "data",
            "migrate",
            "migration",
            "the",
            "all",
            "prod",
            "deploy",
            "auth",
            "api",
            "keys",
            "password",
            "sso",
            "public",
            "release",
            "blog",
            "post",
            "schema",
            "database",
            "alter",
            "table",
            "rm",
            "-rf",
            "force",
            "push",
            "publish",
            "package",
            "strategy",
            "rfc",
            "k8s",
            "dns",
            "café",
            "naïve",
            "顧客",
            "über",
            "x\u{301}",
            "²",
            "\u{212a}",
            "\u{17f}",
            "ｋ８ｓ",
            "données",
        ];
        let joins = [
            " ", " ", " ", "  ", "\u{a0}", "\u{2003}", "-", "—", "‑", "/", ".", "", "\u{200d}",
            "\u{301}", "’", "_", "\n", "\t", "²",
        ];
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        let mut pick = |n: usize| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state % n as u64) as usize
        };
        for _ in 0..20_000 {
            let mut text = String::new();
            for _ in 0..2 + pick(5) {
                text.push_str(words[pick(words.len())]);
                text.push_str(joins[pick(joins.len())]);
            }
            same(&text);
        }
        // a project's own pattern replaces ours and is read over all of Unicode, as it was written
        assert_eq!(&*ascii_reading("plain ask"), "plain ask");
    }

    #[test]
    fn labels_and_schedule() {
        assert_eq!(labels_for(3), ["A", "B", "C"]);
        assert_eq!(labels_for(28)[25..], ["Z", "AA", "AB"]);
        assert_eq!(schedule_for(7, 3), [7, 5, 3, 1]);
        assert_eq!(schedule_for(3, 1), [3, 1]);
        assert_eq!(schedule_for(7, 4), [7, 5, 3, 2, 1]);
        assert_eq!(schedule_for(7, 2), [7, 5, 1]);
    }

    #[test]
    fn blind_hides_authors_and_flags_leaks() {
        let cards = vec![
            card("c1", "L1", 100),
            card("c2", "L2", 100),
            card("c3", "L3", 100),
        ];
        let out = blind(&cards, 42, caps::CARD, &[], 1.5);
        assert_eq!(out["ok"], json!(true));
        assert!(!out.s("packet").contains("L1") && out.s("packet").contains("### Candidate A: "));
        let ids: BTreeSet<String> = items(&out["mapping"])
            .map(|(_, v)| v.s("id").to_string())
            .collect();
        assert_eq!(ids.len(), 3);
        let mut leaky = card("c4", "L4", 100);
        leaky["body"] = json!("As the cost expert I say my lens is right and seat L4 agrees");
        let out = blind(
            &[leaky, card("c5", "L5", 14)],
            1,
            caps::CARD,
            &["Fable".to_string()],
            1.5,
        );
        let errors = out["errors"].to_string();
        assert!(
            errors.contains("the card names 'L4'")
                && errors.contains("identity leak: 'As the cost expert'"),
            "{errors}"
        );
        assert!(
            names_token("built by Fable.", "Fable")
                && !names_token("Fable-like models", "Fable")
                && !names_token("unFabled", "Fable")
        );
        assert!(
            leak_in("use the rubric as a lens").is_none()
                && leak_in("a seat call costs tokens").is_none()
        );
        assert!(leak_in("voter l3 said").is_none() && leak_in("voter L3 said").is_some());
    }

    #[test]
    fn a_voter_cannot_lift_its_own_card_and_a_gap_is_no_opinion() {
        let mapping = json!({"A": {"id": "a", "authors": ["L1"]}, "B": {"id": "b", "authors": ["L2"]}, "C": {"id": "c", "authors": ["L3"]}});
        let votes = vec![
            json!({"voter": "L1", "ranking": ["A", "B", "C"]}),
            json!({"voter": "L2", "ranking": ["Candidate B", "c.", "a"]}),
            json!({"voter": "L3", "ranking": ["C", "A"]}),
        ];
        let out = tally(&votes, &mapping, &TallyOptions::default());
        // The exact line the Python reference prints for the same ballots.
        assert_eq!(
            Json::LINE.dumps(&out),
            concat!(
                r#"{"ranking": [{"label": "B", "id": "b", "score": 0.583, "check": "n/a"}, {"label": "C", "id": "c", "score": 0.5, "check": "n/a"}, "#,
                r#"{"label": "A", "id": "a", "score": 0.417, "check": "n/a"}], "ballots": 3, "head_to_heads": 2, "recused": 5, "gaps": 1, "#,
                r#""thin": [["A", "B"]], "ties": [], "agreement": null, "#,
                r#""agreement_note": "not measurable: no two voters judged the same head-to-head", "best_on": {}, "problems": []}"#
            )
        );
        let failed = tally(
            &votes,
            &mapping,
            &TallyOptions {
                checks: Some(&json!({"A": "FAIL"})),
                keep: Some(2),
                ..Default::default()
            },
        );
        assert_eq!(failed["ranking"][2]["label"], json!("A"));
        assert_eq!(failed["eliminated"], json!(["A"]));
    }

    #[test]
    fn the_wildcard_replaces_the_weakest_survivor_only_above_the_final_cut() {
        let ranked: Vec<String> = ["A", "B", "C", "D", "E"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let none = HashSet::new();
        let out = cut(&ranked, 3, &json!({}), &none, &json!({"R2": "E"}), None);
        assert_eq!(
            out,
            json!({"survivors": ["A", "B", "E"], "eliminated": ["D", "C"], "wildcard": {"label": "E", "criterion": "R2", "replaces": "C"}})
        );
        let out = cut(&ranked, 1, &json!({}), &none, &json!({"R2": "E"}), None);
        assert_eq!(out["survivors"], json!(["A"]));
        assert!(out["wildcard"].is_null());
        let thin = json!([["A", "D"], ["C", "E"]]);
        assert_eq!(protected(&ranked, 3, &thin, &none), ["D", "E"]);
        assert_eq!(
            protected(&ranked, 3, &thin, &["D".to_string()].into_iter().collect()),
            ["E"]
        );
    }

    #[test]
    fn text_from_other_machines_reads_the_same() {
        let text = "members:\n  research:\n    seats: 1\n";
        let utf16: Vec<u8> = [0xff, 0xfe]
            .into_iter()
            .chain(
                text.replace('\n', "\r\n")
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes),
            )
            .collect();
        assert_eq!(decode(&utf16), text);
        let with_mark: Vec<u8> = [0xef, 0xbb, 0xbf]
            .into_iter()
            .chain(text.replace('\n', "\r\n").bytes())
            .collect();
        assert_eq!(decode(&with_mark), text);
        assert!(decode(b"caf\xe9").starts_with("caf"));
        assert_eq!(
            sha256_hex(&normalise_line_ends(b"a\r\nb\r\n")),
            sha256_hex(b"a\nb\n")
        );
    }
}
