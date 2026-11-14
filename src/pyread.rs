//! Reading Python source: public names with their signatures, imports, and the names the code uses.
//!
//! The Python reference does this with the standard library's `ast` module. Here the same facts come
//! from a tree-sitter parse, laid out the way `ast.unparse` lays them out, so that a signature reads
//! the same from either implementation in ordinary code.

use crate::codemap::{first_line, Symbol};
use crate::py;
use indexmap::IndexMap;
use std::collections::HashMap;
use tree_sitter::{Node, Parser};

/// What one Python file defines and leans on.
#[derive(Debug, Default)]
pub struct PyFile {
    /// First line of the module docstring.
    pub doc: String,
    /// Top-level functions and classes, in file order.
    pub symbols: Vec<Symbol>,
    /// Imported modules and names, as written (`pkg.mod`, `.sibling.name`).
    pub imports: Vec<String>,
    /// Is there an `if __name__ == "__main__":` block?
    pub main: bool,
    /// Names the code uses (never a comment or a string), with counts.
    pub uses: IndexMap<String, usize>,
}

/// Modules of Python's standard library (`sys.stdlib_module_names`, Python 3.13): `re.search` is not
/// a use of this project's `search`.
pub const STDLIB: &[&str] = &[
    "__future__",
    "_abc",
    "_aix_support",
    "_android_support",
    "_apple_support",
    "_ast",
    "_asyncio",
    "_bisect",
    "_blake2",
    "_bz2",
    "_codecs",
    "_codecs_cn",
    "_codecs_hk",
    "_codecs_iso2022",
    "_codecs_jp",
    "_codecs_kr",
    "_codecs_tw",
    "_collections",
    "_collections_abc",
    "_colorize",
    "_compat_pickle",
    "_compression",
    "_contextvars",
    "_csv",
    "_ctypes",
    "_curses",
    "_curses_panel",
    "_datetime",
    "_dbm",
    "_decimal",
    "_elementtree",
    "_frozen_importlib",
    "_frozen_importlib_external",
    "_functools",
    "_gdbm",
    "_hashlib",
    "_heapq",
    "_imp",
    "_interpchannels",
    "_interpqueues",
    "_interpreters",
    "_io",
    "_ios_support",
    "_json",
    "_locale",
    "_lsprof",
    "_lzma",
    "_markupbase",
    "_md5",
    "_multibytecodec",
    "_multiprocessing",
    "_opcode",
    "_opcode_metadata",
    "_operator",
    "_osx_support",
    "_overlapped",
    "_pickle",
    "_posixshmem",
    "_posixsubprocess",
    "_py_abc",
    "_pydatetime",
    "_pydecimal",
    "_pyio",
    "_pylong",
    "_pyrepl",
    "_queue",
    "_random",
    "_scproxy",
    "_sha1",
    "_sha2",
    "_sha3",
    "_signal",
    "_sitebuiltins",
    "_socket",
    "_sqlite3",
    "_sre",
    "_ssl",
    "_stat",
    "_statistics",
    "_string",
    "_strptime",
    "_struct",
    "_suggestions",
    "_symtable",
    "_sysconfig",
    "_thread",
    "_threading_local",
    "_tkinter",
    "_tokenize",
    "_tracemalloc",
    "_typing",
    "_uuid",
    "_warnings",
    "_weakref",
    "_weakrefset",
    "_winapi",
    "_wmi",
    "_zoneinfo",
    "abc",
    "antigravity",
    "argparse",
    "array",
    "ast",
    "asyncio",
    "atexit",
    "base64",
    "bdb",
    "binascii",
    "bisect",
    "builtins",
    "bz2",
    "cProfile",
    "calendar",
    "cmath",
    "cmd",
    "code",
    "codecs",
    "codeop",
    "collections",
    "colorsys",
    "compileall",
    "concurrent",
    "configparser",
    "contextlib",
    "contextvars",
    "copy",
    "copyreg",
    "csv",
    "ctypes",
    "curses",
    "dataclasses",
    "datetime",
    "dbm",
    "decimal",
    "difflib",
    "dis",
    "doctest",
    "email",
    "encodings",
    "ensurepip",
    "enum",
    "errno",
    "faulthandler",
    "fcntl",
    "filecmp",
    "fileinput",
    "fnmatch",
    "fractions",
    "ftplib",
    "functools",
    "gc",
    "genericpath",
    "getopt",
    "getpass",
    "gettext",
    "glob",
    "graphlib",
    "grp",
    "gzip",
    "hashlib",
    "heapq",
    "hmac",
    "html",
    "http",
    "idlelib",
    "imaplib",
    "importlib",
    "inspect",
    "io",
    "ipaddress",
    "itertools",
    "json",
    "keyword",
    "linecache",
    "locale",
    "logging",
    "lzma",
    "mailbox",
    "marshal",
    "math",
    "mimetypes",
    "mmap",
    "modulefinder",
    "msvcrt",
    "multiprocessing",
    "netrc",
    "nt",
    "ntpath",
    "nturl2path",
    "numbers",
    "opcode",
    "operator",
    "optparse",
    "os",
    "pathlib",
    "pdb",
    "pickle",
    "pickletools",
    "pkgutil",
    "platform",
    "plistlib",
    "poplib",
    "posix",
    "posixpath",
    "pprint",
    "profile",
    "pstats",
    "pty",
    "pwd",
    "py_compile",
    "pyclbr",
    "pydoc",
    "pydoc_data",
    "pyexpat",
    "queue",
    "quopri",
    "random",
    "re",
    "readline",
    "reprlib",
    "resource",
    "rlcompleter",
    "runpy",
    "sched",
    "secrets",
    "select",
    "selectors",
    "shelve",
    "shlex",
    "shutil",
    "signal",
    "site",
    "smtplib",
    "socket",
    "socketserver",
    "sqlite3",
    "sre_compile",
    "sre_constants",
    "sre_parse",
    "ssl",
    "stat",
    "statistics",
    "string",
    "stringprep",
    "struct",
    "subprocess",
    "symtable",
    "sys",
    "sysconfig",
    "syslog",
    "tabnanny",
    "tarfile",
    "tempfile",
    "termios",
    "textwrap",
    "this",
    "threading",
    "time",
    "timeit",
    "tkinter",
    "token",
    "tokenize",
    "tomllib",
    "trace",
    "traceback",
    "tracemalloc",
    "tty",
    "turtle",
    "turtledemo",
    "types",
    "typing",
    "unicodedata",
    "unittest",
    "urllib",
    "uuid",
    "venv",
    "warnings",
    "wave",
    "weakref",
    "webbrowser",
    "winreg",
    "winsound",
    "wsgiref",
    "xml",
    "xmlrpc",
    "zipapp",
    "zipfile",
    "zipimport",
    "zlib",
    "zoneinfo",
];

