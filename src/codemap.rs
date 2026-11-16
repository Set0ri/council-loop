//! A cheap, deterministic skeleton of a codebase, for the mapper seat and for people.
//!
//! ELI5: before anyone draws a map, a program walks every street and lists it: each file, what it
//! defines, what it leans on, and which files everything else leans on. The mapper (an AI) reads that
//! list and a few key files, then writes the map. Next time the program says what changed in the
//! street plan, so the map is redrawn only when the plan changed, not when someone repainted a door.
//!
//! What it does (no model, no network):
//!
//! - [`skeleton`]: files, sizes, public symbols, the import graph, a ranking of key files,
//!   commands / tools / routes / environment variables, tests
//! - [`focus_map`]: the few files and names that matter most for ONE task, within a token budget
//! - [`stale`]: has the STRUCTURE changed since the map was last accepted? (bodies do not count)
//! - [`validate_map`]: does a written map have the sections, and does every path it names exist?
//! - [`install_map`]: put an accepted map in place, without ever overwriting a hand-written one
//!
//! The ranking is the method of aider's repo map (Aider-AI/aider, Apache-2.0): a graph whose edges
//! run from the file that USES a name to the file that DEFINES it, weighted by sqrt(uses), x10 for
//! names the task mentions and for long well-formed names, x0.1 for names defined in more than five
//! files, ranked with personalised PageRank and cut to a token budget. Written from aider's
//! published design: no aider code is copied.
//!
//! Python is read with a tree-sitter parse ([`crate::pyread`]); other languages with small pattern
//! readers that are the same on every machine, which is what staleness needs.

use crate::core::{self};
use crate::py::{self, Json};
use crate::say;
use crate::val::Get;
use anyhow::Result;
use indexmap::IndexMap;
use regex::Regex;
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

const IGNORE_DIRS: [&str; 24] = [
    ".git",
    "node_modules",
    ".venv",
    "venv",
    "env",
    "dist",
    "build",
    "out",
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
    ".next",
    ".nuxt",
    "target",
    "vendor",
    ".idea",
    ".vscode",
    "coverage",
    ".tox",
    ".council",
    ".gradle",
    "Pods",
    "DerivedData",
];
const CODE: [(&str, &str); 22] = [
    (".py", "Python"),
    (".js", "JavaScript"),
    (".jsx", "JavaScript"),
    (".mjs", "JavaScript"),
    (".cjs", "JavaScript"),
    (".ts", "TypeScript"),
    (".tsx", "TypeScript"),
    (".go", "Go"),
    (".rs", "Rust"),
    (".java", "Java"),
    (".kt", "Kotlin"),
    (".cs", "C#"),
    (".rb", "Ruby"),
    (".php", "PHP"),
    (".swift", "Swift"),
    (".c", "C"),
    (".h", "C"),
    (".cc", "C++"),
    (".cpp", "C++"),
    (".hpp", "C++"),
    (".sh", "Shell"),
    (".sql", "SQL"),
];
const OTHER: [(&str, &str); 7] = [
    (".md", "Markdown"),
    (".yaml", "YAML"),
    (".yml", "YAML"),
    (".json", "JSON"),
    (".toml", "TOML"),
    (".html", "HTML"),
    (".css", "CSS"),
];
const MAX_BYTES: u64 = 512_000;
const MAX_FILES: usize = 4000;
/// Files looked at when git cannot list them.
const WALK_CAP: usize = 50_000;
/// The first-line marker of a map this tool wrote.
pub const MARKER: &str = "<!-- council-loop:map";
const REQUIRED: [&str; 6] = [
    "Purpose and philosophy",
    "Bird's-eye view",
    "Code map",
    "Interfaces",
    "Invariants",
    "Extending it",
];
const ADVISED: [&str; 3] = [
    "Data and control flow",
    "Performance and cost",
    "Where to change what",
];
/// A resolved import is certain; a matching name is likely.
const IMPORT_WEIGHT: f64 = 3.0;

fn code_lang(suffix: &str) -> Option<&'static str> {
    CODE.iter().find(|(s, _)| *s == suffix).map(|(_, l)| *l)
}

fn other_lang(suffix: &str) -> Option<&'static str> {
    OTHER.iter().find(|(s, _)| *s == suffix).map(|(_, l)| *l)
}

/// `Path(name).suffix.lower()`: the last extension of the file name, with its dot.
fn suffix(name: &str) -> String {
    let file = name.rsplit('/').next().unwrap_or(name);
    match file.rfind('.') {
        Some(i) if i > 0 && i < file.len() - 1 => file[i..].to_lowercase(),
        _ => String::new(),
    }
}

fn stem(name: &str) -> &str {
    let file = name.rsplit('/').next().unwrap_or(name);
    match file.rfind('.') {
        Some(i) if i > 0 && i < file.len() - 1 => &file[..i],
        _ => file,
    }
}

fn parent(name: &str) -> &str {
    name.rfind('/').map_or("", |i| &name[..i])
}

// ----------------------------------------------------------------------------- reading the code

/// The first line of a text that is not blank, cut to `cap` characters.
pub fn first_line(text: &str, cap: usize) -> String {
    let text = &*crate::safe::shown(text);
    let line = py::lines(text)
        .into_iter()
        .map(py::strip)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    if py::len(line) <= cap {
        line.to_string()
    } else {
        format!("{}…", py::head(line, cap - 1))
    }
}

/// One public name of a file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Symbol {
    /// The name.
    pub name: String,
    /// `def`, `class`, `fn`, `struct`, ...
    pub kind: String,
    /// The signature, as shown in the skeleton.
    pub sig: String,
    /// First line of its doc comment.
    pub doc: String,
    /// The line it is defined on.
    pub line: usize,
    /// Public methods, for classes.
    pub methods: Option<Vec<String>>,
}

/// What a reader found in one file.
#[derive(Clone, Debug, Default)]
pub struct Read {
    /// First line of the file's doc comment.
    pub doc: String,
    /// Names the file defines.
    pub symbols: Vec<Symbol>,
    /// What it imports, as written.
    pub imports: Vec<String>,
    /// Is it runnable?
    pub main: bool,
    /// Names it uses, with counts (empty: count words instead).
    pub uses: IndexMap<String, usize>,
}

/// A pattern of the code reader. Every one is matched in time proportional to the text (no
/// look-around, no back-references), so no source file can be written to make the reader slow.
fn rx(pattern: &str) -> Regex {
    Regex::new(pattern).expect("the built-in patterns are valid")
}

fn by_regex(text: &str, symbols: &Regex, imports: Option<&Regex>) -> Read {
    by_regex_unless(text, symbols, imports, None)
}

/// [`by_regex`], leaving out a definition whose line starts the way `unless` describes (a pattern
/// anchored with `\A`). The search goes on from the next line, exactly as a look-ahead at the
/// start of the line would make it.
fn by_regex_unless(
    text: &str,
    symbols: &Regex,
    imports: Option<&Regex>,
    unless: Option<&Regex>,
) -> Read {
    let mut out = Read::default();
    // matches come in order, so line numbers are counted once, forward (not from the top each time)
    let (mut pos, mut line, mut counted) = (0usize, 1usize, 0usize);
    while pos <= text.len() {
        let Some(m) = symbols.captures_at(text, pos) else {
            break;
        };
        let whole = m.get(0).expect("group 0 is the match");
        if unless.is_some_and(|u| u.is_match(&text[whole.start()..])) {
            match text[whole.start()..].find('\n') {
                Some(nl) => pos = whole.start() + nl + 1,
                None => break,
            }
            continue;
        }
        line += text[counted..whole.start()].matches('\n').count();
        counted = whole.start();
        let kind = m
            .get(1)
            .map(|k| k.as_str())
            .filter(|k| !k.is_empty())
            .unwrap_or("def");
        let squashed = py::squash(whole.as_str());
        out.symbols.push(Symbol {
            name: m.get(2).map(|n| n.as_str().to_string()).unwrap_or_default(),
            kind: py::split(kind).last().unwrap_or("def").to_string(),
            sig: py::head(&squashed, 120).to_string(),
            doc: String::new(),
            line,
            methods: None,
        });
        pos = if whole.end() > whole.start() {
            whole.end()
        } else {
            whole.end() + text[whole.end()..].chars().next().map_or(1, char::len_utf8)
        };
    }
    if let Some(imports) = imports {
        for m in imports.captures_iter(text) {
            if let Some(found) = (1..m.len())
                .filter_map(|i| m.get(i))
                .map(|g| g.as_str())
                .find(|g| !g.is_empty())
            {
                out.imports.push(found.to_string());
            }
        }
    }
    out
}

fn extract_python(text: &str) -> Read {
    static SYMBOLS: LazyLock<Regex> =
        LazyLock::new(|| rx(r"(?m)^(class|def|async def)\s+([A-Za-z_]\w*)"));
    static IMPORTS: LazyLock<Regex> =
        LazyLock::new(|| rx(r"(?m)^\s*(?:from\s+([\w.]+)\s+import|import\s+([\w.]+))"));
    match crate::pyread::read(text) {
        Some(f) => Read {
            doc: f.doc,
            symbols: f.symbols,
            imports: f.imports,
            main: f.main,
            uses: f.uses,
        },
        None => by_regex(text, &SYMBOLS, Some(&IMPORTS)),
    }
}

