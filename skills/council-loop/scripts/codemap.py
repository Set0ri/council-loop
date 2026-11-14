#!/usr/bin/env python3
"""codemap - a cheap, deterministic skeleton of a codebase, for the mapper seat and for people.

ELI5: before anyone draws a map, a script walks every street and lists it: each file, what it
defines, what it leans on, and which files everything else leans on. The mapper (an AI) reads that
list and a few key files, then writes the map. Next time the script says what changed in the
street plan, so the map is redrawn only when the plan changed, not when someone repainted a door.

What it does (no model, no network, standard library only):
  skeleton(root)       files, sizes, public symbols, the import graph, a ranking of key files,
                       commands / tools / routes / environment variables, tests
  focus_map(root, ask) the few files and names that matter most for ONE task, within a token budget
  stale(root, path)    has the STRUCTURE changed since the map was last accepted? (bodies do not count)
  validate_map(text)   does a written map have the sections, and does every path it names exist?
  install_map(text)    put an accepted map in place, without ever overwriting a hand-written one

Built on the best prior work (see reference/prior-art.md):
  - The ranking is the method of aider's repo map (Aider-AI/aider, Apache-2.0): a graph whose edges
    run from the file that USES a name to the file that DEFINES it, weighted by sqrt(uses), x10 for
    names the task mentions and for long well-formed names, x0.1 for names defined in more than five
    files, ranked with personalised PageRank and cut to a token budget. Written again here from
    aider's published design: no aider code is copied.
  - Symbols come from small built-in readers, so the tool has no dependencies. When the optional
    `tree-sitter-language-pack` (MIT) is installed and its parsers are present, it supplies richer
    symbols for 300+ languages. What counts as a change of STRUCTURE always comes from the built-in
    readers, so two machines agree on whether a map is stale.

Add a language: write a function text -> {"doc", "symbols", "imports"} and `register()` it.
"""
from __future__ import annotations

import ast
import hashlib
import json
import math
import os
import re
import subprocess
import sys
from collections import Counter
from pathlib import Path

import council

IGNORE_DIRS = {".git", "node_modules", ".venv", "venv", "env", "dist", "build", "out", "__pycache__", ".pytest_cache",
               ".mypy_cache", ".ruff_cache", ".next", ".nuxt", "target", "vendor", ".idea", ".vscode", "coverage",
               ".tox", ".council", ".gradle", "Pods", "DerivedData"}
CODE = {".py": "Python", ".js": "JavaScript", ".jsx": "JavaScript", ".mjs": "JavaScript", ".cjs": "JavaScript",
        ".ts": "TypeScript", ".tsx": "TypeScript", ".go": "Go", ".rs": "Rust", ".java": "Java", ".kt": "Kotlin",
        ".cs": "C#", ".rb": "Ruby", ".php": "PHP", ".swift": "Swift", ".c": "C", ".h": "C", ".cc": "C++",
        ".cpp": "C++", ".hpp": "C++", ".sh": "Shell", ".sql": "SQL"}
OTHER = {".md": "Markdown", ".yaml": "YAML", ".yml": "YAML", ".json": "JSON", ".toml": "TOML", ".html": "HTML",
         ".css": "CSS"}
MAX_BYTES, MAX_FILES = 512_000, 4000
WALK_CAP = 50_000                                # files looked at when git cannot list them
MARKER = "<!-- council-loop:map"
REQUIRED = ("Purpose and philosophy", "Bird's-eye view", "Code map", "Interfaces", "Invariants", "Extending it")
ADVISED = ("Data and control flow", "Performance and cost", "Where to change what")


# ----------------------------------------------------------------------------- reading the code
def first_line(text: str | None, cap: int = 110) -> str:
    line = next((x.strip() for x in (text or "").splitlines() if x.strip()), "")
    return line if len(line) <= cap else line[: cap - 1] + "…"


def extract_python(text: str) -> dict:
    out = {"doc": "", "symbols": [], "imports": [], "main": False}
    try:
        tree = ast.parse(text)
    except (SyntaxError, ValueError):
        return _by_regex(text, r"^(class|def|async def)\s+([A-Za-z_]\w*)", r"^\s*(?:from\s+([\w.]+)\s+import|import\s+([\w.]+))")
    out["doc"] = first_line(ast.get_docstring(tree))
    for node in tree.body:
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            args = ast.unparse(node.args) if hasattr(ast, "unparse") else ""
            out["symbols"].append({"name": node.name, "kind": "def", "sig": f"{node.name}({args})"[:120],
                                   "doc": first_line(ast.get_docstring(node), 90), "line": node.lineno})
        elif isinstance(node, ast.ClassDef):
            methods = [n.name for n in node.body if isinstance(n, (ast.FunctionDef, ast.AsyncFunctionDef))
                       and not n.name.startswith("_")]
            out["symbols"].append({"name": node.name, "kind": "class", "sig": f"class {node.name}",
                                   "doc": first_line(ast.get_docstring(node), 90), "line": node.lineno,
                                   "methods": methods[:8]})
        elif isinstance(node, ast.If) and "__main__" in (ast.unparse(node.test) if hasattr(ast, "unparse") else ""):
            out["main"] = True
    uses: Counter = Counter()
    alias: dict[str, str] = {}                                   # local name -> the module it was imported from
    dotted: list[tuple[str, str]] = []
    for node in ast.walk(tree):                                  # imports anywhere, lazy ones too
        if isinstance(node, ast.Import):
            out["imports"] += [a.name for a in node.names]
            alias.update({(a.asname or a.name).split(".")[0]: a.name.split(".")[0] for a in node.names})
        elif isinstance(node, ast.ImportFrom):                   # `from pkg import mod` may name a module: keep both parts
            head = "." * node.level + (node.module or "")
            out["imports"] += [head + ("" if head.endswith(".") or not head else ".") + a.name for a in node.names]
            uses.update(a.name for a in node.names)
            alias.update({a.asname or a.name: (node.module or a.name).split(".")[0] for a in node.names})
        elif isinstance(node, ast.Name):                         # names the code uses: never a comment or a string
            uses[node.id] += 1
        elif isinstance(node, ast.Attribute) and isinstance(node.value, ast.Name):
            dotted.append((node.value.id, node.attr))
    stdlib = getattr(sys, "stdlib_module_names", ())
    for base, attr in dotted:                                    # `mod.name` counts when `mod` is an import that is not the
        if base in alias and alias[base] not in stdlib:          # standard library: `re.search` is not a use of our search()
            uses[attr] += 1
    out["uses"] = uses
    return out