fn text<'a>(node: Node, src: &'a str) -> &'a str {
    node.utf8_text(src.as_bytes()).unwrap_or("")
}

fn named<'t>(node: Node<'t>) -> Vec<Node<'t>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|c| c.kind() != "comment")
        .collect()
}

fn all<'t>(node: Node<'t>) -> Vec<Node<'t>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).collect()
}

/// The value of a simple escape-free reading of a string literal: `None` for bytes and f-strings,
/// which are not docstrings and which `repr` would print differently.
fn string_value(node: Node, src: &str) -> Option<String> {
    match node.kind() {
        "concatenated_string" => named(node)
            .into_iter()
            .map(|part| string_value(part, src))
            .collect::<Option<Vec<_>>>()
            .map(|p| p.concat()),
        "string" => {
            let parts = all(node);
            let start = parts
                .iter()
                .find(|c| c.kind() == "string_start")
                .map(|c| text(*c, src))
                .unwrap_or("");
            let prefix: String = start
                .chars()
                .take_while(|c| c.is_ascii_alphabetic())
                .collect::<String>()
                .to_lowercase();
            if prefix.contains('b') || prefix.contains('f') {
                return None;
            }
            let raw: String = parts
                .iter()
                .filter(|c| c.kind() == "string_content")
                .map(|c| text(*c, src))
                .collect();
            Some(if prefix.contains('r') {
                raw
            } else {
                unescape(&raw)
            })
        }
        _ => None,
    }
}