fn extract_js(text: &str) -> Read {
    static SYMBOLS: LazyLock<Regex> = LazyLock::new(|| {
        rx(
            r"(?m)^\s*export\s+(?:default\s+)?(?:async\s+)?(function\*?|class|const|let|var|interface|type|enum)\s+([A-Za-z_$][\w$]*)",
        )
    });
    static IMPORTS: LazyLock<Regex> = LazyLock::new(|| {
        rx(r#"(?m)(?:\bfrom\s*|\bimport\s*\(?\s*|\brequire\(\s*)['"]([^'"\n]+)['"]"#)
    });
    static HEAD: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?s)\A\s*/\*\*?(.*?)\*/").expect("valid"));
    static STAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\s*\*\s?").expect("valid"));
    let mut out = by_regex(text, &SYMBOLS, Some(&IMPORTS));
    if let Some(head) = HEAD.captures(text) {
        out.doc = first_line(&STAR.replace_all(&head[1], ""), 110);
    }
    out
}

fn extract_go(text: &str) -> Read {
    static FUNCS: LazyLock<Regex> =
        LazyLock::new(|| rx(r"(?m)^(func)\s+(?:\([^)]*\)\s*)?([A-Z]\w*)\s*\("));
    static TYPES: LazyLock<Regex> =
        LazyLock::new(|| rx(r"(?m)^(type)\s+([A-Z]\w*)\s+(?:struct|interface)"));
    static IMPORTS: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"(?m)^\s*(?:[\w.]+\s+)?"([\w./-]+)"\s*$"#).expect("valid"));
    static MAIN: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?m)^func\s+main\s*\(").expect("valid"));
    let mut out = by_regex(text, &FUNCS, None);
    out.symbols.extend(by_regex(text, &TYPES, None).symbols);
    out.imports = IMPORTS
        .captures_iter(text)
        .map(|m| m[1].to_string())
        .collect();
    out.main = MAIN.is_match(text);
    out
}

fn extract_rust(text: &str) -> Read {
    static SYMBOLS: LazyLock<Regex> = LazyLock::new(|| {
        rx(
            r"(?m)^\s*pub(?:\([^)]*\))?\s+(?:async\s+)?(fn|struct|enum|trait|mod|type|const)\s+(\w+)",
        )
    });
    static IMPORTS: LazyLock<Regex> =
        LazyLock::new(|| rx(r"(?m)^\s*(?:pub\s+)?(?:mod|use)\s+([\w:]+)"));
    static MAIN: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?m)^fn\s+main\s*\(").expect("valid"));
    let mut out = by_regex(text, &SYMBOLS, Some(&IMPORTS));
    out.main = MAIN.is_match(text);
    out
}

fn extract_classes(text: &str) -> Read {
    static SYMBOLS: LazyLock<Regex> = LazyLock::new(|| {
        rx(
            r"(?m)^\s*(?:(?:public|internal|abstract|final|sealed|data|open|static)\s+)*(class|interface|enum|record|object|struct)\s+(\w+)",
        )
    });
    by_regex(text, &SYMBOLS, None)
}

fn extract_ruby(text: &str) -> Read {
    static SYMBOLS: LazyLock<Regex> =
        LazyLock::new(|| rx(r"(?m)^\s*(class|module|def)\s+([\w.:?!]+)"));
    static IMPORTS: LazyLock<Regex> =
        LazyLock::new(|| rx(r#"(?m)^\s*require(?:_relative)?\s+['"]([^'"]+)['"]"#));
    by_regex(text, &SYMBOLS, Some(&IMPORTS))
}

/// Languages without a reader of their own (C, C++, shell, SQL): un-indented lines that look like
/// a definition. Rough, but it is the same on every machine, which is what staleness needs.
fn extract_generic(text: &str) -> Read {
    static FUNCS: LazyLock<Regex> = LazyLock::new(|| {
        rx(
            r"(?m)^(?:[A-Za-z_][\w:<>,*&\s]*?\s+\**)?()([A-Za-z_]\w{2,})\s*\([^;{}]*\)\s*(?:const\s*)?\{?\s*$",
        )
    });
    // a statement is not a definition: `while (x) {` starts with a keyword
    static KEYWORD: LazyLock<Regex> =
        LazyLock::new(|| rx(r"\A(?:if|for|while|switch|return|else|do|case)\b"));
    static TYPES: LazyLock<Regex> = LazyLock::new(|| {
        rx(r"(?m)^(?:typedef\s+)?(struct|class|enum|union|namespace)\s+([A-Za-z_]\w*)")
    });
    static SQL: LazyLock<Regex> = LazyLock::new(|| {
        rx(
            r#"(?im)^\s*create\s+(?:or\s+replace\s+)?(table|view|function|procedure|index)\s+(?:if\s+not\s+exists\s+)?[`"\[]?([\w.]+)"#,
        )
    });
    static INCLUDES: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"(?m)^\s*#\s*include\s+"([^"]+)""#).expect("valid"));
    static MAIN: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?m)^\s*(?:int|void)\s+main\s*\(").expect("valid"));
    let mut out = by_regex_unless(text, &FUNCS, None, Some(&KEYWORD));
    out.symbols.extend(by_regex(text, &TYPES, None).symbols);
    out.symbols.extend(by_regex(text, &SQL, None).symbols);
    out.imports = INCLUDES
        .captures_iter(text)
        .map(|m| m[1].to_string())
        .collect();
    out.main = MAIN.is_match(text);
    out
}

/// The reader for a file suffix. Add a language: write a `fn(&str) -> Read` and name its suffixes here.
fn extractor(suffix: &str) -> Option<fn(&str) -> Read> {
    Some(match suffix {
        ".py" => extract_python,
        ".js" | ".jsx" | ".mjs" | ".cjs" | ".ts" | ".tsx" => extract_js,
        ".go" => extract_go,
        ".rs" => extract_rust,
        ".java" | ".kt" | ".cs" | ".swift" | ".php" => extract_classes,
        ".rb" => extract_ruby,
        ".c" | ".h" | ".cc" | ".cpp" | ".hpp" | ".sh" | ".sql" => extract_generic,
        _ => return None,
    })
}

static ENV_RX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?:os\.environ(?:\.get)?\(\s*|os\.environ\[\s*|os\.getenv\(\s*|\benv(?:iron)?\.get\(\s*|\benv\[\s*|process\.env\[\s*|env::var\(\s*|os\.Getenv\(\s*|ENV\[\s*)['"]([A-Z][A-Z0-9_]{2,})['"]|process\.env\.([A-Z][A-Z0-9_]{2,})"#,
    )
    .expect("valid")
});
static CLI_RX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"add_parser\(\s*['"]([\w-]+)['"]|@(?:\w+\.)?command\(\s*(?:name\s*=\s*)?['"]([\w-]+)['"]"#).expect("valid")
});
static ROUTE_RX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"@?\b(?:app|router|bp|api|blueprint)\.(get|post|put|patch|delete|route)\(\s*['"`]([^'"`\n]+)['"`]"#).expect("valid")
});
static TOOL_RX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"@mcp\.(tool|resource|prompt)\(([^)\n]*)\)\s*\n\s*(?:async\s+)?def\s+(\w+)")
        .expect("valid")
});
static IDENT_RX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z_][A-Za-z0-9_]{2,}").expect("valid"));
static IDENT_FULL_RX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z_][A-Za-z0-9_]{2,}$").expect("valid"));
// comments and strings
static NOISE_RX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)/\*.*?\*/|//[^\n]*|"(?:\\.|[^"\\\n])*"|`(?:\\.|[^`\\])*`"#).expect("valid")
});
static QUOTE_RX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"'(?:\\.|[^'\\\n])*'").expect("valid"));
static HASH_RX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)(?:^|\s)#[^\n]*").expect("valid"));
static TEST_RX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^\s*(?:async\s+)?def\s+test_\w+|^\s*(?:it|test)\(\s*['"`]|^func\s+Test\w+|#\[test\]"#).expect("valid")
});
const SINGLE_QUOTED: [&str; 12] = [
    ".js", ".jsx", ".mjs", ".cjs", ".ts", ".tsx", ".rb", ".php", ".sh", ".sql", ".py", ".swift",
];
const HASH_COMMENTS: [&str; 4] = [".rb", ".sh", ".py", ".php"];

/// The names a file uses, with comments and string contents removed first (roughly: this is a
/// pattern, not a parser). A word in a comment is not a use of the function that shares its name.
fn code_words(text: &str, suffix: &str) -> IndexMap<String, usize> {
    let mut body = NOISE_RX.replace_all(text, " ").into_owned();
    if SINGLE_QUOTED.contains(&suffix) {
        body = QUOTE_RX.replace_all(&body, " ").into_owned();
    }
    if HASH_COMMENTS.contains(&suffix) {
        body = HASH_RX.replace_all(&body, " ").into_owned();
    }
    let mut out = IndexMap::new();
    for m in IDENT_RX.find_iter(&body) {
        *out.entry(m.as_str().to_string()).or_insert(0) += 1;
    }
    out
}