def _by_regex(text: str, symbols: str, imports: str | None = None, kind_group: int = 1, name_group: int = 2) -> dict:
    out = {"doc": "", "symbols": [], "imports": [], "main": False}
    for m in re.finditer(symbols, text, re.M):
        out["symbols"].append({"name": m.group(name_group), "kind": (m.group(kind_group) or "def").split()[-1],
                               "sig": " ".join(m.group(0).split())[:120], "doc": "",
                               "line": text.count("\n", 0, m.start()) + 1})
    if imports:
        for m in re.finditer(imports, text, re.M):
            out["imports"].append(next(g for g in m.groups() if g))
    return out


def extract_js(text: str) -> dict:
    out = _by_regex(text, r"^\s*export\s+(?:default\s+)?(?:async\s+)?(function\*?|class|const|let|var|interface|type|enum)\s+([A-Za-z_$][\w$]*)",
                    r"""(?:\bfrom\s*|\bimport\s*\(?\s*|\brequire\(\s*)['"]([^'"\n]+)['"]""")
    head = re.match(r"\s*/\*\*?(.*?)\*/", text, re.S)
    out["doc"] = first_line(re.sub(r"^\s*\*\s?", "", head.group(1), flags=re.M)) if head else ""
    return out


def extract_go(text: str) -> dict:
    out = _by_regex(text, r"^(func)\s+(?:\([^)]*\)\s*)?([A-Z]\w*)\s*\(")
    out["symbols"] += _by_regex(text, r"^(type)\s+([A-Z]\w*)\s+(?:struct|interface)")["symbols"]
    out["imports"] = re.findall(r'^\s*(?:[\w.]+\s+)?"([\w./-]+)"\s*$', text, re.M)
    out["main"] = bool(re.search(r"^func\s+main\s*\(", text, re.M))
    return out


def extract_rust(text: str) -> dict:
    out = _by_regex(text, r"^\s*pub(?:\([^)]*\))?\s+(?:async\s+)?(fn|struct|enum|trait|mod|type|const)\s+(\w+)",
                    r"^\s*(?:pub\s+)?(?:mod|use)\s+([\w:]+)")
    out["main"] = bool(re.search(r"^fn\s+main\s*\(", text, re.M))
    return out


def extract_classes(text: str) -> dict:
    return _by_regex(text, r"^\s*(?:(?:public|internal|abstract|final|sealed|data|open|static)\s+)*(class|interface|enum|record|object|struct)\s+(\w+)")


def extract_ruby(text: str) -> dict:
    return _by_regex(text, r"^\s*(class|module|def)\s+([\w.:?!]+)", r"^\s*require(?:_relative)?\s+['\"]([^'\"]+)['\"]")


EXTRACTORS: dict = {}


def register(suffixes, fn) -> None:
    """Teach the skeleton a language: fn(text) -> {"doc", "symbols": [{name, kind, sig, doc, line}], "imports", "main"}."""
    for s in suffixes:
        EXTRACTORS[s] = fn


def extract_generic(text: str) -> dict:
    """Languages without a reader of their own (C, C++, shell, SQL): un-indented lines that look like
    a definition. Rough, but it is the same on every machine, which is what staleness needs."""
    out = _by_regex(text, r"^(?!(?:if|for|while|switch|return|else|do|case)\b)(?:[A-Za-z_][\w:<>,*&\s]*?\s+\**)?()([A-Za-z_]\w{2,})\s*\([^;{}]*\)\s*(?:const\s*)?\{?\s*$")
    out["symbols"] += _by_regex(text, r"^(?:typedef\s+)?(struct|class|enum|union|namespace)\s+([A-Za-z_]\w*)")["symbols"]
    out["symbols"] += _by_regex(text, r"(?i)^\s*create\s+(?:or\s+replace\s+)?(table|view|function|procedure|index)\s+(?:if\s+not\s+exists\s+)?[`\"\[]?([\w.]+)")["symbols"]
    out["imports"] = re.findall(r'^\s*#\s*include\s+"([^"]+)"', text, re.M)
    out["main"] = bool(re.search(r"^\s*(?:int|void)\s+main\s*\(", text, re.M))
    return out


register([".py"], extract_python)
register([".js", ".jsx", ".mjs", ".cjs", ".ts", ".tsx"], extract_js)
register([".go"], extract_go)
register([".rs"], extract_rust)
register([".java", ".kt", ".cs", ".swift", ".php"], extract_classes)
register([".rb"], extract_ruby)
register([".c", ".h", ".cc", ".cpp", ".hpp", ".sh", ".sql"], extract_generic)


# ----------------------------------------------------------------------------- optional: tree-sitter
_TS: tuple | None = None
_PRIVATE = re.compile(r"^\s*(?:private|protected|fileprivate|internal)\b|^\s*#")


def tree_sitter() -> tuple:
    """(the tree-sitter language pack or None, the languages whose parsers are on this machine).
    Nothing is downloaded from here: `council map --parsers` does that, when the user asks."""
    global _TS
    if _TS is None:
        try:
            if os.environ.get("COUNCIL_PARSERS") == "built-in":    # the same symbols on every machine (tests, parity)
                raise ImportError("switched off")
            import tree_sitter_language_pack as pack
            _TS = (pack, set(pack.downloaded_languages()))
        except Exception:  # noqa: BLE001 - optional: absent, broken or too old all mean "not there"
            _TS = (None, set())
    return _TS


def _public(lang: str, name: str, sig: str, exported: set) -> bool:
    if not name or name.startswith("_"):
        return False
    if lang == "go":
        return name[:1].isupper()
    if lang == "rust":
        return sig.lstrip().startswith("pub")
    if lang in ("typescript", "tsx", "javascript") and exported:
        return name in exported
    return not _PRIVATE.match(sig)