fn unescape(raw: &str) -> String {
    if !raw.contains('\\') {
        return raw.to_string();
    }
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\n') => {}
            Some('0') => out.push('\0'),
            Some(q @ ('\\' | '\'' | '"')) => out.push(q),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// `ast.get_docstring(node)`, as far as its first line: the string that opens a module, class or function.
fn docstring(body: Node, src: &str) -> Option<String> {
    let first = *named(body).first()?;
    if first.kind() != "expression_statement" {
        return None;
    }
    let inner = named(first);
    if inner.len() != 1 {
        return None;
    }
    string_value(inner[0], src)
}

fn number(node: Node, src: &str) -> String {
    let raw = text(node, src);
    let clean = raw.replace('_', "").to_lowercase();
    if node.kind() == "integer" {
        let parsed = if let Some(hex) = clean.strip_prefix("0x") {
            u128::from_str_radix(hex, 16).ok()
        } else if let Some(oct) = clean.strip_prefix("0o") {
            u128::from_str_radix(oct, 8).ok()
        } else if let Some(bin) = clean.strip_prefix("0b") {
            u128::from_str_radix(bin, 2).ok()
        } else {
            clean.parse().ok()
        };
        return parsed
            .map(|n| n.to_string())
            .unwrap_or_else(|| raw.to_string());
    }
    clean
        .parse::<f64>()
        .map(py::float_repr)
        .unwrap_or_else(|_| raw.to_string())
}

fn joined(nodes: &[Node], src: &str) -> String {
    nodes
        .iter()
        .map(|n| expr(*n, src))
        .collect::<Vec<_>>()
        .join(", ")
}

/// An expression the way `ast.unparse` writes it, for the forms that occur in signatures. Anything
/// else is the source text with its white space folded.
fn expr(node: Node, src: &str) -> String {
    let kids = named(node);
    let field = |name: &str| node.child_by_field_name(name);
    match node.kind() {
        "identifier" => text(node, src).to_string(),
        "type" | "expression_statement" => kids.first().map(|k| expr(*k, src)).unwrap_or_default(),
        "attribute" | "member_type" => match (
            field("object").or(kids.first().copied()),
            field("attribute").or(kids.last().copied()),
        ) {
            (Some(object), Some(attr)) => format!("{}.{}", expr(object, src), text(attr, src)),
            _ => py::squash(text(node, src)),
        },
        "subscript" => {
            let value = field("value").map(|v| expr(v, src)).unwrap_or_default();
            let mut cursor = node.walk();
            let subs: Vec<Node> = node
                .children_by_field_name("subscript", &mut cursor)
                .collect();
            format!("{value}[{}]", joined(&subs, src))
        }
        "generic_type" => match kids.as_slice() {
            [name, params] => format!("{}[{}]", expr(*name, src), joined(&named(*params), src)),
            _ => py::squash(text(node, src)),
        },
        "union_type" => kids
            .iter()
            .map(|k| expr(*k, src))
            .collect::<Vec<_>>()
            .join(" | "),
        "slice" => text(node, src)
            .split(':')
            .map(py::strip)
            .collect::<Vec<_>>()
            .join(":"),
        "string" | "concatenated_string" => match string_value(node, src) {
            Some(value) => py::str_repr(&value),
            None => py::squash(text(node, src)),
        },
        "integer" | "float" => number(node, src),
        "true" => "True".to_string(),
        "false" => "False".to_string(),
        "none" => "None".to_string(),
        "ellipsis" => "...".to_string(),
        "tuple" => match kids.len() {
            0 => "()".to_string(),
            1 => format!("({},)", expr(kids[0], src)),
            _ => format!("({})", joined(&kids, src)),
        },
        "list" => format!("[{}]", joined(&kids, src)),
        "set" => format!("{{{}}}", joined(&kids, src)),
        "dictionary" => format!("{{{}}}", joined(&kids, src)),
        "pair" => match (field("key"), field("value")) {
            (Some(k), Some(v)) => format!("{}: {}", expr(k, src), expr(v, src)),
            _ => py::squash(text(node, src)),
        },
        "list_splat" | "list_splat_pattern" => format!(
            "*{}",
            kids.first().map(|k| expr(*k, src)).unwrap_or_default()
        ),
        "dictionary_splat" | "dictionary_splat_pattern" => format!(
            "**{}",
            kids.first().map(|k| expr(*k, src)).unwrap_or_default()
        ),
        "keyword_argument" => match (field("name"), field("value")) {
            (Some(k), Some(v)) => format!("{}={}", text(k, src), expr(v, src)),
            _ => py::squash(text(node, src)),
        },
        "call" => match (field("function"), field("arguments")) {
            (Some(f), Some(args)) if args.kind() == "argument_list" => {
                format!("{}({})", expr(f, src), joined(&named(args), src))
            }
            _ => py::squash(text(node, src)),
        },
        "parenthesized_expression" => match kids.first() {
            Some(inner) if matches!(inner.kind(), "tuple" | "generator_expression") => {
                expr(*inner, src)
            }
            Some(inner) => {
                let loose = matches!(
                    inner.kind(),
                    "binary_operator"
                        | "boolean_operator"
                        | "comparison_operator"
                        | "conditional_expression"
                        | "lambda"
                        | "not_operator"
                        | "unary_operator"
                );
                let tight = node.parent().is_some_and(|p| {
                    matches!(
                        p.kind(),
                        "binary_operator"
                            | "unary_operator"
                            | "comparison_operator"
                            | "boolean_operator"
                            | "not_operator"
                            | "attribute"
                            | "call"
                            | "subscript"
                    )
                });
                if loose && tight {
                    format!("({})", expr(*inner, src))
                } else {
                    expr(*inner, src)
                }
            }
            None => "()".to_string(),
        },
        "binary_operator" | "boolean_operator" => {
            match (field("left"), field("operator"), field("right")) {
                (Some(l), Some(op), Some(r)) => {
                    format!("{} {} {}", expr(l, src), text(op, src), expr(r, src))
                }
                _ => py::squash(text(node, src)),
            }
        }
        "unary_operator" => match (field("operator"), field("argument")) {
            (Some(op), Some(arg)) => format!("{}{}", text(op, src), expr(arg, src)),
            _ => py::squash(text(node, src)),
        },
        "not_operator" => format!(
            "not {}",
            field("argument").map(|a| expr(a, src)).unwrap_or_default()
        ),
        "comparison_operator" => all(node)
            .into_iter()
            .filter(|c| c.kind() != "comment")
            .map(|c| {
                if c.is_named() {
                    expr(c, src)
                } else {
                    text(c, src).to_string()
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
        "conditional_expression" => match kids.as_slice() {
            [a, c, b] => format!(
                "{} if {} else {}",
                expr(*a, src),
                expr(*c, src),
                expr(*b, src)
            ),
            _ => py::squash(text(node, src)),
        },
        "lambda" => {
            let params = field("parameters")
                .map(|p| params_text(p, src))
                .unwrap_or_default();
            let body = field("body").map(|b| expr(b, src)).unwrap_or_default();
            if params.is_empty() {
                format!("lambda: {body}")
            } else {
                format!("lambda {params}: {body}")
            }
        }
        _ => py::squash(text(node, src)),
    }
}

/// The parameters of a function the way `ast.unparse` writes them: `a, b: int=2, *args, c=None, **kw`.
fn params_text(params: Node, src: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for p in named(params) {
        let kids = named(p);
        let field = |name: &str| p.child_by_field_name(name);
        let item = match p.kind() {
            "identifier" => text(p, src).to_string(),
            "typed_parameter" => {
                let name = kids
                    .first()
                    .map(|k| {
                        if k.kind() == "identifier" {
                            text(*k, src).to_string()
                        } else {
                            expr(*k, src)
                        }
                    })
                    .unwrap_or_default();
                let kind = field("type")
                    .or(kids.last().copied())
                    .map(|t| expr(t, src))
                    .unwrap_or_default();
                format!("{name}: {kind}")
            }
            "default_parameter" => match (field("name"), field("value")) {
                (Some(n), Some(v)) => format!("{}={}", text(n, src), expr(v, src)),
                _ => py::squash(text(p, src)),
            },
            "typed_default_parameter" => match (field("name"), field("type"), field("value")) {
                (Some(n), Some(t), Some(v)) => {
                    format!("{}: {}={}", text(n, src), expr(t, src), expr(v, src))
                }
                _ => py::squash(text(p, src)),
            },
            "list_splat_pattern" | "dictionary_splat_pattern" => expr(p, src),
            "keyword_separator" => "*".to_string(),
            "positional_separator" => "/".to_string(),
            _ => py::squash(text(p, src)),
        };
        out.push(item);
    }
    out.join(", ")
}

fn unwrap_decorated(node: Node) -> Node {
    if node.kind() == "decorated_definition" {
        node.child_by_field_name("definition").unwrap_or(node)
    } else {
        node
    }
}

fn doc_line(body: Option<Node>, src: &str, cap: usize) -> String {
    first_line(
        &body.and_then(|b| docstring(b, src)).unwrap_or_default(),
        cap,
    )
}

/// Is this identifier what `ast` calls a `Name`: a variable read, written or deleted? Not a name
/// being defined, a parameter, an attribute, a keyword in a call, or part of an import.
fn is_name(node: Node) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    let is_field = |name: &str| {
        parent
            .child_by_field_name(name)
            .is_some_and(|c| c.id() == node.id())
    };
    match parent.kind() {
        "function_definition" | "class_definition" => !is_field("name"),
        "parameters" | "lambda_parameters" | "typed_parameter" => false,
        "default_parameter" | "typed_default_parameter" | "keyword_argument" => !is_field("name"),
        "attribute" => !is_field("attribute"),
        "dotted_name" | "aliased_import" | "global_statement" | "nonlocal_statement"
        | "import_prefix" | "type_parameter" => false,
        "list_splat_pattern" | "dictionary_splat_pattern" => !parent.parent().is_some_and(|g| {
            matches!(
                g.kind(),
                "parameters" | "lambda_parameters" | "typed_parameter"
            )
        }),
        "except_clause" | "except_group_clause" => !is_field("alias"),
        "as_pattern_target" => !parent
            .parent()
            .and_then(|as_pattern| as_pattern.parent())
            .is_some_and(|holder| matches!(holder.kind(), "except_clause" | "except_group_clause")),
        _ => true,
    }
}

fn first_part(name: &str) -> String {
    name.split('.').next().unwrap_or("").to_string()
}

fn import_names<'t>(node: Node<'t>, skip: Option<Node<'t>>) -> Vec<(Node<'t>, Option<Node<'t>>)> {
    named(node)
        .into_iter()
        .filter(|c| Some(c.id()) != skip.map(|s| s.id()))
        .filter_map(|c| match c.kind() {
            "dotted_name" | "identifier" => Some((c, None)),
            "aliased_import" => c
                .child_by_field_name("name")
                .map(|n| (n, c.child_by_field_name("alias"))),
            _ => None,
        })
        .collect()
}

fn walk(
    node: Node,
    src: &str,
    out: &mut PyFile,
    alias: &mut HashMap<String, String>,
    dotted: &mut Vec<(String, String)>,
) {
    match node.kind() {
        "import_statement" => {
            for (name, asname) in import_names(node, None) {
                let full = text(name, src).to_string();
                let local = asname
                    .map(|a| text(a, src).to_string())
                    .unwrap_or_else(|| full.clone());
                alias.insert(first_part(&local), first_part(&full));
                out.imports.push(full);
            }
            return;
        }
        "import_from_statement" | "future_import_statement" => {
            let module_node = node.child_by_field_name("module_name");
            let (dots, module) = match module_node {
                None if node.kind() == "future_import_statement" => {
                    (String::new(), "__future__".to_string())
                }
                None => (String::new(), String::new()),
                Some(m) if m.kind() == "relative_import" => {
                    let parts = named(m);
                    let dots = parts
                        .iter()
                        .find(|c| c.kind() == "import_prefix")
                        .map(|c| text(*c, src).to_string())
                        .unwrap_or_default();
                    let module = parts
                        .iter()
                        .find(|c| c.kind() == "dotted_name")
                        .map(|c| text(*c, src).to_string())
                        .unwrap_or_default();
                    (dots, module)
                }
                Some(m) => (String::new(), text(m, src).to_string()),
            };
            let head = format!("{dots}{module}");
            let sep = if head.is_empty() || head.ends_with('.') {
                ""
            } else {
                "."
            };
            let mut names: Vec<(String, Option<String>)> = import_names(node, module_node)
                .into_iter()
                .map(|(n, a)| {
                    (
                        text(n, src).to_string(),
                        a.map(|a| text(a, src).to_string()),
                    )
                })
                .collect();
            if all(node).iter().any(|c| c.kind() == "wildcard_import") {
                names.push(("*".to_string(), None));
            }
            for (name, asname) in names {
                out.imports.push(format!("{head}{sep}{name}"));
                *out.uses.entry(name.clone()).or_insert(0) += 1;
                let origin = if module.is_empty() {
                    first_part(&name)
                } else {
                    first_part(&module)
                };
                alias.insert(asname.unwrap_or(name), origin);
            }
            return;
        }
        "identifier" => {
            if is_name(node) {
                *out.uses.entry(text(node, src).to_string()).or_insert(0) += 1;
            }
            return;
        }
        "attribute" => {
            if let (Some(object), Some(attr)) = (
                node.child_by_field_name("object"),
                node.child_by_field_name("attribute"),
            ) {
                if object.kind() == "identifier" {
                    dotted.push((text(object, src).to_string(), text(attr, src).to_string()));
                }
            }
        }
        "comment" | "string_content" | "string_start" | "string_end" => return,
        _ => {}
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, src, out, alias, dotted);
    }
}

/// Read one Python file. `None` when it does not parse: the caller then falls back to patterns, as
/// the reference does on a `SyntaxError`.
pub fn read(src: &str) -> Option<PyFile> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .ok()?;
    let tree = parser.parse(src, None)?;
    let root = tree.root_node();
    if root.has_error() {
        return None;
    }
    let mut out = PyFile {
        doc: doc_line(Some(root), src, 110),
        ..Default::default()
    };
    for top in named(root) {
        let node = unwrap_decorated(top);
        let name = node
            .child_by_field_name("name")
            .map(|n| text(n, src).to_string())
            .unwrap_or_default();
        let line = node.start_position().row + 1;
        match node.kind() {
            "function_definition" => {
                let args = node
                    .child_by_field_name("parameters")
                    .map(|p| params_text(p, src))
                    .unwrap_or_default();
                let sig = py::head(&format!("{name}({args})"), 120).to_string();
                let doc = doc_line(node.child_by_field_name("body"), src, 90);
                out.symbols.push(Symbol {
                    name,
                    kind: "def".to_string(),
                    sig,
                    doc,
                    line,
                    methods: None,
                });
            }
            "class_definition" => {
                let body = node.child_by_field_name("body");
                let methods: Vec<String> = body
                    .map(named)
                    .unwrap_or_default()
                    .into_iter()
                    .map(unwrap_decorated)
                    .filter(|m| m.kind() == "function_definition")
                    .filter_map(|m| {
                        m.child_by_field_name("name")
                            .map(|n| text(n, src).to_string())
                    })
                    .filter(|m| !m.starts_with('_'))
                    .take(8)
                    .collect();
                let doc = doc_line(body, src, 90);
                out.symbols.push(Symbol {
                    sig: format!("class {name}"),
                    name,
                    kind: "class".to_string(),
                    doc,
                    line,
                    methods: Some(methods),
                });
            }
            "if_statement"
                if node
                    .child_by_field_name("condition")
                    .is_some_and(|c| text(c, src).contains("__main__")) =>
            {
                out.main = true;
            }
            _ => {}
        }
    }
    let mut alias: HashMap<String, String> = HashMap::new(); // local name -> the module it was imported from
    let mut dotted: Vec<(String, String)> = Vec::new();
    walk(root, src, &mut out, &mut alias, &mut dotted);
    for (base, attr) in dotted {
        // `mod.name` counts when `mod` is an import that is not the standard library
        if alias
            .get(&base)
            .is_some_and(|module| !STDLIB.contains(&module.as_str()))
        {
            *out.uses.entry(attr).or_insert(0) += 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#""""Billing: charge an order.

More text.
"""
from __future__ import annotations

import os
import re as regex
from . import db
from .queue import Queue, post as send
from shop.ledger import *


def bill(order, retries: int = 2, *args, tag: str | None = "x", mode=0o17, **kw) -> int:
    """Charge it.
    Twice if needed."""
    for attempt in range(retries):
        with open(order.path) as fh:
            total = db.save(order, key=fh.read())
    try:
        regex.search("a", "b")
    except ValueError as err:
        raise
    return send(total)


@decorated
async def fetch(a, /, b, *, c: dict[str, list[int]] = {}, d=(1, 2), e=lambda x: x + 1):
    pass


class Cart(Base):
    "Holds items."
    limit = 3

    def add(self, item): ...
    def _hidden(self): ...
    @property
    def size(self): return len(self.items)


if __name__ == "__main__":
    bill(None)
"#;

    #[test]
    fn a_python_file_reads_the_way_ast_reads_it() {
        let f = read(SAMPLE).expect("the sample parses");
        assert_eq!(f.doc, "Billing: charge an order.");
        assert!(f.main);
        let sigs: Vec<&str> = f.symbols.iter().map(|s| s.sig.as_str()).collect();
        assert_eq!(
            sigs,
            [
                "bill(order, retries: int=2, *args, tag: str | None='x', mode=15, **kw)",
                "fetch(a, /, b, *, c: dict[str, list[int]]={}, d=(1, 2), e=lambda x: x + 1)",
                "class Cart"
            ]
        );
        assert_eq!(f.symbols[0].doc, "Charge it.");
        assert_eq!(
            (f.symbols[0].line, f.symbols[1].line, f.symbols[2].line),
            (14, 28, 32)
        );
        assert_eq!(
            f.symbols[2].methods.as_deref(),
            Some(&["add".to_string(), "size".to_string()][..])
        );
        assert_eq!(f.symbols[2].doc, "Holds items.");
        assert_eq!(
            f.imports,
            [
                "__future__.annotations",
                "os",
                "re",
                ".db",
                ".queue.Queue",
                ".queue.post",
                "shop.ledger.*"
            ]
        );
        let uses = |name: &str| f.uses.get(name).copied().unwrap_or(0);
        // imported names count once each; `db.save` counts (db is ours), `regex.search` does not (re is the standard library)
        assert_eq!(
            (
                uses("Queue"),
                uses("post"),
                uses("db"),
                uses("save"),
                uses("search")
            ),
            (1, 1, 2, 1, 0)
        );
        // parameters, attributes, keywords, `except ... as err` and definitions are not uses; `with ... as fh` is
        assert_eq!(
            (
                uses("order"),
                uses("retries"),
                uses("key"),
                uses("err"),
                uses("bill"),
                uses("fh")
            ),
            (2, 1, 0, 0, 1, 2)
        );
        assert_eq!(
            (
                uses("attempt"),
                uses("total"),
                uses("send"),
                uses("Base"),
                uses("decorated"),
                uses("self")
            ),
            (1, 2, 1, 1, 1, 1)
        );
        assert!(read("def broken(:\n").is_none());
    }
}