/// Repo files worth reading, as posix paths, and whether the list was capped. Uses git when it
/// can, so ignore rules are honoured.
pub fn list_files(root: &Path) -> (Vec<String>, bool) {
    let mut names: Option<Vec<String>> = None;
    if root.join(".git").exists() {
        // -z: names as they are, never quoted or escaped
        let out = crate::safe::git(root)
            .args(["ls-files", "-co", "--exclude-standard", "-z"])
            .output();
        if let Ok(out) = out {
            if out.status.success() {
                names = Some(
                    String::from_utf8_lossy(&out.stdout)
                        .split('\0')
                        .filter(|n| !n.is_empty())
                        .map(str::to_string)
                        .collect(),
                );
            }
        }
    }
    let names = names.unwrap_or_else(|| {
        let mut names = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while names.len() < WALK_CAP {
            // not a repository, or far too big: stop looking
            let Some(dir) = stack.pop() else { break };
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            let mut entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
            entries.sort();
            for p in entries {
                let name = p
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                // links are not followed: one may point out of the project, or back up into it
                let Ok(kind) = std::fs::symlink_metadata(&p).map(|m| m.file_type()) else {
                    continue;
                };
                if kind.is_dir() {
                    if !IGNORE_DIRS.contains(&name.as_str()) {
                        stack.push(p);
                    }
                } else if kind.is_file() {
                    if let Ok(rel) = p.strip_prefix(root) {
                        names.push(py::posix(rel));
                    }
                }
            }
        }
        names
    });
    let unique: BTreeSet<String> = names.into_iter().collect();
    let mut keep = Vec::new();
    // folder -> does it, with links followed, lie inside the project? (asked once per folder)
    let mut inside: HashMap<PathBuf, bool> = HashMap::new();
    for n in unique {
        let parts: Vec<&str> = n.split('/').collect();
        let suf = suffix(&n);
        if parts[..parts.len() - 1]
            .iter()
            .any(|p| IGNORE_DIRS.contains(p))
            || (code_lang(&suf).is_none() && other_lang(&suf).is_none())
        {
            continue;
        }
        let p = root.join(&n);
        // a plain file only: a link may point out of the project (its target's names would land in
        // the skeleton, and the skeleton goes to a model), a device never ends, and a name with
        // control characters would be printed
        if crate::safe::plain_file(&p, MAX_BYTES).is_none() || crate::safe::odd_name(&n) {
            continue;
        }
        let folder = p.parent().unwrap_or(root).to_path_buf();
        let ok = *inside
            .entry(folder)
            .or_insert_with_key(|dir| crate::safe::within(dir, root));
        if ok {
            keep.push(n);
        }
    }
    let capped = keep.len() > MAX_FILES;
    keep.truncate(MAX_FILES);
    (keep, capped)
}

/// What the scan knows about one file.
#[derive(Clone, Debug, Default)]
pub struct FileInfo {
    /// The language's name.
    pub lang: &'static str,
    /// Is it source code (as opposed to prose or data)?
    pub code: bool,
    /// Line count.
    pub lines: usize,
    /// First line of the file's doc comment.
    pub doc: String,
    /// Names it defines (private ones too).
    pub symbols: Vec<Symbol>,
    /// What it imports, as written.
    pub imports: Vec<String>,
    /// Is it runnable?
    pub main: bool,
    /// Environment variables it reads.
    pub env: Vec<String>,
    /// Commands it defines.
    pub cli: Vec<String>,
    /// HTTP routes it serves.
    pub routes: Vec<String>,
    /// MCP tools, resources and prompts it declares.
    pub tools: Vec<String>,
    /// Test functions in it.
    pub tests: usize,
    /// Names it uses that this repository defines, with counts.
    pub uses: IndexMap<String, usize>,
}

/// Every file of a repository, read once.
#[derive(Clone, Debug, Default)]
pub struct Scan {
    /// path -> what was found.
    pub files: IndexMap<String, FileInfo>,
    /// Was the file list cut?
    pub capped: bool,
}

/// What one file holds. `None` when it cannot be read.
fn read_file(root: &Path, n: &str) -> Option<FileInfo> {
    // line ends and byte-order marks must not change what is read: a CRLF checkout and an LF one are the same code
    let raw = std::fs::read(root.join(n)).ok()?;
    let raw = core::normalise_line_ends(&raw);
    let text =
        String::from_utf8_lossy(raw.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&raw)).into_owned();
    let suf = suffix(n);
    let code = code_lang(&suf).is_some();
    let mut info = FileInfo {
        lang: code_lang(&suf).or(other_lang(&suf)).unwrap_or(""),
        code,
        lines: text.matches('\n').count() + usize::from(!text.is_empty() && !text.ends_with('\n')),
        ..Default::default()
    };
    if let Some(read) = extractor(&suf) {
        let found = read(&text);
        info.doc = found.doc;
        info.symbols = found.symbols;
        info.imports = found.imports;
        info.main = found.main;
        info.uses = found.uses;
    }
    if code {
        if info.uses.is_empty() {
            info.uses = code_words(&text, &suf);
        }
        let env: BTreeSet<String> = ENV_RX
            .captures_iter(&text)
            .filter_map(|m| m.get(1).or(m.get(2)).map(|g| g.as_str().to_string()))
            .collect();
        info.env = env.into_iter().collect();
        info.cli = CLI_RX
            .captures_iter(&text)
            .filter_map(|m| m.get(1).or(m.get(2)).map(|g| g.as_str().to_string()))
            .collect();
        info.routes = ROUTE_RX
            .captures_iter(&text)
            .map(|m| format!("{} {}", m[1].to_uppercase(), &m[2]))
            .collect();
        info.tools = TOOL_RX
            .captures_iter(&text)
            .map(|m| format!("{} {}", &m[1], &m[3]))
            .collect();
        info.tests = TEST_RX.find_iter(&text).count();
    }
    Some(info)
}

/// Read every file once.
pub fn scan(root: &Path) -> Scan {
    let started = std::time::Instant::now();
    let (names, capped) = list_files(root);
    crate::diag!(
        Debug,
        "map",
        "{} file(s) listed in {} ms{}",
        names.len(),
        started.elapsed().as_millis(),
        if capped { " (the list was cut)" } else { "" }
    );
    // every file is read on its own, so the reading is spread over the processor's cores; the
    // results are put together in the order of the list, so the outcome is the same on any machine
    let threads = if names.len() < 64 {
        1
    } else {
        std::thread::available_parallelism()
            .map_or(1, |n| n.get())
            .min(16)
    };
    let read: Vec<Option<FileInfo>> =
        crate::providers::in_parallel(&names, threads, |n| read_file(root, n));
    let mut files: IndexMap<String, FileInfo> = IndexMap::new();
    for (n, info) in names.into_iter().zip(read) {
        if let Some(info) = info {
            files.insert(n, info);
        }
    }
    crate::diag!(
        Debug,
        "map",
        "{} file(s) read in {} ms on {threads} thread(s)",
        files.len(),
        started.elapsed().as_millis()
    );
    let defined: HashSet<String> = files.values().flat_map(def_names).collect();
    for info in files.values_mut() {
        // keep only uses of names this repository defines
        info.uses.retain(|name, _| defined.contains(name));
    }
    Scan { files, capped }
}

/// The public symbols to show for a file.
fn shown(info: &FileInfo) -> Vec<&Symbol> {
    info.symbols
        .iter()
        .filter(|s| !s.name.starts_with('_'))
        .collect()
}

fn def_names(info: &FileInfo) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for sym in shown(info) {
        names.insert(sym.name.clone());
        names.extend(sym.methods.iter().flatten().cloned());
    }
    names
        .into_iter()
        .filter(|n| py::len(n) >= 3 && IDENT_FULL_RX.is_match(n))
        .collect()
}

/// file -> the files in this repo it imports. Python by module name, JS/TS by relative path.
fn resolve_imports(files: &IndexMap<String, FileInfo>) -> IndexMap<String, Vec<String>> {
    let mut by_stem: HashMap<&str, Vec<&String>> = HashMap::new();
    for n in files.keys().filter(|n| n.ends_with(".py")) {
        let key = if stem(n) == "__init__" {
            parent(n).rsplit('/').next().unwrap_or("")
        } else {
            stem(n)
        };
        by_stem.entry(key).or_default().push(n);
    }
    let join = |base: &str, rest: &str| {
        if base.is_empty() {
            rest.to_string()
        } else {
            format!("{base}/{rest}")
        }
    };
    let mut edges: IndexMap<String, Vec<String>> = IndexMap::new();
    for (n, info) in files {
        let here = parent(n);
        let mut found: BTreeSet<String> = BTreeSet::new();
        for imp in &info.imports {
            let mut target: Option<String> = None;
            if n.ends_with(".py") {
                let trimmed = imp.trim_start_matches('.');
                let dots = imp.len() - trimmed.len();
                let parts: Vec<&str> = trimmed.split('.').filter(|x| !x.is_empty()).collect();
                let mut base = here;
                for _ in 0..dots.saturating_sub(1) {
                    base = parent(base);
                }
                'search: for k in (1..=parts.len()).rev() {
                    let as_path = parts[..k].join("/");
                    let cands = [
                        format!("{}.py", join(base, &as_path)),
                        join(base, &format!("{as_path}/__init__.py")),
                        format!("{as_path}.py"),
                        format!("{as_path}/__init__.py"),
                    ];
                    for cand in cands {
                        if files.contains_key(&cand) {
                            target = Some(cand);
                            break 'search;
                        }
                    }
                }
                if target.is_none() && !parts.is_empty() && dots == 0 {
                    let same = by_stem.get(parts[0]).cloned().unwrap_or_default();
                    let near: Vec<&&String> = same.iter().filter(|c| parent(c) == here).collect();
                    target = match near.first() {
                        Some(first) => Some((**first).clone()),
                        None if same.len() == 1 => Some(same[0].clone()),
                        None => None,
                    };
                }
            } else if imp.starts_with('.') {
                let mut parts: Vec<&str> = Vec::new();
                let joined = join(here, imp);
                for seg in joined.split('/') {
                    if seg == ".." {
                        parts.pop();
                    } else if !seg.is_empty() && seg != "." {
                        parts.push(seg);
                    }
                }
                let stem_path = parts.join("/");
                let mut cands = vec![stem_path.clone()];
                cands.extend(
                    [".ts", ".tsx", ".js", ".jsx", ".mjs"]
                        .iter()
                        .map(|e| format!("{stem_path}{e}")),
                );
                cands.extend(
                    [".ts", ".tsx", ".js", ".jsx"]
                        .iter()
                        .map(|e| format!("{stem_path}/index{e}")),
                );
                target = cands.into_iter().find(|c| files.contains_key(c));
            }
            if let Some(t) = target {
                if &t != n {
                    found.insert(t);
                }
            }
        }
        if !found.is_empty() {
            edges.insert(n.clone(), found.into_iter().collect());
        }
    }
    edges
}