def rich_symbols(name: str, text: str) -> list[dict] | None:
    """Public symbols with full signatures from tree-sitter, or None when it cannot help here.
    Python keeps its own reader (the standard library's parser already gives signatures and docstrings)."""
    pack, have = tree_sitter()
    if not pack or name.endswith(".py"):
        return None
    try:
        lang = pack.detect_language_from_path(name)
        if not lang or lang not in have:
            return None
        res = pack.process(text, pack.ProcessConfig(language=lang, structure=True, imports=False, exports=True, symbols=True))
    except Exception:  # noqa: BLE001 - one odd file must not stop the map
        return None
    exported = {e.name for e in res.exports if getattr(e, "name", None)}
    clean = lambda doc: first_line(re.sub(r"(?m)^\s*(?:///?|//!|/\*\*?|\*/?|#)\s?", "", doc or ""), 90)   # noqa: E731
    out, by_name = [], {}
    for it in res.structure:
        kind, sig = str(it.kind).lower(), " ".join((it.signature or it.name or "").split())[:140]
        kids = [c.name for c in it.children if _public(lang, c.name, c.signature or "", set())]
        if kind == "impl" and it.name in by_name:              # Rust: the methods belong to the type
            by_name[it.name]["methods"] = (by_name[it.name].get("methods") or []) + kids[:8]
            continue
        if kind in ("module", "namespace", "package") and not kids:
            continue                                           # `package x;` is not a symbol
        if kind != "impl" and not _public(lang, it.name, sig, exported):
            continue
        sym = {"name": it.name, "kind": kind, "sig": sig, "doc": clean(it.doc_comment), "line": it.span.start_line + 1}
        if kids:
            sym["methods"] = kids[:8]
        out.append(sym)
        by_name.setdefault(it.name, sym)
    if lang != "rust":                                         # types and constants the structure list leaves out
        for sy in res.symbols:
            kind = str(sy.kind).lower()
            if sy.name in by_name or kind not in ("type", "class", "interface", "struct", "enum", "constant", "trait"):
                continue
            if _public(lang, sy.name, "", exported):
                out.append({"name": sy.name, "kind": kind, "sig": f"{kind} {sy.name}", "doc": clean(getattr(sy, "doc", "")),
                            "line": sy.span.start_line + 1})
                by_name[sy.name] = out[-1]
    return out or None


def fetch_parsers(root: Path) -> dict:
    """Download the tree-sitter parsers for the languages this repository uses. The one network call
    in this file, and only on request."""
    global _TS
    pack, have = tree_sitter()
    if not pack:
        return {"ok": False, "note": "the optional package is not installed: pip install tree-sitter-language-pack"}
    names, _ = list_files(root)
    want = set()
    for n in names:
        if Path(n).suffix.lower() in CODE and not n.endswith(".py"):
            try:
                lang = pack.detect_language_from_path(n)
            except Exception:  # noqa: BLE001
                lang = None
            if lang:
                want.add(lang)
    todo = sorted(want - have)
    if todo:
        pack.download(todo)
    _TS = None
    return {"ok": True, "wanted": sorted(want), "fetched": todo, "have": sorted(tree_sitter()[1] & want)}

ENV_RX = re.compile(r"""(?:os\.environ(?:\.get)?\(\s*|os\.environ\[\s*|os\.getenv\(\s*|\benv(?:iron)?\.get\(\s*|\benv\[\s*|process\.env\[\s*|env::var\(\s*|os\.Getenv\(\s*|ENV\[\s*)['"]([A-Z][A-Z0-9_]{2,})['"]|process\.env\.([A-Z][A-Z0-9_]{2,})""")
CLI_RX = re.compile(r"""add_parser\(\s*['"]([\w-]+)['"]|@(?:\w+\.)?command\(\s*(?:name\s*=\s*)?['"]([\w-]+)['"]""")
ROUTE_RX = re.compile(r"""@?\b(?:app|router|bp|api|blueprint)\.(get|post|put|patch|delete|route)\(\s*['"`]([^'"`\n]+)['"`]""")
TOOL_RX = re.compile(r"@mcp\.(tool|resource|prompt)\(([^)\n]*)\)\s*\n\s*(?:async\s+)?def\s+(\w+)")
IDENT_RX = re.compile(r"[A-Za-z_][A-Za-z0-9_]{2,}")
NOISE_RX = re.compile(r'/\*.*?\*/|//[^\n]*|"(?:\\.|[^"\\\n])*"|`(?:\\.|[^`\\])*`', re.S)      # comments and strings
QUOTE_RX = re.compile(r"'(?:\\.|[^'\\\n])*'")
HASH_RX = re.compile(r"(?m)(?:^|\s)#[^\n]*")
SINGLE_QUOTED = {".js", ".jsx", ".mjs", ".cjs", ".ts", ".tsx", ".rb", ".php", ".sh", ".sql", ".py", ".swift"}
HASH_COMMENTS = {".rb", ".sh", ".py", ".php"}


def code_words(text: str, suffix: str) -> Counter:
    """The names a file uses, with comments and string contents removed first (roughly: this is a
    pattern, not a parser). A word in a comment is not a use of the function that shares its name."""
    body = NOISE_RX.sub(" ", text)
    if suffix in SINGLE_QUOTED:
        body = QUOTE_RX.sub(" ", body)
    if suffix in HASH_COMMENTS:
        body = HASH_RX.sub(" ", body)
    return Counter(IDENT_RX.findall(body))
TEST_RX = re.compile(r"^\s*(?:async\s+)?def\s+test_\w+|^\s*(?:it|test)\(\s*['\"`]|^func\s+Test\w+|#\[test\]", re.M)


def list_files(root: Path) -> tuple[list[str], bool]:
    """Repo files worth reading, as posix paths. Uses git when it can, so ignore rules are honoured."""
    names = None
    if (root / ".git").exists():
        try:
            res = subprocess.run(["git", "-C", str(root), "ls-files", "-co", "--exclude-standard", "-z"],
                                 capture_output=True, encoding="utf-8", errors="replace", timeout=30)
            if res.returncode == 0:                         # -z: names as they are, never quoted or escaped
                names = [n for n in res.stdout.split("\0") if n]
        except (OSError, subprocess.SubprocessError):
            names = None
    if names is None:
        names = []
        stack = [root]
        while stack and len(names) < WALK_CAP:                 # not a repository, or far too big: stop looking
            try:
                entries = sorted(stack.pop().iterdir())
            except OSError:
                continue
            for p in entries:
                if p.is_dir():
                    if p.name not in IGNORE_DIRS and not p.is_symlink():
                        stack.append(p)
                elif p.is_file():
                    names.append(p.relative_to(root).as_posix())
    keep = []
    for n in sorted(set(names)):
        p = root / n
        if set(Path(n).parts[:-1]) & IGNORE_DIRS or p.suffix.lower() not in {**CODE, **OTHER}:
            continue
        try:
            if p.is_file() and p.stat().st_size <= MAX_BYTES:
                keep.append(n)
        except OSError:
            continue
    return keep[:MAX_FILES], len(keep) > MAX_FILES