/// A long snake_case, kebab-case or camelCase name is rarely a coincidence (aider's test).
fn well_named(ident: &str) -> bool {
    let snake = ident.contains('_') && ident.chars().any(char::is_alphabetic);
    let camel = ident.chars().any(char::is_uppercase) && ident.chars().any(char::is_lowercase);
    (snake || camel || ident.contains('-')) && py::len(ident) >= 8
}

/// The result of [`ranking`].
#[derive(Debug, Default)]
pub struct Ranking {
    /// file -> its score.
    pub score: IndexMap<String, f64>,
    /// (file, name) -> the share of rank that points at that name.
    pub names: BTreeMap<(String, String), f64>,
    /// (file, name) -> how many other files use the name.
    pub used_by: HashMap<(String, String), usize>,
}

fn round9(x: f64) -> f64 {
    py::round(x, 9)
}

/// Rank files and the names they define by how much other code leans on them.
///
/// The method is aider's repo map: an edge runs from the file that uses a name to the file that
/// defines it, with weight sqrt(uses) x10 if the task mentions the name, x10 if the name is long and
/// well formed, x0.1 if more than five files define it, x50 if the using file is in focus. Resolved
/// imports are added as edges too. PageRank then runs with its restart on the focus files, and each
/// file's score is shared out to the names it points at. A name nobody else uses gets a small share
/// (0.1) of its own file's score; unlike aider's self-edge it holds no rank back, so a file that
/// nothing uses cannot rise by being isolated.
///
/// Scores are rounded to nine places: the order of a ranking must not hang on the last bit of a sum.
pub fn ranking(
    files: &IndexMap<String, FileInfo>,
    edges: &IndexMap<String, Vec<String>>,
    focus: &HashSet<String>,
    mentioned: &HashSet<String>,
) -> Ranking {
    let code: Vec<&String> = files
        .iter()
        .filter(|(_, i)| i.code)
        .map(|(n, _)| n)
        .collect();
    if code.is_empty() {
        return Ranking::default();
    }
    let index: HashMap<&str, usize> = code
        .iter()
        .enumerate()
        .map(|(i, n)| (n.as_str(), i))
        .collect();
    let focus: HashSet<usize> = focus
        .iter()
        .filter_map(|f| index.get(f.as_str()).copied())
        .collect();
    let mut defines: IndexMap<String, BTreeSet<usize>> = IndexMap::new();
    let mut users: HashMap<&str, Vec<(usize, usize)>> = HashMap::new();
    for (i, n) in code.iter().enumerate() {
        for name in def_names(&files[*n]) {
            defines.entry(name).or_default().insert(i);
        }
        for (name, count) in &files[*n].uses {
            users.entry(name.as_str()).or_default().push((i, *count));
        }
    }
    // BTreeSet orders definers by their place in the file list, which is name order: the same sums
    // in the same order on every machine.
    let mut out: Vec<IndexMap<usize, f64>> = vec![IndexMap::new(); code.len()]; // user -> definer -> weight
    let mut via: IndexMap<(usize, usize), IndexMap<&str, f64>> = IndexMap::new(); // (user, definer) -> name -> weight
    let mut own: Vec<IndexMap<&str, f64>> = vec![IndexMap::new(); code.len()]; // file -> its names that nobody else uses
    let mut used_by: HashMap<(String, String), usize> = HashMap::new();
    for (name, definers) in &defines {
        let mul = (if mentioned.contains(name) { 10.0 } else { 1.0 })
            * (if well_named(name) { 10.0 } else { 1.0 })
            * (if definers.len() > 5 { 0.1 } else { 1.0 });
        let refs: Vec<(usize, usize)> = users
            .get(name.as_str())
            .map(|u| {
                u.iter()
                    .filter(|(user, _)| !definers.contains(user))
                    .copied()
                    .collect()
            })
            .unwrap_or_default();
        if refs.is_empty() {
            for d in definers {
                own[*d].insert(name.as_str(), 0.1);
            }
            continue;
        }
        for d in definers {
            used_by.insert((code[*d].clone(), name.clone()), refs.len());
        }
        for (user, count) in refs {
            let w = mul * (if focus.contains(&user) { 50.0 } else { 1.0 }) * (count as f64).sqrt();
            for d in definers {
                *out[user].entry(*d).or_insert(0.0) += w;
                *via.entry((user, *d))
                    .or_default()
                    .entry(name.as_str())
                    .or_insert(0.0) += w;
            }
        }
    }
    for (src, targets) in edges {
        for t in targets {
            if let (Some(&s), Some(&t)) = (index.get(src.as_str()), index.get(t.as_str())) {
                *out[s].entry(t).or_insert(0.0) += IMPORT_WEIGHT;
            }
        }
    }
    let lowered: HashSet<String> = mentioned.iter().map(|m| m.to_lowercase()).collect();
    // files that define a name the task mentions
    let home: HashSet<usize> = mentioned
        .iter()
        .flat_map(|name| defines.get(name).into_iter().flatten().copied())
        .collect();
    let pers: Vec<f64> = code
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let without_suffix = match n.rfind('/') {
                Some(cut) => format!("{}/{}", &n[..cut], stem(n)),
                None => stem(n).to_string(),
            };
            let named = without_suffix
                .split('/')
                .any(|p| lowered.contains(&p.to_lowercase()));
            f64::from(u8::from(focus.contains(&i)))
                + f64::from(u8::from(home.contains(&i)))
                + f64::from(u8::from(named))
        })
        .collect();
    let total: f64 = pers.iter().sum();
    let base: Vec<f64> = pers
        .iter()
        .map(|p| {
            if total != 0.0 {
                p / total
            } else {
                1.0 / code.len() as f64
            }
        })
        .collect();
    let mut score = base.clone();
    for _ in 0..40 {
        let mut next: Vec<f64> = base.iter().map(|b| 0.15 * b).collect();
        let mut dangling = 0.0;
        for n in 0..code.len() {
            let tot: f64 = out[n].values().sum();
            if tot != 0.0 {
                for (t, w) in &out[n] {
                    next[*t] += 0.85 * score[n] * w / tot;
                }
            } else {
                dangling += score[n];
            }
        }
        if dangling != 0.0 {
            for n in 0..code.len() {
                next[n] += 0.85 * dangling * base[n];
            }
        }
        score = next;
    }
    let share: Vec<f64> = (0..code.len())
        .map(|n| out[n].values().sum::<f64>() + own[n].values().sum::<f64>())
        .collect();
    let mut names: BTreeMap<(String, String), f64> = BTreeMap::new();
    for ((src, dst), idents) in &via {
        for (name, w) in idents {
            *names
                .entry((code[*dst].clone(), (*name).to_string()))
                .or_insert(0.0) += score[*src] * w / share[*src];
        }
    }
    for (n, idents) in own.iter().enumerate() {
        for (name, w) in idents {
            *names
                .entry((code[n].clone(), (*name).to_string()))
                .or_insert(0.0) += score[n] * w / share[n];
        }
    }
    Ranking {
        score: code
            .iter()
            .enumerate()
            .map(|(i, n)| ((*n).clone(), round9(score[i])))
            .collect(),
        names: names.into_iter().map(|(k, v)| (k, round9(v))).collect(),
        used_by,
    }
}

static TEST_FILE_RX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(^|/)(tests?|__tests__|specs?|testing)/|(^|/)(test_[^/]*|conftest)\.py$|_test\.[a-z0-9]+$|\.(test|spec)\.[a-z0-9]+$").expect("valid")
});

fn is_test_name(name: &str) -> bool {
    if name.starts_with("test_") {
        return true;
    }
    let after = |prefix: &str| name.strip_prefix(prefix).map(|rest| rest.chars().next());
    matches!(after("Test"), Some(None) | Some(Some('A'..='Z' | '_')))
        || matches!(after("test"), Some(Some('A'..='Z')))
}

/// What makes a file part of the STRUCTURE: its public names, signatures, imports and interfaces.
/// A change inside a function body leaves this alone, so it does not make the map stale. Nor does a
/// new test in a test file: the names of the tests are left out (the file, its imports and its
/// helpers still count), so that writing tests never costs a run of the mapper.
pub fn fingerprint(info: &FileInfo, name: &str) -> String {
    let tests = TEST_FILE_RX.is_match(name);
    let mut public: Vec<String> = info
        .symbols
        .iter()
        .filter(|s| !s.name.starts_with('_') && !(tests && is_test_name(&s.name)))
        .map(|s| {
            format!(
                "{} {} {}",
                s.kind,
                if s.sig.is_empty() { &s.name } else { &s.sig },
                s.methods.as_deref().unwrap_or(&[]).join(",")
            )
        })
        .collect();
    public.sort();
    let imports: BTreeSet<&String> = info.imports.iter().collect();
    let blob = Json::LINE.dumps(&json!([
        public,
        imports,
        info.env,
        info.cli,
        info.routes,
        info.tools,
        info.main
    ]));
    format!("{:x}", Sha1::digest(blob.as_bytes()))[..16].to_string()
}

// ----------------------------------------------------------------------------- state and staleness

/// Where the project's map lives: `members.map.path` in the config, else `docs/ARCHITECTURE.md`
/// when there is a `docs/` folder, else `ARCHITECTURE.md` at the top.
pub fn map_path(root: &Path) -> PathBuf {
    let cfg = core::config(root);
    let own = cfg.at("members").at("map").at("path");
    let conf = if py::truthy(own) {
        own
    } else {
        cfg.at("map").at("path")
    };
    if py::truthy(conf) {
        let conf = py::to_str(conf);
        let path = Path::new(&conf);
        if conf.ends_with(".md")
            && !path.is_absolute()
            && !path.components().any(|c| c.as_os_str() == "..")
        {
            return root.join(py::path(&conf));
        }
    }
    if root.join("docs").is_dir() {
        root.join("docs").join("ARCHITECTURE.md")
    } else {
        root.join("ARCHITECTURE.md")
    }
}

fn rel(path: &Path, root: &Path) -> String {
    py::posix(path.strip_prefix(root).unwrap_or(path))
}

fn state_file(root: &Path) -> PathBuf {
    root.join(".council").join("map").join("state.json")
}

/// file -> fingerprint, for the source files of a scan.
pub fn structure(scanned: &Scan) -> IndexMap<String, String> {
    scanned
        .files
        .iter()
        .filter(|(_, i)| i.code)
        .map(|(n, i)| (n.clone(), fingerprint(i, n)))
        .collect()
}

fn changes(before: &Value, now: &IndexMap<String, String>) -> Value {
    let old: BTreeSet<&String> = before
        .as_object()
        .into_iter()
        .flat_map(|m| m.keys())
        .collect();
    let new: BTreeSet<&String> = now.keys().collect();
    let added: Vec<&&String> = new.difference(&old).collect();
    let removed: Vec<&&String> = old.difference(&new).collect();
    let mut changed: Vec<&String> = now
        .iter()
        .filter(|(n, h)| {
            before
                .get(n.as_str())
                .is_some_and(|b| b.as_str() != Some(h.as_str()))
        })
        .map(|(n, _)| n)
        .collect();
    changed.sort();
    json!({"added": added, "removed": removed, "changed": changed})
}

/// The files behind a stale map, by name: `a.py, b.py and 4 more`.
fn which(diff: &Value, most: usize) -> String {
    let names: Vec<String> = ["changed", "added", "removed"]
        .iter()
        .flat_map(|kind| crate::val::strings(diff.at(kind)))
        .collect();
    let mut out = names
        .iter()
        .take(most)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if names.len() > most {
        out.push_str(&format!(" and {} more", names.len() - most));
    }
    out
}

/// Should the mapper run? Not when there is no code, and not when the structure is as it was mapped.
/// Returns `{"applies", "stale", "reason", "path", "diff"?, "mapped"?}`.
pub fn stale(root: &Path, scanned: &Scan) -> Value {
    let now = structure(scanned);
    let path = map_path(root);
    let shown_path = path.display().to_string();
    if now.is_empty() {
        return json!({"applies": false, "stale": false, "reason": "no source code found in this project", "path": shown_path});
    }
    let state: Value = core::read_text(&state_file(root))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| json!({}));
    if !path.exists() {
        return json!({"applies": true, "stale": true, "reason": format!("no map yet at {}", rel(&path, root)), "path": shown_path,
                      "diff": changes(&json!({}), &now)});
    }
    if !state.truthy("structure") {
        return json!({"applies": true, "stale": true, "reason": "the map was never checked against the code", "path": shown_path,
                      "diff": changes(&json!({}), &now)});
    }
    let diff = changes(state.at("structure"), &now);
    let n: usize = ["added", "removed", "changed"]
        .iter()
        .map(|k| diff.list(k).len())
        .sum();
    let mapped = state.show("mapped");
    let reason = if n > 0 {
        format!(
            "{n} file(s) changed in structure since the map of {mapped}: {}",
            which(&diff, 3)
        )
    } else {
        format!("the structure is as mapped on {mapped}")
    };
    json!({"applies": true, "stale": n > 0, "path": shown_path, "diff": diff, "mapped": state.at("mapped"), "reason": reason})
}

/// Record that the map describes the code as it is now.
pub fn accept(root: &Path, scanned: &Scan, run: Option<&str>) -> Result<()> {
    let state = json!({"mapped": crate::clock::today_iso(), "run": run, "map": rel(&map_path(root), root), "structure": structure(scanned)});
    core::write_text(&state_file(root), &Json::ONE.dumps(&state))
}

// ----------------------------------------------------------------------------- the skeleton

/// The numbers behind a skeleton.
#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    /// Files read.
    pub files: usize,
    /// Source files among them.
    pub code_files: usize,
    /// Lines in all of them.
    pub lines: usize,
    /// Imports resolved inside the repository.
    pub edges: usize,
    /// Commands found.
    pub commands: usize,
    /// MCP entries found.
    pub tools: usize,
}

/// A skeleton with what it was made from.
pub struct Skeleton {
    /// The skeleton as Markdown.
    pub markdown: String,
    /// The staleness verdict (see [`stale`]).
    pub stale: Value,
    /// The scan behind it.
    pub scanned: Scan,
    /// Counts for the one-line summary.
    pub stats: Stats,
}