def scan(root: Path) -> dict:
    """Read every file once. Returns {"files": {path: info}, "capped": bool}."""
    names, capped = list_files(root)
    files = {}
    for n in names:
        p = root / n
        try:                                                   # line ends and byte-order marks must not change what is read:
            raw = p.read_bytes().replace(b"\r\n", b"\n")       # a CRLF checkout and an LF one are the same code
        except OSError:
            continue
        text = raw.decode("utf-8-sig", errors="replace")
        suf = p.suffix.lower()
        info = {"lang": CODE.get(suf) or OTHER[suf], "code": suf in CODE, "lines": text.count("\n") + (1 if text and not text.endswith("\n") else 0),
                "sha": hashlib.sha1(raw).hexdigest(), "doc": "", "symbols": [], "imports": [], "main": False,
                "env": [], "cli": [], "routes": [], "tools": [], "tests": 0}
        if suf in EXTRACTORS:
            try:
                info.update(EXTRACTORS[suf](text))
            except Exception:  # noqa: BLE001 - one odd file must not stop the map
                pass
        if info["code"]:
            info["uses"] = info.pop("uses", None) or code_words(text, suf)
            rich = rich_symbols(n, text)
            if rich:
                info["rich"] = rich                             # shown and ranked; never part of the fingerprint
            info["env"] = sorted({a or b for a, b in ENV_RX.findall(text)})
            info["cli"] = [a or b for a, b in CLI_RX.findall(text)]
            info["routes"] = [f"{m.upper()} {path}" for m, path in ROUTE_RX.findall(text)]
            info["tools"] = [f"{kind} {name}" for kind, _, name in TOOL_RX.findall(text)]
            info["tests"] = len(TEST_RX.findall(text))
        files[n] = info
    defined = {name for i in files.values() for name in def_names(i)}
    for i in files.values():                                   # keep only uses of names this repository defines
        i["uses"] = {k: v for k, v in (i.get("uses") or {}).items() if k in defined}
    return {"files": files, "capped": capped, "parsers": "tree-sitter" if any("rich" in i for i in files.values()) else "built-in"}


def shown(info: dict) -> list[dict]:
    """The public symbols to show for a file: tree-sitter's when it read the file, else the built-in reader's."""
    return info.get("rich") or [s for s in info["symbols"] if not s["name"].startswith("_")]


def def_names(info: dict) -> set[str]:
    names = set()
    for sym in shown(info):
        names.add(sym["name"])
        names.update(sym.get("methods") or [])
    return {n for n in names if len(n) >= 3 and IDENT_RX.fullmatch(n)}


def resolve_imports(files: dict) -> dict[str, list[str]]:
    """file -> the files in this repo it imports. Python by module name, JS/TS by relative path."""
    by_stem: dict[str, list[str]] = {}
    for n in files:
        if n.endswith(".py"):
            by_stem.setdefault(Path(n).stem if Path(n).stem != "__init__" else Path(n).parent.name, []).append(n)
    edges: dict[str, list[str]] = {}
    for n, info in files.items():
        here, found = Path(n).parent, set()
        for imp in info["imports"]:
            target = None
            if n.endswith(".py"):
                dots = len(imp) - len(imp.lstrip("."))
                parts = [x for x in imp.lstrip(".").split(".") if x]
                base = here
                for _ in range(max(0, dots - 1)):
                    base = base.parent
                for k in range(len(parts), 0, -1):
                    as_path = "/".join(parts[:k])
                    for cand in ((base / as_path).as_posix() + ".py", (base / as_path / "__init__.py").as_posix(),
                                 as_path + ".py", as_path + "/__init__.py"):
                        if cand in files:
                            target = cand
                            break
                    if target:
                        break
                if not target and parts and not dots:
                    same = [c for c in by_stem.get(parts[0], [])]
                    near = [c for c in same if Path(c).parent == here]
                    target = near[0] if near else (same[0] if len(same) == 1 else None)
            elif imp.startswith("."):
                stem = (here / imp).as_posix()
                stem = re.sub(r"(^|/)\./", r"\1", stem)
                parts = []
                for seg in stem.split("/"):
                    if seg == "..":
                        if parts:
                            parts.pop()
                    elif seg and seg != ".":
                        parts.append(seg)
                stem = "/".join(parts)
                for cand in (stem, *(stem + e for e in (".ts", ".tsx", ".js", ".jsx", ".mjs")),
                             *(stem + "/index" + e for e in (".ts", ".tsx", ".js", ".jsx"))):
                    if cand in files:
                        target = cand
                        break
            if target and target != n:
                found.add(target)
        if found:
            edges[n] = sorted(found)
    return edges


IMPORT_WEIGHT = 3.0                              # a resolved import is certain; a matching name is likely


def well_named(ident: str) -> bool:
    """A long snake_case, kebab-case or camelCase name is rarely a coincidence (aider's test)."""
    snake = "_" in ident and any(c.isalpha() for c in ident)
    camel = any(c.isupper() for c in ident) and any(c.islower() for c in ident)
    return (snake or camel or "-" in ident) and len(ident) >= 8