fn ticks(names: &[String]) -> String {
    names
        .iter()
        .map(|n| format!("`{n}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn symbol_line(
    n: &str,
    s: &Symbol,
    used_by: &HashMap<(String, String), usize>,
    with_methods: bool,
) -> String {
    let mut extra = String::new();
    if with_methods {
        if let Some(methods) = s.methods.as_ref().filter(|m| !m.is_empty()) {
            extra.push_str(&format!(" (methods: {})", methods.join(", ")));
        }
    }
    if let Some(uses) = used_by
        .get(&(n.to_string(), s.name.clone()))
        .filter(|u| **u > 0)
    {
        extra.push_str(&format!(
            " · used in {uses} other file{}",
            if *uses == 1 { "" } else { "s" }
        ));
    }
    let sig = if s.sig.is_empty() { &s.name } else { &s.sig };
    format!(
        "`{sig}`{extra}{}",
        if s.doc.is_empty() {
            String::new()
        } else {
            format!(": {}", s.doc)
        }
    )
}

/// The skeleton as Markdown, plus the numbers behind it.
pub fn skeleton(root: &Path, budget: usize, scanned: Scan) -> Skeleton {
    let files = &scanned.files;
    let code: IndexMap<&String, &FileInfo> = files.iter().filter(|(_, i)| i.code).collect();
    let started = std::time::Instant::now();
    let edges = resolve_imports(files);
    let linked = started.elapsed().as_millis();
    let ranked_ = ranking(files, &edges, &HashSet::new(), &HashSet::new());
    crate::diag!(
        Debug,
        "map",
        "imports resolved in {linked} ms, files ranked in {} ms",
        started.elapsed().as_millis() - linked
    );
    let mut imported_by: HashMap<&str, usize> = HashMap::new();
    for outs in edges.values() {
        for t in outs {
            *imported_by.entry(t.as_str()).or_insert(0) += 1;
        }
    }
    let mut langs: IndexMap<&str, (usize, usize)> = IndexMap::new();
    for i in files.values() {
        let entry = langs.entry(i.lang).or_insert((0, 0));
        entry.0 += 1;
        entry.1 += i.lines;
    }
    let mut dirs: BTreeMap<String, (usize, usize, BTreeSet<&str>)> = BTreeMap::new();
    for (n, i) in files {
        let parts: Vec<&str> = n.split('/').collect();
        let d = parts[..parts.len() - 1]
            .iter()
            .take(3)
            .copied()
            .collect::<Vec<_>>()
            .join("/");
        let entry = dirs
            .entry(if d.is_empty() { ".".to_string() } else { d })
            .or_default();
        entry.0 += 1;
        entry.1 += i.lines;
        entry.2.insert(i.lang);
    }
    let started = std::time::Instant::now();
    let st = stale(root, &scanned);
    crate::diag!(
        Debug,
        "map",
        "compared with the map's record in {} ms",
        started.elapsed().as_millis()
    );
    let total_lines: usize = files.values().map(|i| i.lines).sum();
    let root_name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut md: Vec<String> = vec![
        format!("# Codebase skeleton: {root_name}"),
        String::new(),
        format!(
            "Made by `council map` on {}. A script wrote this. It lists what is there, not why: trust it for names and counts, and read the code for intent.",
            crate::clock::today_iso()
        ),
        String::new(),
        format!(
            "{} files, {} lines; {} are source code.{}",
            files.len(),
            py::commas(total_lines as i64),
            code.len(),
            if scanned.capped { " The file list was capped: this is a large repository." } else { "" }
        ),
        String::new(),
        "## Size".to_string(),
        String::new(),
        "| Language | Files | Lines |".to_string(),
        "| --- | ---: | ---: |".to_string(),
    ];
    let mut by_size: Vec<(&&str, &(usize, usize))> = langs.iter().collect();
    by_size.sort_by_key(|entry| std::cmp::Reverse(entry.1 .1));
    md.extend(
        by_size
            .iter()
            .map(|(lang, v)| format!("| {lang} | {} | {} |", v.0, py::commas(v.1 as i64))),
    );
    md.push(String::new());
    md.push("## Directories".to_string());
    md.push(String::new());
    md.extend(dirs.iter().take(40).map(|(d, v)| {
        format!(
            "- `{d}/` {} files, {} lines ({})",
            v.0,
            py::commas(v.1 as i64),
            v.2.iter().copied().collect::<Vec<_>>().join(", ")
        )
    }));
    let cli: BTreeSet<(&String, &String)> = code
        .iter()
        .flat_map(|(n, i)| i.cli.iter().map(move |c| (c, *n)))
        .collect();
    let mut sorted_code: Vec<(&String, &FileInfo)> = code.iter().map(|(n, i)| (*n, *i)).collect();
    sorted_code.sort_by(|a, b| a.0.cmp(b.0));
    let tools: Vec<(&String, &String)> = sorted_code
        .iter()
        .flat_map(|(n, i)| i.tools.iter().map(move |t| (t, *n)))
        .collect();
    let routes: Vec<(&String, &String)> = sorted_code
        .iter()
        .flat_map(|(n, i)| i.routes.iter().map(move |r| (r, *n)))
        .collect();
    let env: BTreeSet<&String> = code.values().flat_map(|i| i.env.iter()).collect();
    let mut mains: Vec<&String> = code
        .iter()
        .filter(|(_, i)| i.main)
        .map(|(n, _)| *n)
        .collect();
    mains.sort();
    md.push(String::new());
    md.push("## Interfaces found".to_string());
    md.push(String::new());
    md.push(
        "Found by pattern matching, so the lists can be incomplete: confirm in the code."
            .to_string(),
    );
    md.push(String::new());
    let mut by_file: IndexMap<&String, Vec<&str>> = IndexMap::new();
    for (c, n) in &cli {
        by_file.entry(n).or_default().push(c.as_str());
    }
    md.extend(
        by_file
            .iter()
            .map(|(n, cs)| format!("- Commands in `{n}`: {}", cs.join(", "))),
    );
    let mut by_file: IndexMap<&String, Vec<&str>> = IndexMap::new();
    for (t, n) in &tools {
        by_file.entry(n).or_default().push(t.as_str());
    }
    md.extend(
        by_file
            .iter()
            .map(|(n, ts)| format!("- MCP surface in `{n}`: {}", ts.join(", "))),
    );
    md.extend(
        routes
            .iter()
            .take(30)
            .map(|(r, n)| format!("- HTTP route `{r}` in `{n}`")),
    );
    if !env.is_empty() {
        md.push(format!(
            "- Environment variables read: {}",
            env.iter()
                .map(|e| format!("`{e}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !mains.is_empty() {
        md.push(format!(
            "- Runnable files (entry points): {}",
            mains
                .iter()
                .map(|n| format!("`{n}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if cli.is_empty() && tools.is_empty() && routes.is_empty() && env.is_empty() && mains.is_empty()
    {
        md.push("- none detected by the script".to_string());
    }
    md.push(String::new());
    md.push("## Key files, ranked by how much other code leans on them".to_string());
    md.push(String::new());
    md.push(
        "Ranked the way aider's repo map does it: a file scores when other files use the names it defines or import it. \
         Within a file, the names other files use most come first. Symbols read by: built-in readers."
            .to_string(),
    );
    md.push(String::new());
    let score = |n: &str| ranked_.score.get(n).copied().unwrap_or(0.0);
    let mut ranked: Vec<&String> = code.keys().copied().collect();
    ranked.sort_by(|a, b| {
        score(b)
            .partial_cmp(&score(a))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.cmp(b))
    });
    for (k, n) in ranked.iter().take(25).enumerate() {
        let i = code[*n];
        md.push(format!(
            "### {}. `{n}` · {} · {} lines · imported by {}",
            k + 1,
            i.lang,
            py::commas(i.lines as i64),
            imported_by.get(n.as_str()).copied().unwrap_or(0)
        ));
        if !i.doc.is_empty() {
            md.push(format!("\"{}\"", i.doc));
        }
        let name_rank = |s: &Symbol| {
            ranked_
                .names
                .get(&((*n).clone(), s.name.clone()))
                .copied()
                .unwrap_or(0.0)
        };
        let mut public = shown(i);
        // stable: file order breaks ties
        public.sort_by(|a, b| {
            name_rank(b)
                .partial_cmp(&name_rank(a))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        md.extend(
            public
                .iter()
                .take(14)
                .map(|s| format!("- {}", symbol_line(n, s, &ranked_.used_by, true))),
        );
        if public.len() > 14 {
            md.push(format!("- … and {} more public names", public.len() - 14));
        }
        md.push(String::new());
    }
    if ranked.len() > 25 {
        let rest: Vec<String> = ranked[25..].iter().take(60).map(|n| (*n).clone()).collect();
        md.push(format!(
            "Other source files ({}): {}{}",
            ranked.len() - 25,
            ticks(&rest),
            if ranked.len() > 85 { " …" } else { "" }
        ));
        md.push(String::new());
    }
    md.push("## Who imports whom (inside this repository)".to_string());
    md.push(String::new());
    let mut sorted_edges: Vec<(&String, &Vec<String>)> = edges.iter().collect();
    sorted_edges.sort_by(|a, b| a.0.cmp(b.0));
    if sorted_edges.is_empty() {
        md.push("- no internal imports found".to_string());
    }
    md.extend(
        sorted_edges
            .iter()
            .take(60)
            .map(|(src, outs)| format!("- `{src}` → {}", ticks(outs))),
    );
    let mut ext: HashMap<String, usize> = HashMap::new();
    for (n, i) in &code {
        let outside: HashSet<&str> = i
            .imports
            .iter()
            .filter(|x| !x.is_empty() && !x.starts_with('.'))
            .map(|x| {
                x.trim_start_matches('.')
                    .split('.')
                    .next()
                    .unwrap_or("")
                    .split('/')
                    .next()
                    .unwrap_or("")
            })
            .collect();
        for imp in outside {
            if n.ends_with(".py") && crate::pyread::STDLIB.contains(&imp) {
                continue; // the standard library is not a dependency
            }
            if !imp.is_empty()
                && !files
                    .keys()
                    .any(|f| stem(f) == imp || f.split('/').next() == Some(imp))
            {
                *ext.entry(imp.to_string()).or_insert(0) += 1;
            }
        }
    }
    let mut ext: Vec<(String, usize)> = ext.into_iter().collect();
    ext.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    md.push(String::new());
    md.push(
        "## Outside dependencies (imported names, with the number of files that use them)"
            .to_string(),
    );
    md.push(String::new());
    let listed: Vec<String> = ext
        .iter()
        .take(40)
        .map(|(k, v)| format!("{k} ({v})"))
        .collect();
    md.push(if listed.is_empty() {
        "none found".to_string()
    } else {
        listed.join(", ")
    });
    let mut tests: Vec<(&String, usize)> = code
        .iter()
        .filter(|(_, i)| i.tests > 0)
        .map(|(n, i)| (*n, i.tests))
        .collect();
    tests.sort();
    md.push(String::new());
    md.push("## Tests".to_string());
    md.push(String::new());
    let listed: Vec<String> = tests
        .iter()
        .take(20)
        .map(|(n, c)| format!("`{n}` ({c})"))
        .collect();
    md.push(format!(
        "{} test functions in {} files: {}",
        tests.iter().map(|(_, c)| c).sum::<usize>(),
        tests.len(),
        if listed.is_empty() {
            "none found".to_string()
        } else {
            listed.join(", ")
        }
    ));
    md.push(String::new());
    md.push("## Changed in structure since the last accepted map".to_string());
    md.push(String::new());
    if !st.truthy("applies") {
        md.push(format!("- not applicable: {}", st.s("reason")));
    } else if st.at("mapped").is_null() {
        md.push(format!("- {}: map all of it", st.s("reason")));
    } else {
        let d = st.at("diff");
        let mut any = false;
        for (key, label) in [
            ("added", "new files"),
            ("removed", "files that are gone"),
            (
                "changed",
                "files whose public names, imports or interfaces changed",
            ),
        ] {
            let names: Vec<String> = crate::val::strings(d.at(key))
                .into_iter()
                .take(40)
                .collect();
            if !d.list(key).is_empty() {
                md.push(format!("- {label}: {}", ticks(&names)));
                any = true;
            }
        }
        if !any {
            md.push("- nothing: the structure is as mapped".to_string());
        }
    }
    if md.len() > budget {
        md.truncate(budget);
        md.push(String::new());
        md.push(format!(
            "(cut at {budget} lines to save context; run `council map --budget N` for more)"
        ));
    }
    let stats = Stats {
        files: files.len(),
        code_files: code.len(),
        lines: total_lines,
        edges: edges.values().map(Vec::len).sum(),
        commands: cli.len(),
        tools: tools.len(),
    };
    Skeleton {
        markdown: format!("{}\n", md.join("\n")),
        stale: st,
        scanned,
        stats,
    }
}

// ----------------------------------------------------------------------------- the focus map

/// The result of [`focus_map`].
#[derive(Debug, Default)]
pub struct Focus {
    /// The ranked list, as Markdown.
    pub markdown: String,
    /// The files listed, in order.
    pub files: Vec<String>,
    /// About how many tokens the list is.
    pub tokens: usize,
    /// Names and files the ask mentions.
    pub mentioned: Vec<String>,
}

/// The code that matters most for ONE task, as a short list: aider's repo map, applied to an ask.
/// Names and files the ask mentions pull the ranking toward them; the list is cut to a token budget
/// (about four characters to a token).
pub fn focus_map(about: &str, paths: &[String], budget_tokens: usize, scanned: &Scan) -> Focus {
    let files = &scanned.files;
    let code: IndexMap<&String, &FileInfo> = files.iter().filter(|(_, i)| i.code).collect();
    if code.is_empty() {
        return Focus::default();
    }
    let words: HashSet<&str> = IDENT_RX.find_iter(about).map(|m| m.as_str()).collect();
    let defined: HashSet<String> = code.values().flat_map(|i| def_names(i)).collect();
    let stems: HashSet<String> = code
        .keys()
        .flat_map(|n| {
            let mut parts: Vec<String> = n.split('/').map(str::to_lowercase).collect();
            if let Some(last) = parts.last_mut() {
                *last = stem(n).to_lowercase();
            }
            parts
        })
        .collect();
    let mentioned: HashSet<String> = words
        .iter()
        .filter(|w| defined.contains(**w) || (stems.contains(&w.to_lowercase()) && py::len(w) >= 4))
        .map(|w| (*w).to_string())
        .collect();
    let strip: &[char] = &['`', '\'', '"', '.', ',', ';', ':', '(', ')'];
    let named: HashSet<String> = py::split(about)
        .map(|t| t.trim_matches(strip).to_string())
        .chain(paths.iter().cloned())
        .collect();
    let focus: HashSet<String> = code
        .keys()
        .filter(|n| {
            named.contains(n.as_str())
                || named
                    .iter()
                    .any(|t| (t.contains('/') || t.contains('.')) && n.ends_with(&format!("/{t}")))
        })
        .map(|n| (*n).clone())
        .collect();
    let res = ranking(files, &resolve_imports(files), &focus, &mentioned);
    let score = |n: &str| res.score.get(n).copied().unwrap_or(0.0);
    let sym: HashMap<(&str, &str), &Symbol> = code
        .iter()
        .flat_map(|(n, i)| {
            shown(i)
                .into_iter()
                .map(move |s| ((n.as_str(), s.name.as_str()), s))
        })
        .collect();
    let mut picked: IndexMap<String, Vec<&Symbol>> = IndexMap::new(); // a file the ask names is always listed
    for n in code.keys().filter(|n| focus.contains(n.as_str())) {
        picked.insert((*n).clone(), Vec::new());
    }
    let mut size: usize = picked.keys().map(|n| py::len(n) + 12).sum();
    let mut by_rank: Vec<(&(String, String), f64)> =
        res.names.iter().map(|(k, v)| (k, *v)).collect();
    by_rank.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(b.0))
    });
    // what the ask names comes first, whatever its rank
    let first: Vec<&(String, String)> = by_rank
        .iter()
        .map(|(k, _)| *k)
        .filter(|k| mentioned.contains(&k.1))
        .take(12)
        .collect();
    let rest = by_rank
        .iter()
        .map(|(k, _)| *k)
        .filter(|k| !first.contains(k));
    for (n, name) in first.iter().copied().chain(rest) {
        let Some(s) = sym.get(&(n.as_str(), name.as_str())) else {
            continue;
        }; // a method: its class line already names it
        if picked.get(n).is_some_and(|list| list.contains(s)) {
            continue;
        }
        let sig_len = py::len(if s.sig.is_empty() { name } else { &s.sig });
        let cost = sig_len
            + py::len(&s.doc)
            + 28
            + if picked.contains_key(n) {
                0
            } else {
                py::len(n) + 12
            };
        if (size + cost) as f64 / 4.0 > budget_tokens as f64 {
            break;
        }
        picked.entry(n.clone()).or_default().push(s);
        size += cost;
    }
    let lead: HashSet<&String> = first.iter().map(|k| &k.0).collect();
    let mut order: Vec<&String> = picked.keys().collect();
    order.sort_by(|a, b| {
        (!focus.contains(*a), !lead.contains(a))
            .cmp(&(!focus.contains(*b), !lead.contains(b)))
            .then(
                score(b)
                    .partial_cmp(&score(a))
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
            .then(a.cmp(b))
    });
    let mut md: Vec<String> = Vec::new();
    for n in &order {
        md.push(format!(
            "- `{n}`{}",
            if focus.contains(*n) {
                " (named in the ask)"
            } else {
                ""
            }
        ));
        md.extend(
            picked[*n]
                .iter()
                .map(|s| format!("  - {}", symbol_line(n, s, &res.used_by, false))),
        );
    }
    let text = md.join("\n");
    let mut mentioned: Vec<String> = mentioned.into_iter().collect();
    mentioned.sort();
    Focus {
        tokens: py::len(&text) / 4,
        markdown: text,
        files: order.into_iter().cloned().collect(),
        mentioned,
    }
}

// ----------------------------------------------------------------------------- a written map

/// Backticked tokens in a map that claim to be files or folders of this repository. Checked: anything
/// with a slash, and any bare name that ends like source code. Not checked: placeholders, flags, web
/// addresses, and run-time state under `.council/` (it is not in a fresh checkout).
fn named_paths(text: &str) -> Vec<String> {
    static FENCE_RX: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?s)```.*?```").expect("valid"));
    static PATH_RX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`([^`\s]+)`").expect("valid"));
    static LINE_RX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r":\d+(-\d+)?$").expect("valid"));
    let prose = FENCE_RX.replace_all(text, "");
    let mut out: BTreeSet<String> = BTreeSet::new();
    for m in PATH_RX.captures_iter(&prose) {
        let tok = m[1].trim_end_matches(['.', ',', ';', ':', ')']);
        let t = LINE_RX
            .replace(tok.split('#').next().unwrap_or(""), "")
            .into_owned();
        if t.contains(['<', '>', '*', '{', '}', '$', '|', '=', '(', ')']) || t.contains("://") {
            continue;
        }
        if t.starts_with('-')
            || t.starts_with('~')
            || t.starts_with('/')
            || t.starts_with(".council")
        {
            continue;
        }
        if t.trim_matches('/').contains('/') || t.ends_with('/') || code_lang(&suffix(&t)).is_some()
        {
            out.insert(t);
        }
    }
    out.into_iter().collect()
}

/// Check a written map. Errors send the mapper back; warnings go to the chair. Dead paths are the
/// main thing: a map that names files which do not exist was written from imagination.
pub fn validate_map(text: &str, root: &Path) -> (Vec<String>, Vec<String>) {
    static HEAD_RX: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?m)^##\s+(.+?)\s*$").expect("valid"));
    static ELI5_RX: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?im)^>?\s*\**ELI5\b").expect("valid"));
    let (mut errs, mut warns): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    let words = py::words(text);
    if words < 200 {
        errs.push(format!(
            "the map is {words} words: too thin to find one's way by"
        ));
    } else if words > 3500 {
        warns.push(format!(
            "the map is {words} words (aim for 600 to 2,000: a map, not a transcript)"
        ));
    }
    let heads: Vec<String> = HEAD_RX
        .captures_iter(text)
        .map(|m| py::strip(&m[1]).to_lowercase())
        .collect();
    for need in REQUIRED {
        if !heads.iter().any(|h| h.contains(&need.to_lowercase())) {
            errs.push(format!("missing section: ## {need}"));
        }
    }
    for want in ADVISED {
        if !heads.iter().any(|h| h.contains(&want.to_lowercase())) {
            warns.push(format!("no section: ## {want}"));
        }
    }
    if !text.contains("```mermaid") {
        warns.push("no Mermaid diagram in the bird's-eye view".to_string());
    }
    if !ELI5_RX.is_match(text) {
        warns.push("no ELI5 line under the title".to_string());
    }
    let (files, _) = list_files(root);
    let mut known: HashSet<String> = files.iter().cloned().collect();
    let mut names: HashSet<&str> = HashSet::new();
    for f in &files {
        let parts: Vec<&str> = f.split('/').collect();
        for k in 1..parts.len() {
            known.insert(parts[..k].join("/"));
        }
        names.insert(parts[parts.len() - 1]);
    }
    let exists = |p: &str| {
        let q = p.trim_matches('/');
        if !q.contains('/') {
            return names.contains(q) || known.contains(q);
        }
        if known.contains(q) {
            return true;
        }
        let tail = format!("/{q}");
        known.iter().any(|k| k.ends_with(&tail)) || root.join(q).exists()
    };
    let dead: Vec<String> = named_paths(text)
        .into_iter()
        .filter(|p| !exists(p))
        .collect();
    if dead.len() >= 3 {
        errs.push(format!(
            "these paths are named in the map but do not exist: {}",
            dead.iter().take(12).cloned().collect::<Vec<_>>().join(", ")
        ));
    } else if !dead.is_empty() {
        warns.push(format!(
            "named in the map but not found: {}",
            dead.join(", ")
        ));
    }
    (errs, warns)
}

/// Put the map in place and record what it was checked against. A map that a person wrote
/// (no marker on its first lines) is never overwritten: the new one lands beside it.
/// Returns where it was put and, when a hand-written map was left alone, a note saying so.
pub fn install_map(
    text: &str,
    root: &Path,
    run: Option<&str>,
    scanned: &Scan,
    by: &str,
) -> Result<(PathBuf, Option<String>)> {
    static HEAD_RX: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?s)\A\s*<!-- council-loop:map.*?-->\s*").expect("valid"));
    let mut path = map_path(root);
    let mut note = None;
    if path.exists() {
        let first: Vec<String> = py::lines(&core::read_text(&path).unwrap_or_default())
            .into_iter()
            .take(5)
            .map(str::to_string)
            .collect();
        if !first.join("\n").contains(MARKER) {
            note = Some(format!(
                "{} was written by hand, so it was left alone",
                rel(&path, root)
            ));
            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            path = path.with_file_name(format!("{stem}.council.md"));
        }
    }
    let body = HEAD_RX.replace(text, "");
    let body = py::strip(&body);
    let head = format!(
        "{MARKER} · written by {by}{} on {} · checked against {} source files · regenerate: council map -->",
        run.map(|r| format!(" in run {r}")).unwrap_or_default(),
        crate::clock::today_iso(),
        structure(scanned).len()
    );
    core::write_text(&path, &format!("{head}\n{body}\n"))?;
    accept(root, scanned, run)?;
    Ok((path, note))
}

// ----------------------------------------------------------------------------- CLI glue

/// The options of `council map`.
#[derive(Clone, Debug, Default)]
pub struct MapArgs {
    /// Exit 1 when the map is missing or behind the code.
    pub check: bool,
    /// Record that the map describes the code as it is now.
    pub accept: bool,
    /// Check the written map.
    pub validate: bool,
    /// Where to write the skeleton.
    pub out: Option<String>,
    /// Skeleton length in lines.
    pub budget: i64,
    /// A written map to validate and put in place.
    pub install: Option<String>,
    /// Say which readers this build has.
    pub parsers: bool,
    /// A task to print the focus map for.
    pub focus: Option<String>,
}

/// `council map`.
pub fn cmd_map(args: &MapArgs) -> Result<i32> {
    let root = core::project_root();
    if args.parsers {
        say!("this build reads Python with tree-sitter and the other languages with built-in pattern readers: there is nothing to download");
        return Ok(0);
    }
    if let Some(focus) = &args.focus {
        let fm = focus_map(focus, &[], 700, &scan(&root));
        say!(
            "{}",
            if fm.markdown.is_empty() {
                "no source code found in this project"
            } else {
                &fm.markdown
            }
        );
        say!(
            "\n(about {} tokens; names the text mentions: {})",
            fm.tokens,
            if fm.mentioned.is_empty() {
                "none".to_string()
            } else {
                fm.mentioned.join(", ")
            }
        );
        return Ok(0);
    }
    let budget = usize::try_from(args.budget).unwrap_or(0);
    let sk = skeleton(&root, budget, scan(&root));
    let path = map_path(&root);
    let print_findings = |errs: &[String], warns: &[String]| {
        for w in warns {
            say!("warning: {w}");
        }
        for e in errs {
            say!("error: {e}");
        }
    };
    if let Some(install) = &args.install {
        let src = Path::new(install);
        if !src.exists() && !crate::safe::is_link(src) {
            say!("no file at {install}");
            return Ok(1);
        }
        if crate::safe::plain_file(src, crate::safe::MAX_OUTPUT).is_none() {
            say!("{install} is not a plain file of a map's size");
            return Ok(1);
        }
        let text = crate::safe::unfence(&core::read_text(src)?);
        let text = crate::safe::shown(&text).into_owned();
        let (errs, warns) = validate_map(&text, &root);
        print_findings(&errs, &warns);
        if !errs.is_empty() {
            say!("not installed: {} problem(s)", errs.len());
            return Ok(1);
        }
        let (placed, note) = install_map(
            &text,
            &root,
            None,
            &sk.scanned,
            "hand (council map --install)",
        )?;
        say!(
            "map installed: {}{}",
            rel(&placed, &root),
            note.map(|n| format!(" ({n})")).unwrap_or_default()
        );
        let conf = crate::memory::settings(Some(&root));
        match crate::memory::publish_map(
            &crate::safe::read_own(&placed)?,
            &rel(&placed, &root),
            &conf,
        ) {
            Ok(Some(copy)) => say!("a copy for Obsidian: {}", copy.display()),
            Ok(None) => {}
            Err(err) => say!(
                "note: no copy in the shared memory ({})",
                py::head(&err.to_string(), 120)
            ),
        }
        return Ok(0);
    }
    if args.validate {
        if !path.exists() {
            say!("no map at {}", rel(&path, &root));
            return Ok(1);
        }
        let (errs, warns) = validate_map(
            &crate::safe::read_capped(&path, crate::safe::MAX_OUTPUT)?,
            &root,
        );
        print_findings(&errs, &warns);
        if errs.is_empty() {
            say!("map: ok");
        } else {
            say!("map: {} problem(s)", errs.len());
        }
        return Ok(i32::from(!errs.is_empty()));
    }
    if args.accept {
        if !path.exists() {
            say!("no map at {} to accept", rel(&path, &root));
            return Ok(1);
        }
        accept(&root, &sk.scanned, None)?;
        say!(
            "recorded: {} describes the code as it is today",
            rel(&path, &root)
        );
        return Ok(0);
    }
    let target = match &args.out {
        Some(out) => {
            // a path the person named is theirs to choose
            let target = py::plain(out);
            crate::safe::write_named(&target, &sk.markdown)?;
            target
        }
        None => {
            let target = root.join(".council").join("map").join("skeleton.md");
            core::write_text(&target, &sk.markdown)?;
            target
        }
    };
    let s = sk.stats;
    say!(
        "skeleton: {}  ({} files, {} lines, {} source files, {} internal imports, {} commands, {} MCP entries)",
        target.display(),
        s.files,
        py::commas(s.lines as i64),
        s.code_files,
        s.edges,
        s.commands,
        s.tools
    );
    let is_stale = sk.stale.truthy("stale");
    say!(
        "map: {}: {}{}",
        rel(&path, &root),
        if is_stale { "STALE: " } else { "" },
        sk.stale.s("reason")
    );
    Ok(i32::from(args.check && is_stale))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suffixes_and_first_lines() {
        assert_eq!(
            (
                suffix("a/b/c.PY"),
                suffix(".gitignore"),
                suffix("x."),
                suffix("a.tar.gz")
            ),
            (".py".into(), String::new(), String::new(), ".gz".into())
        );
        assert_eq!(
            (
                stem("a/b/c.py"),
                stem(".env"),
                parent("a/b/c.py"),
                parent("c.py")
            ),
            ("c", ".env", "a/b", "")
        );
        assert_eq!(first_line("\n\n  Hello there  \nmore", 110), "Hello there");
        assert_eq!(first_line("abcdefghij", 5), "abcd…");
        assert!(
            well_named("snake_case_name")
                && well_named("camelCaseName")
                && !well_named("short_a")
                && !well_named("lowercaseonly")
        );
        assert!(
            is_test_name("test_x")
                && is_test_name("TestCart")
                && is_test_name("Test")
                && is_test_name("testCart")
        );
        assert!(!is_test_name("Testing") && !is_test_name("tester") && !is_test_name("contest"));
    }

    #[test]
    fn pattern_readers_find_public_names() {
        let js = extract_js("/** Cart things.\n * more */\nimport { x } from './util';\nexport function total(items) {}\nexport default class Cart {}\nconst hidden = 1;\n");
        assert_eq!(js.doc, "Cart things.");
        assert_eq!(
            js.symbols
                .iter()
                .map(|s| (s.name.as_str(), s.kind.as_str()))
                .collect::<Vec<_>>(),
            [("total", "function"), ("Cart", "class")]
        );
        assert_eq!(js.imports, ["./util"]);
        let go = extract_go("package shop\n\nimport (\n\t\"fmt\"\n\tq \"shop/queue\"\n)\n\nfunc Total(items []int) int {\n}\nfunc hidden() {}\ntype Cart struct {\n}\nfunc main() {}\n");
        assert_eq!(
            go.symbols
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>(),
            ["Total", "Cart"]
        );
        assert_eq!(
            (go.imports.clone(), go.main),
            (vec!["fmt".to_string(), "shop/queue".to_string()], true)
        );
        let rs = extract_rust("use std::fs;\npub mod cart;\npub(crate) fn total() {}\nfn private() {}\npub struct Cart;\nfn main() {}\n");
        assert_eq!(
            rs.symbols
                .iter()
                .map(|s| s.sig.as_str())
                .collect::<Vec<_>>(),
            ["pub mod cart", "pub(crate) fn total", "pub struct Cart"]
        );
        assert_eq!(
            (rs.imports.clone(), rs.main),
            (vec!["std::fs".to_string(), "cart".to_string()], true)
        );
        let c = extract_generic("#include \"cart.h\"\nstatic int total_items(struct cart *c)\n{\n}\nif (x) {\n}\nstruct cart {\n};\nint main(void) {\n}\nCREATE TABLE IF NOT EXISTS orders (\n");
        assert_eq!(
            c.symbols
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>(),
            ["total_items", "main", "cart", "orders"]
        );
        assert_eq!(
            (c.imports.clone(), c.main),
            (vec!["cart.h".to_string()], true)
        );
    }

    #[test]
    fn words_in_comments_and_strings_are_not_uses() {
        let uses = code_words("// total_items in a comment\nlet x = total_items(\"total_items\") + other_name; /* total_items */", ".js");
        assert_eq!(uses.get("total_items"), Some(&1));
        assert_eq!(uses.get("other_name"), Some(&1));
    }
}