def ranking(files: dict, edges: dict, focus=(), mentioned=()) -> dict:
    """Rank files and the names they define by how much other code leans on them.

    The method is aider's repo map: an edge runs from the file that uses a name to the file that
    defines it, with weight sqrt(uses) x10 if the task mentions the name, x10 if the name is long and
    well formed, x0.1 if more than five files define it, x50 if the using file is in focus. Resolved
    imports are added as edges too. PageRank then runs with its restart on the focus files, and each
    file's score is shared out to the names it points at. A name nobody else uses gets a small share
    (0.1) of its own file's score; unlike aider's self-edge it holds no rank back, so a file that
    nothing uses cannot rise by being isolated. Returns {"score", "names", "used_by"}."""
    code = [n for n, i in files.items() if i["code"]]
    if not code:
        return {"score": {}, "names": {}, "used_by": {}}
    focus, mentioned = {f for f in focus if f in files}, set(mentioned)
    defines: dict[str, set] = {}
    users: dict[str, list] = {}
    for n in code:
        for name in sorted(def_names(files[n])):               # sorted: the same sums in the same order on every machine
            defines.setdefault(name, set()).add(n)
        for name, count in files[n].get("uses", {}).items():
            users.setdefault(name, []).append((n, count))
    out: dict[str, dict] = {n: {} for n in code}                # user -> definer -> weight
    via: dict[tuple, dict] = {}                                 # (user, definer) -> name -> weight
    own: dict[str, dict] = {n: {} for n in code}                # file -> its names that nobody else uses
    used_by: dict[tuple, int] = {}

    def add(src, dst, name, weight):
        out[src][dst] = out[src].get(dst, 0.0) + weight
        if name:
            edge = via.setdefault((src, dst), {})
            edge[name] = edge.get(name, 0.0) + weight

    for name, definers in defines.items():
        mul = (10 if name in mentioned else 1) * (10 if well_named(name) else 1) * (0.1 if len(definers) > 5 else 1)
        refs = [(u, c) for u, c in users.get(name, []) if u not in definers]
        definers = sorted(definers)
        if not refs:
            for d in definers:
                own[d][name] = 0.1
            continue
        for d in definers:
            used_by[(d, name)] = len(refs)
        for user, count in refs:
            w = mul * (50 if user in focus else 1) * math.sqrt(count)
            for d in definers:
                add(user, d, name, w)
    for src, targets in edges.items():
        for t in targets:
            if src in out and t in out:
                add(src, t, None, IMPORT_WEIGHT)
    lowered = {m.lower() for m in mentioned}
    home = {d for name in mentioned for d in defines.get(name, ())}          # files that define a name the task mentions
    pers = {n: (1.0 if n in focus else 0.0) + (1.0 if n in home else 0.0) +
            (1.0 if ({p.lower() for p in Path(n).with_suffix("").parts} & lowered) else 0.0) for n in code}
    total = sum(pers.values())
    base = {n: (pers[n] / total if total else 1 / len(code)) for n in code}
    score = dict(base)
    for _ in range(40):
        nxt = {n: 0.15 * base[n] for n in code}
        dangling = 0.0
        for n in code:
            tot = sum(out[n].values())
            if tot:
                for t, w in out[n].items():
                    nxt[t] += 0.85 * score[n] * w / tot
            else:
                dangling += score[n]
        if dangling:
            for n in code:
                nxt[n] += 0.85 * dangling * base[n]
        score = nxt
    names: dict[tuple, float] = {}
    share = {n: sum(out[n].values()) + sum(own[n].values()) for n in code}
    for (src, dst), idents in via.items():
        for name, w in idents.items():
            names[(dst, name)] = names.get((dst, name), 0.0) + score[src] * w / share[src]
    for n, idents in own.items():
        for name, w in idents.items():
            names[(n, name)] = names.get((n, name), 0.0) + score[n] * w / share[n]
    # Rounded: the order of a ranking must not hang on the last bit of a sum (it differs between
    # Python versions, and between this file and the Rust implementation of the same method).
    return {"score": {n: round(v, 9) for n, v in score.items()}, "names": {k: round(v, 9) for k, v in names.items()},
            "used_by": used_by}


def rank(files: dict, edges: dict) -> dict[str, float]:
    """File scores only (kept for callers that need nothing else)."""
    return ranking(files, edges)["score"]


TEST_FILE_RX = re.compile(r"(^|/)(tests?|__tests__|specs?|testing)/|(^|/)(test_[^/]*|conftest)\.py$"
                          r"|_test\.[a-z0-9]+$|\.(test|spec)\.[a-z0-9]+$")
TEST_NAME_RX = re.compile(r"test_|Test(?=[A-Z_]|$)|test(?=[A-Z])")


def fingerprint(info: dict, name: str = "") -> str:
    """What makes a file part of the STRUCTURE: its public names, signatures, imports and interfaces.
    A change inside a function body leaves this alone, so it does not make the map stale. Nor does a
    new test in a test file: the names of the tests are left out (the file, its imports and its
    helpers still count), so that writing tests never costs a run of the mapper."""
    tests = bool(TEST_FILE_RX.search(name))
    public = sorted(f"{s['kind']} {s.get('sig') or s['name']} {','.join(s.get('methods') or [])}"
                    for s in info["symbols"]
                    if not s["name"].startswith("_") and not (tests and TEST_NAME_RX.match(s["name"])))
    blob = json.dumps([public, sorted(set(info["imports"])), info["env"], info["cli"], info["routes"], info["tools"], info["main"]])
    return hashlib.sha1(blob.encode()).hexdigest()[:16]


# ----------------------------------------------------------------------------- state and staleness
def map_path(root: Path) -> Path:
    cfg = council.config(root)
    members = cfg.get("members") if isinstance(cfg.get("members"), dict) else {}
    conf = (members.get("map") if isinstance(members.get("map"), dict) else {}).get("path") or (cfg.get("map") or {}).get("path")
    if conf and str(conf).endswith(".md") and not Path(conf).is_absolute() and ".." not in Path(conf).parts:
        return root / conf
    return root / "docs" / "ARCHITECTURE.md" if (root / "docs").is_dir() else root / "ARCHITECTURE.md"


def state_file(root: Path) -> Path:
    return root / ".council" / "map" / "state.json"


def structure(scanned: dict) -> dict[str, str]:
    return {n: fingerprint(i, n) for n, i in scanned["files"].items() if i["code"]}


def changes(before: dict, now: dict) -> dict:
    return {"added": sorted(set(now) - set(before)), "removed": sorted(set(before) - set(now)),
            "changed": sorted(n for n in now if n in before and now[n] != before[n])}


def which(diff: dict, most: int = 3) -> str:
    """The files behind a stale map, by name: 'a.py, b.py and 4 more'."""
    names = [n for kind in ("changed", "added", "removed") for n in diff.get(kind) or []]
    return ", ".join(names[:most]) + (f" and {len(names) - most} more" if len(names) > most else "")


def stale(root: Path, scanned: dict | None = None) -> dict:
    """Should the mapper run? Not when there is no code, and not when the structure is as it was mapped."""
    scanned = scanned or scan(root)
    now = structure(scanned)
    path = map_path(root)
    if not now:
        return {"applies": False, "stale": False, "reason": "no source code found in this project", "path": str(path)}
    try:
        state = json.loads(state_file(root).read_text(encoding="utf-8"))
    except (OSError, ValueError):
        state = {}
    if not path.exists():
        return {"applies": True, "stale": True, "reason": f"no map yet at {path.relative_to(root).as_posix()}", "path": str(path),
                "diff": changes({}, now)}
    if not state.get("structure"):
        return {"applies": True, "stale": True, "reason": "the map was never checked against the code", "path": str(path),
                "diff": changes({}, now)}
    diff = changes(state["structure"], now)
    n = sum(len(v) for v in diff.values())
    return {"applies": True, "stale": n > 0, "path": str(path), "diff": diff, "mapped": state.get("mapped"),
            "reason": (f"{n} file(s) changed in structure since the map of {state.get('mapped')}: {which(diff)}" if n else
                       f"the structure is as mapped on {state.get('mapped')}")}


def accept(root: Path, scanned: dict | None = None, run: str | None = None) -> None:
    """Record that the map describes the code as it is now."""
    scanned = scanned or scan(root)
    f = state_file(root)
    f.parent.mkdir(parents=True, exist_ok=True)
    f.write_text(json.dumps({"mapped": council.today().isoformat(), "run": run, "map": map_path(root).relative_to(root).as_posix(),
                             "structure": structure(scanned)}, indent=1), encoding="utf-8", newline="\n")


# ----------------------------------------------------------------------------- the skeleton
def skeleton(root: Path, budget: int = 420, scanned: dict | None = None) -> dict:
    """The skeleton as Markdown, plus the numbers behind it."""
    scanned = scanned or scan(root)
    files = scanned["files"]
    code = {n: i for n, i in files.items() if i["code"]}
    edges = resolve_imports(files)
    ranked_ = ranking(files, edges)
    score, name_rank, used_by = ranked_["score"], ranked_["names"], ranked_["used_by"]
    imported_by: dict[str, int] = {}
    for src, outs in edges.items():
        for t in outs:
            imported_by[t] = imported_by.get(t, 0) + 1
    langs: dict[str, list[int]] = {}
    for i in files.values():
        langs.setdefault(i["lang"], [0, 0])
        langs[i["lang"]][0] += 1
        langs[i["lang"]][1] += i["lines"]
    dirs: dict[str, list] = {}
    for n, i in files.items():
        d = "/".join(Path(n).parts[:-1][:3]) or "."
        dirs.setdefault(d, [0, 0, set()])
        dirs[d][0] += 1
        dirs[d][1] += i["lines"]
        dirs[d][2].add(i["lang"])
    st = stale(root, scanned)
    md = [f"# Codebase skeleton: {root.name}", "",
          f"Made by `council map` on {council.today().isoformat()}. A script wrote this. It lists what is there, "
          "not why: trust it for names and counts, and read the code for intent.", "",
          f"{len(files)} files, {sum(i['lines'] for i in files.values()):,} lines; {len(code)} are source code."
          + (" The file list was capped: this is a large repository." if scanned["capped"] else ""), "",
          "## Size", "", "| Language | Files | Lines |", "| --- | ---: | ---: |"]
    md += [f"| {lang} | {v[0]} | {v[1]:,} |" for lang, v in sorted(langs.items(), key=lambda x: -x[1][1])]
    md += ["", "## Directories", ""]
    md += [f"- `{d}/` {v[0]} files, {v[1]:,} lines ({', '.join(sorted(v[2]))})" for d, v in sorted(dirs.items())][:40]
    cli = sorted({(c, n) for n, i in code.items() for c in i["cli"]})
    tools = [(t, n) for n, i in sorted(code.items()) for t in i["tools"]]
    routes = [(r, n) for n, i in sorted(code.items()) for r in i["routes"]]
    env = sorted({e for i in code.values() for e in i["env"]})
    mains = sorted(n for n, i in code.items() if i["main"])
    md += ["", "## Interfaces found", "", "Found by pattern matching, so the lists can be incomplete: confirm in the code.", ""]
    by_file: dict[str, list[str]] = {}
    for c, n in cli:
        by_file.setdefault(n, []).append(c)
    md += [f"- Commands in `{n}`: {', '.join(cs)}" for n, cs in by_file.items()]
    by_file = {}
    for t, n in tools:
        by_file.setdefault(n, []).append(t)
    md += [f"- MCP surface in `{n}`: {', '.join(ts)}" for n, ts in by_file.items()]
    md += [f"- HTTP route `{r}` in `{n}`" for r, n in routes[:30]]
    if env:
        md.append("- Environment variables read: " + ", ".join(f"`{e}`" for e in env))
    if mains:
        md.append("- Runnable files (entry points): " + ", ".join(f"`{n}`" for n in mains))
    if not (cli or tools or routes or env or mains):
        md.append("- none detected by the script")
    md += ["", "## Key files, ranked by how much other code leans on them", "",
           "Ranked the way aider's repo map does it: a file scores when other files use the names it defines "
           f"or import it. Within a file, the names other files use most come first. Symbols read by: {scanned.get('parsers', 'built-in')} readers.", ""]
    ranked = sorted(code, key=lambda n: (-score.get(n, 0), n))
    for k, n in enumerate(ranked[:25], 1):
        i = code[n]
        md.append(f"### {k}. `{n}` · {i['lang']} · {i['lines']:,} lines · imported by {imported_by.get(n, 0)}")
        if i["doc"]:
            md.append(f"\"{i['doc']}\"")
        public = sorted(shown(i), key=lambda s: -name_rank.get((n, s["name"]), 0))     # stable: file order breaks ties
        for s in public[:14]:
            extra = f" (methods: {', '.join(s['methods'])})" if s.get("methods") else ""
            uses = used_by.get((n, s["name"]))
            extra += f" · used in {uses} other file{'s' if uses != 1 else ''}" if uses else ""
            md.append(f"- `{s.get('sig') or s['name']}`{extra}" + (f": {s['doc']}" if s.get("doc") else ""))
        if len(public) > 14:
            md.append(f"- … and {len(public) - 14} more public names")
        md.append("")
    if len(ranked) > 25:
        md += [f"Other source files ({len(ranked) - 25}): " + ", ".join(f"`{n}`" for n in ranked[25:85]) +
               (" …" if len(ranked) > 85 else ""), ""]
    md += ["## Who imports whom (inside this repository)", ""]
    md += [f"- `{src}` → " + ", ".join(f"`{t}`" for t in outs) for src, outs in sorted(edges.items())][:60] or ["- no internal imports found"]
    ext: dict[str, int] = {}
    for n, i in code.items():
        for imp in {x.lstrip(".").split(".")[0].split("/")[0] for x in i["imports"] if x and not x.startswith(".")}:
            if n.endswith(".py") and imp in getattr(sys, "stdlib_module_names", ()):
                continue                                          # the standard library is not a dependency
            if imp and not any(Path(f).stem == imp or Path(f).parts[0] == imp for f in files):
                ext[imp] = ext.get(imp, 0) + 1
    md += ["", "## Outside dependencies (imported names, with the number of files that use them)", "",
           ", ".join(f"{k} ({v})" for k, v in sorted(ext.items(), key=lambda x: (-x[1], x[0]))[:40]) or "none found"]
    tests = {n: i["tests"] for n, i in code.items() if i["tests"]}
    md += ["", "## Tests", "", f"{sum(tests.values())} test functions in {len(tests)} files: " +
           (", ".join(f"`{n}` ({c})" for n, c in sorted(tests.items())[:20]) or "none found")]
    md += ["", "## Changed in structure since the last accepted map", ""]
    if not st.get("applies"):
        md.append("- not applicable: " + st["reason"])
    elif st.get("mapped") is None:
        md.append("- " + st["reason"] + ": map all of it")
    else:
        d = st["diff"]
        md += [f"- {label}: " + ", ".join(f"`{n}`" for n in d[key][:40]) for key, label in
               (("added", "new files"), ("removed", "files that are gone"), ("changed", "files whose public names, imports or interfaces changed"))
               if d[key]] or ["- nothing: the structure is as mapped"]
    if len(md) > budget:
        md = md[:budget] + ["", f"(cut at {budget} lines to save context; run `council map --budget N` for more)"]
    return {"markdown": "\n".join(md) + "\n", "stale": st, "scanned": scanned,
            "stats": {"files": len(files), "code_files": len(code), "lines": sum(i["lines"] for i in files.values()),
                      "edges": sum(len(v) for v in edges.values()), "commands": len(cli), "tools": len(tools)}}


# ----------------------------------------------------------------------------- the focus map
def focus_map(root: Path, about: str = "", paths=(), budget_tokens: int = 700, scanned: dict | None = None) -> dict:
    """The code that matters most for ONE task, as a short list: aider's repo map, applied to an ask.
    Names and files the ask mentions pull the ranking toward them; the list is cut to a token budget
    (about four characters to a token). Returns {"markdown", "files", "tokens", "mentioned"}."""
    scanned = scanned or scan(root)
    files = scanned["files"]
    code = {n: i for n, i in files.items() if i["code"]}
    if not code:
        return {"markdown": "", "files": [], "tokens": 0, "mentioned": []}
    words = set(IDENT_RX.findall(about))
    defined = {name for i in code.values() for name in def_names(i)}
    stems = {p.lower() for n in code for p in Path(n).with_suffix("").parts}
    mentioned = (words & defined) | {w for w in words if w.lower() in stems and len(w) >= 4}
    named = {t.strip("`'\".,;:()") for t in about.split()} | set(paths or ())
    focus = {n for n in code if n in named or any(n.endswith("/" + t) for t in named if "/" in t or "." in t)}
    res = ranking(files, resolve_imports(files), focus, mentioned)
    score, name_rank, used_by = res["score"], res["names"], res["used_by"]
    sym = {(n, s["name"]): s for n, i in code.items() for s in shown(i)}
    picked: dict[str, list] = {n: [] for n in focus}            # a file the ask names is always listed
    size = sum(len(n) + 12 for n in picked)
    by_rank = sorted(name_rank.items(), key=lambda kv: (-kv[1], kv[0]))
    first = [k for k, _ in by_rank if k[1] in mentioned][:12]   # what the ask names comes first, whatever its rank
    for n, name in first + [k for k, _ in by_rank if k not in first]:
        s = sym.get((n, name))
        if not s or s in picked.get(n, ()):
            continue                                            # a method: its class line already names it
        cost = len(s.get("sig") or name) + len(s.get("doc") or "") + 28 + (0 if n in picked else len(n) + 12)
        if (size + cost) / 4 > budget_tokens:
            break
        picked.setdefault(n, []).append(s)
        size += cost
    lead = {n for n, _ in first}
    order = sorted(picked, key=lambda n: (n not in focus, n not in lead, -score.get(n, 0), n))
    md = []
    for n in order:
        md.append(f"- `{n}`" + (" (named in the ask)" if n in focus else ""))
        for s in picked[n]:
            uses = used_by.get((n, s["name"]))
            md.append(f"  - `{s.get('sig') or s['name']}`" + (f" · used in {uses} other file{'s' if uses != 1 else ''}" if uses else "")
                      + (f": {s['doc']}" if s.get("doc") else ""))
    text = "\n".join(md)
    return {"markdown": text, "files": order, "tokens": len(text) // 4, "mentioned": sorted(mentioned)}


# ----------------------------------------------------------------------------- a written map
PATH_RX = re.compile(r"`([^`\s]+)`")


def named_paths(text: str) -> list[str]:
    """Backticked tokens in a map that claim to be files or folders of this repository. Checked: anything
    with a slash, and any bare name that ends like source code. Not checked: placeholders, flags, web
    addresses, and run-time state under `.council/` (it is not in a fresh checkout)."""
    out = []
    for tok in PATH_RX.findall(re.sub(r"```.*?```", "", text, flags=re.S)):
        t = re.sub(r":\d+(-\d+)?$", "", tok.rstrip(".,;:)").split("#")[0])
        if any(ch in t for ch in "<>*{}$|=()") or "://" in t or t.startswith(("-", "~", "/", ".council")):
            continue
        if "/" in t.strip("/") or t.endswith("/") or Path(t).suffix.lower() in CODE:
            out.append(t)
    return sorted(set(out))


def validate_map(text: str, root: Path) -> tuple[list[str], list[str]]:
    """Errors send the mapper back; warnings go to the chair. Dead paths are the main thing: a map that
    names files which do not exist was written from imagination."""
    errs, warns = [], []
    words = len(text.split())
    if words < 200:
        errs.append(f"the map is {words} words: too thin to find one's way by")
    elif words > 3500:
        warns.append(f"the map is {words} words (aim for 600 to 2,000: a map, not a transcript)")
    heads = [h.strip().lower() for h in re.findall(r"^##\s+(.+?)\s*$", text, re.M)]
    for need in REQUIRED:
        if not any(need.lower() in h for h in heads):
            errs.append(f"missing section: ## {need}")
    for want in ADVISED:
        if not any(want.lower() in h for h in heads):
            warns.append(f"no section: ## {want}")
    if "```mermaid" not in text:
        warns.append("no Mermaid diagram in the bird's-eye view")
    if not re.search(r"(?im)^>?\s*\**ELI5\b", text):
        warns.append("no ELI5 line under the title")
    files, _ = list_files(root)
    known = set(files) | {"/".join(Path(f).parts[:k]) for f in files for k in range(1, len(Path(f).parts))}
    names = {Path(f).name for f in files}

    def exists(p: str) -> bool:
        q = p.strip("/")
        if "/" not in q:
            return q in names or q in known
        return q in known or any(k.endswith("/" + q) for k in known) or (root / q).exists()

    dead = [p for p in named_paths(text) if not exists(p)]
    if len(dead) >= 3:
        errs.append("these paths are named in the map but do not exist: " + ", ".join(dead[:12]))
    elif dead:
        warns.append("named in the map but not found: " + ", ".join(dead))
    return errs, warns


def install_map(text: str, root: Path, run: str | None = None, scanned: dict | None = None,
                by: str = "the mapper seat") -> tuple[Path, str | None]:
    """Put the map in place and record what it was checked against. A map that a person wrote
    (no marker on its first lines) is never overwritten: the new one lands beside it."""
    path = map_path(root)
    note = None
    if path.exists() and MARKER not in "\n".join(path.read_text(encoding="utf-8", errors="replace").splitlines()[:5]):
        note = f"{path.relative_to(root).as_posix()} was written by hand, so it was left alone"
        path = path.with_name(path.stem + ".council.md")
    scanned = scanned or scan(root)
    body = re.sub(r"\A\s*<!-- council-loop:map.*?-->\s*", "", text, flags=re.S).strip()
    head = (f"{MARKER} · written by {by}" + (f" in run {run}" if run else "") +
            f" on {council.today().isoformat()} · checked against {len(structure(scanned))} source files · "
            "regenerate: council map -->")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(head + "\n" + body + "\n", encoding="utf-8", newline="\n")
    accept(root, scanned, run)
    return path, note


# ----------------------------------------------------------------------------- CLI glue
def cmd_map(check: bool = False, accept_now: bool = False, validate: bool = False, out: str | None = None,
            budget: int = 420, install: str | None = None, parsers: bool = False, focus: str | None = None) -> int:
    root = council.project_root()
    if parsers:
        res = fetch_parsers(root)
        if not res["ok"]:
            print(res["note"])
            return 1
        print(f"tree-sitter parsers: {len(res['have'])} of {len(res['wanted'])} languages in this repository are ready"
              + (f"; fetched now: {', '.join(res['fetched'])}" if res["fetched"] else ""))
        return 0
    if focus is not None:
        fm = focus_map(root, focus)
        print(fm["markdown"] or "no source code found in this project")
        print(f"\n(about {fm['tokens']} tokens; names the text mentions: {', '.join(fm['mentioned']) or 'none'})")
        return 0
    sk = skeleton(root, budget)
    st, path = sk["stale"], map_path(root)
    if install:
        src = Path(install)
        if not src.exists():
            print(f"no file at {install}")
            return 1
        text = re.sub(r"\A\s*```(?:markdown|md)?\s*\n(.*?)\n```\s*\Z", r"\1", src.read_text(encoding="utf-8", errors="replace"), flags=re.S)
        errs, warns = validate_map(text, root)
        for w in warns:
            print("warning:", w)
        for e in errs:
            print("error:", e)
        if errs:
            print(f"not installed: {len(errs)} problem(s)")
            return 1
        placed, note = install_map(text, root, None, sk["scanned"], by="hand (council map --install)")
        print(f"map installed: {placed.relative_to(root).as_posix()}" + (f" ({note})" if note else ""))
        import memory
        try:
            copy = memory.publish_map(placed.read_text(encoding="utf-8"), placed.relative_to(root).as_posix(), None, root)
            if copy:
                print(f"a copy for Obsidian: {copy}")
        except (memory.MemoryError_, OSError) as err:
            print(f"note: no copy in the shared memory ({str(err)[:120]})")
        return 0
    if validate:
        if not path.exists():
            print(f"no map at {path.relative_to(root).as_posix()}")
            return 1
        errs, warns = validate_map(path.read_text(encoding="utf-8", errors="replace"), root)
        for w in warns:
            print("warning:", w)
        for e in errs:
            print("error:", e)
        print("map: ok" if not errs else f"map: {len(errs)} problem(s)")
        return 1 if errs else 0
    if accept_now:
        if not path.exists():
            print(f"no map at {path.relative_to(root).as_posix()} to accept")
            return 1
        accept(root, sk["scanned"])
        print(f"recorded: {path.relative_to(root).as_posix()} describes the code as it is today")
        return 0
    target = Path(out) if out else root / ".council" / "map" / "skeleton.md"
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(sk["markdown"], encoding="utf-8", newline="\n")
    s = sk["stats"]
    print(f"skeleton: {target}  ({s['files']} files, {s['lines']:,} lines, {s['code_files']} source files, "
          f"{s['edges']} internal imports, {s['commands']} commands, {s['tools']} MCP entries)")
    print(f"map: {path.relative_to(root).as_posix()}: " + ("STALE: " if st["stale"] else "") + st["reason"])
    return 1 if (check and st["stale"]) else 0
