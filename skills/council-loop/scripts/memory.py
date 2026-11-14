#!/usr/bin/env python3
"""memory - the council's shared memory: research notes (facts with sources) and the commons
(what members say to each other).

ELI5: a shared notebook with two sections. One holds facts, each with where it came from, and is
kept for good. The other holds what members want to tell each other during a run. Only the referee
script writes in the notebook, so every note has the same shape and no note can leak into a blind vote.

Layout (plain Markdown with YAML properties, so an Obsidian vault can hold it):
  <root>/index.md                 what is here, newest first (rebuilt on every write)
  <root>/research/<slug>.md       ONE fact per note: the claim, its source, a verbatim quote
  <root>/commons/<run id>.md      what the seats said to each other in one run
  <root>/architecture.md          a copy of the codebase map (Obsidian vaults only)

The note layout follows two open conventions, so other tools can read it as it is:
  - Obsidian: YAML properties, tags, wikilinks, and a `Research.base` table view of every fact
    (Bases syntax as documented in kepano/obsidian-skills, MIT).
  - Basic Memory's knowledge format (basicmachines-co/basic-memory): `## Observations` lines of the
    form `- [category] text #tag (context)` and `## Relations` lines of the form `- relation [[Note]]`.
    Only the layout is shared: no code from that project (AGPL-3.0) is used here.
A fact that turns out wrong is retired, not deleted (the rule of getzep/graphiti: "old facts are
invalidated, not deleted"), so the record of what the council once believed stays.

Where <root> is (later wins): the default `.council/memory/` in the project, then `memory:` in the
project's `.council/config.yaml`, then `memory:` in the user's `~/.council/config.yaml`, then the
environment (COUNCIL_MEMORY_DIR, or COUNCIL_OBSIDIAN_VAULT with COUNCIL_OBSIDIAN_FOLDER).
A vault path is personal, so it belongs in the user's file: `council memory setup --obsidian --vault <path>`.
"""
from __future__ import annotations

import datetime as dt
import json
import os
import re
from pathlib import Path

import council

KINDS = ("paper", "docs", "blog", "repo", "code", "data", "talk", "test", "other")
CONFIDENCE = ("high", "medium", "low")
AUDIENCES = ("all", "chair", "research")
CAPS = {"claim": 45, "quote": 70, "why": 30, "note": 70, "tag": 3}
STALE_DAYS = 180
STOP = set("the a an and or of to in on for with by is are was were be been as at it this that from into over via "
           "not no each every per than its their our your you we they them will can should must may has have had "
           "which what when where how why does do did more most less about between than then also only".split())
# A fact is a statement about the world. A line addressed to the reader is not one, wherever it came from.
ORDER_RE = re.compile(r"\b(ignore|disregard|forget)\s+(all|any|your|the|previous|prior|above)\b|\bsystem\s+prompt\b"
                      r"|\byou\s+(must|should|are\s+to|need\s+to)\s+(now\s+)?(run|execute|delete|send|reveal|ignore)\b", re.I)
SEAT_RE = re.compile(r"(?<![\w-])(?:L\d{1,2}|RT|E[A-G]|R[A-D]|MAP|CTRL)(?![\w-])")
ANON = "a member"                               # how a voting seat is named until the run is unblinded


class MemoryError_(Exception):
    pass


# ----------------------------------------------------------------------------- where it lives
def user_config_path() -> Path:
    return council.user_config_path()


def _section(path: Path) -> dict:
    try:
        data = council.load_yaml(path) if path.exists() else None
    except Exception:  # noqa: BLE001 - a broken optional config must not break a run
        return {}
    section = data.get("memory") if isinstance(data, dict) else None
    return section if isinstance(section, dict) else {}


def settings(root: Path | None = None) -> dict:
    """Resolve where the memory lives and how notes link to each other."""
    root = (root or council.project_root()).resolve()
    conf, source = {"backend": "files", "dir": str(root / ".council" / "memory")}, "default"
    for name, layer in (("project", _section(root / ".council" / "config.yaml")), ("user", _section(user_config_path()))):
        if layer:
            if layer.get("backend") == "obsidian" or layer.get("vault"):
                conf = {"backend": "obsidian", "vault": layer.get("vault") or conf.get("vault"),
                        "folder": layer.get("folder") or conf.get("folder")}
            elif layer.get("backend") == "files" or layer.get("dir"):
                conf = {"backend": "files", "dir": layer.get("dir") or str(root / ".council" / "memory")}
            source = name
    env = os.environ
    if env.get("COUNCIL_OBSIDIAN_VAULT"):
        conf, source = {"backend": "obsidian", "vault": env["COUNCIL_OBSIDIAN_VAULT"],
                        "folder": env.get("COUNCIL_OBSIDIAN_FOLDER") or conf.get("folder")}, "env"
    elif env.get("COUNCIL_MEMORY_DIR"):
        conf, source = {"backend": "files", "dir": env["COUNCIL_MEMORY_DIR"]}, "env"
    if conf["backend"] == "obsidian":
        vault = Path(str(conf.get("vault") or "")).expanduser()
        folder = conf.get("folder") or "Council Loop/" + (re.sub(r"[#|^:%\[\]]+", "-", root.name) or "project")
        return {"backend": "obsidian", "vault": str(vault), "folder": folder, "dir": str(vault / folder),
                "links": "wikilinks", "source": source, "ok": bool(conf.get("vault")) and vault.is_dir(),
                "problem": None if (conf.get("vault") and vault.is_dir()) else
                f"the Obsidian vault {str(vault)!r} is not a folder on this machine. Set it: "
                "council memory setup --obsidian --vault \"<path to your vault>\""}
    d = Path(conf["dir"]).expanduser()
    d = d if d.is_absolute() else root / d
    return {"backend": "files", "dir": str(d), "links": "markdown", "source": source, "ok": True, "problem": None}


def store(conf: dict | None = None, root: Path | None = None) -> Path:
    """The memory folder, created on demand. `conf` is a run's pinned settings, when there is a run."""
    conf = conf or settings(root)
    if not conf.get("ok", True):
        raise MemoryError_(conf.get("problem") or "the memory folder is not usable")
    d = Path(conf["dir"])
    (d / "research").mkdir(parents=True, exist_ok=True)
    (d / "commons").mkdir(parents=True, exist_ok=True)
    return d


# ----------------------------------------------------------------------------- small helpers
def tidy(text, cap_words: int | None = None) -> str:
    """One line, no control characters, no link or heading syntax that could misfire in a vault."""
    s = "".join(ch if ch.isprintable() else " " for ch in str(text or ""))
    s = " ".join(s.replace("[[", "[").replace("]]", "]").split()).lstrip("#>-* ").strip()
    if cap_words and len(s.split()) > cap_words:
        s = " ".join(s.split()[:cap_words]) + " …"
    return s


def tokens(text: str) -> set:
    return {w for w in re.findall(r"[a-z0-9][a-z0-9+.#-]*", str(text).lower()) if len(w) > 2 and w not in STOP}


WINDOWS_RESERVED = {"con", "prn", "aux", "nul", *(f"com{i}" for i in range(1, 10)), *(f"lpt{i}" for i in range(1, 10))}


def slugify(text: str, n: int = 8) -> str:
    ws = [w for w in re.findall(r"[a-z0-9]+", text.lower()) if w not in STOP and len(w) > 1]
    slug = "-".join(ws[:n])[:64].strip("-") or "note"
    return f"note-{slug}" if slug in WINDOWS_RESERVED else slug     # con.md cannot exist on Windows


def norm_source(src: str) -> str:
    s = str(src or "").strip()
    if re.match(r"https?://", s, re.I):
        s = re.sub(r"#.*$", "", s)
        s = re.sub(r"^(https?://)(www\.)?", lambda m: m.group(1).lower(), s, flags=re.I)
        return s.rstrip("/").lower()
    return s


def unsigned(text: str) -> str:
    """A note as other seats may read it: seat ids replaced, so a thought never says whose card is whose."""
    return SEAT_RE.sub("a member", text)


def _yaml_value(v) -> str:
    if isinstance(v, bool):
        return "true" if v else "false"
    if isinstance(v, (int, float)):
        return str(v)
    if isinstance(v, (list, tuple)):
        return "[" + ", ".join(json.dumps(str(x), ensure_ascii=False) for x in v) + "]"
    return json.dumps(str(v), ensure_ascii=False)


def _front(props: dict) -> str:
    return "---\n" + "\n".join(f"{k}: {_yaml_value(v)}" for k, v in props.items() if v not in (None, "", [])) + "\n---\n"


def read_note(path: Path) -> dict:
    """A note's properties plus `path` and `body`. Tolerant: a note someone edited by hand still reads."""
    text = path.read_text(encoding="utf-8", errors="replace")
    props, body = {}, text
    if text.startswith("---"):
        end = text.find("\n---", 3)
        if end != -1:
            try:
                import yaml
                props = yaml.safe_load(text[3:end]) or {}
            except Exception:  # noqa: BLE001
                props = {}
            body = text[end + 4:]
    return {**(props if isinstance(props, dict) else {}), "path": str(path), "id": path.stem, "body": body.strip()}


def link(conf: dict, folder: str, slug: str, title: str, up: bool = False) -> str:
    """A link to a note. `up`: the link is written inside one of the memory's subfolders."""
    title = title.replace("|", "/").replace("[", "(").replace("]", ")")
    if conf.get("links") == "wikilinks":
        where = "/".join(x for x in (str(conf.get("folder") or "").strip("/"), folder, slug) if x)
        return f"[[{where}|{title}]]"
    path = f"{folder}/{slug}.md" if folder else f"{slug}.md"
    return f"[{title}]({'../' if up else ''}{path})"


def live(notes: list[dict]) -> list[dict]:
    """Facts still held: a retired fact stays on disk for the record but is no longer handed to seats."""
    return [n for n in notes if str(n.get("status") or "") != "retired"]


# ----------------------------------------------------------------------------- research notes (facts)
def _in_project(root: Path, rel: str) -> bool:
    """A file source counts only when it names something inside the project (no `../` walks out of it)."""
    try:
        base, target = root.resolve(), (root / rel).resolve()
    except (OSError, ValueError):
        return False
    return bool(rel.strip()) and target.exists() and (target == base or base in target.parents)


def clean_fact(item, root: Path | None = None) -> tuple[dict | None, str | None]:
    """Return (fact, None) or (None, why it was refused). A fact needs a claim and a source."""
    if not isinstance(item, dict):
        return None, "not an object"
    claim = tidy(item.get("claim"), CAPS["claim"])
    if len(claim.split()) < 4:
        return None, "the claim is missing or under 4 words"
    src = str(item.get("source") or "").strip()
    if not src:
        return None, f"no source for: {claim[:60]!r}"
    quote, why = tidy(item.get("quote"), CAPS["quote"]), tidy(item.get("why"), CAPS["why"])
    if ORDER_RE.search(f"{claim} {quote} {why}"):
        return None, f"reads like an instruction, not a fact: {claim[:60]!r}"
    web = bool(re.match(r"https?://\S+\.\S+", src))
    kind = str(item.get("kind") or "").lower()
    if not web:
        if src.lower() in ("memory", "recall", "training", "from memory"):
            src, kind = "memory", "other"
        elif src.lower().startswith("test:"):
            kind = "test"
        elif _in_project(root or council.project_root(), src.split(":")[0].split("#")[0]):
            kind = kind if kind in ("code", "data", "docs", "test") else "code"
        else:
            return None, f"the source {src[:80]!r} is not a web address, a file in this project, or 'test: <command>'"
    conf = str(item.get("confidence") or "medium").lower()
    checked = item.get("checked")
    checked = (src != "memory") if checked is None else bool(checked)
    if src == "memory":
        conf, checked = "low", False
    tags = [slugify(str(t), CAPS["tag"]) for t in (item.get("tags") or []) if str(t).strip()][:6] \
        if isinstance(item.get("tags"), (list, tuple)) else []
    tags = [t if re.search(r"[a-z]", t) else f"n-{t}" for t in tags if t]
    return {"claim": claim, "source": src, "quote": quote, "why": why, "kind": kind if kind in KINDS else "other",
            "date": tidy(item.get("date"), 4) or "n.d.", "confidence": conf if conf in CONFIDENCE else "medium",
            "checked": checked, "tags": tags}, None


def facts(conf: dict | None = None, root: Path | None = None) -> list[dict]:
    d = Path((conf or settings(root))["dir"]) / "research"
    return [read_note(p) for p in sorted(d.glob("*.md"))] if d.is_dir() else []


def add_research(items, *, by: str, role: str = "", model: str | None = None, run: str | None = None,
                 conf: dict | None = None, root: Path | None = None, anonymous: bool = False) -> dict:
    """Publish facts. Returns {"added": [ids], "merged": [ids], "dropped": [reasons]}. A fact already
    held (same source, much the same claim) is not written twice: the note records who else found it.
    anonymous: the note says "a member" and no model until `sign` is called. A voting seat publishes
    this way, so that a fact on disk never tells a voter who wrote the card that cites it."""
    who, which = (ANON, "") if anonymous else (by, model or "")
    conf = conf or settings(root)
    d = store(conf, root) / "research"
    have = facts(conf, root)
    out = {"added": [], "merged": [], "dropped": []}
    today = council.today().isoformat()
    for raw in items if isinstance(items, (list, tuple)) else []:
        fact, problem = clean_fact(raw, root)
        if not fact:
            out["dropped"].append(problem)
            continue
        twin = next((n for n in have if norm_source(n.get("source")) == norm_source(fact["source"]) and
                     len(tokens(n.get("title", "")) & tokens(fact["claim"])) /
                     max(1, len(tokens(n.get("title", "")) | tokens(fact["claim"]))) >= 0.6), None)
        if twin:
            if not (str(twin.get("run") or "") == (run or "") and twin.get("by") == who):    # not its own echo
                _also_found(Path(twin["path"]), f"{run or 'by hand'}/{who}")
            out["merged"].append(twin["id"])
            continue
        slug, n = slugify(fact["claim"]), 2
        while (d / f"{slug}.md").exists():
            slug, n = f"{slugify(fact['claim'])}-{n}", n + 1
        props = {"type": "research", "title": fact["claim"], "source": fact["source"], "source_date": fact["date"],
                 "kind": fact["kind"], "confidence": fact["confidence"], "checked": fact["checked"], "added": today,
                 "run": run or "", "by": who, "role": role, "model": which,
                 "tags": ["council/research", *fact["tags"]], "quote": fact["quote"], "why": fact["why"], "seen_in": []}
        same = [n["id"] for n in have if norm_source(n.get("source")) == norm_source(fact["source"])
                and fact["source"] != "memory"][:5]
        _write_fact(d / f"{slug}.md", props, conf, same)
        have.append({**props, "path": str(d / f"{slug}.md"), "id": slug})
        out["added"].append(slug)
    if out["added"] or out["merged"]:
        write_index(conf, root)
    return out


def _also_found(path: Path, mark: str) -> None:
    """Record that someone else found the same fact. Only the properties and one line change, so
    anything a person wrote into the note stays."""
    note = read_note(path)
    seen = [str(x) for x in (note.get("seen_in") or [])]
    if mark in seen:
        return
    seen.append(mark)
    props = {k: v for k, v in note.items() if k not in ("path", "id", "body")}
    props["seen_in"] = seen
    line = "Also found by: " + ", ".join(f"`{x}`" for x in seen) + "."
    body = re.sub(r"(?m)^Also found by: .*$", line, note["body"]) if "Also found by: " in note["body"] else note["body"] + "\n" + line
    path.write_text(_front(props) + body + "\n", encoding="utf-8", newline="\n")


def _write_fact(path: Path, p: dict, conf: dict | None = None, same_source=()) -> None:
    conf = conf or {"links": "markdown"}
    src = p.get("source", "")
    where = f"[{re.sub(r'^https?://(www[.])?', '', src).split('/')[0]}]({src})" if re.match(r"https?://", src) else f"`{src}`"
    body = [f"# {p['title']}", ""]
    if p.get("quote"):
        body += [f"> {p['quote']}", ""]
    body.append(f"Source: {where} ({p.get('source_date') or 'n.d.'}). Kind: {p.get('kind')}. Confidence: {p.get('confidence')}. "
                + ("Opened when it was added." if p.get("checked") else "NOT re-checked: recalled, or passed on."))
    if p.get("why"):
        body += ["", f"Why it matters: {p['why']}"]
    host = re.sub(r"^https?://(www[.])?", "", src).split("/")[0] if re.match(r"https?://", src) else src
    tags = " ".join(f"#{t}" for t in (p.get("tags") or []) if "/" not in t)
    body += ["", "## Observations", "",
             f"- [fact] {p['title']}{' ' + tags if tags else ''} ({p.get('confidence')} confidence; {host}, {p.get('source_date') or 'n.d.'})"]
    rel = [f"- found_in {link(conf, 'commons', p['run'], p['run'], up=True)}"] if p.get("run") else []
    rel += [f"- same_source {link(conf, 'research', other, other, up=True)}" for other in same_source]
    if rel:
        body += ["", "## Relations", ""] + rel
    who = f"{p.get('by')}" + (f" ({p.get('role')}" + (f", {p.get('model')}" if p.get("model") else "") + ")" if p.get("role") else "")
    body += ["", f"Added {p.get('added')} by {who}" + (f" in run `{p.get('run')}`." if p.get("run") else ".")]
    if p.get("seen_in"):
        body.append("Also found by: " + ", ".join(f"`{x}`" for x in p["seen_in"]) + ".")
    keys = ("type", "title", "source", "source_date", "kind", "confidence", "checked", "added", "run", "by", "role",
            "model", "tags", "quote", "why", "seen_in")
    path.write_text(_front({k: p.get(k) for k in keys}) + "\n".join(body) + "\n", encoding="utf-8", newline="\n")


def sign(authors: dict, seats: dict, conf: dict | None = None, root: Path | None = None) -> int:
    """Put names on the facts that voting seats published, once the run is unblinded.
    authors: {note id: seat id}. seats: the run's seats (for the model name)."""
    d = Path((conf or settings(root))["dir"]) / "research"
    done = 0
    for note_id, seat in (authors or {}).items():
        path = d / f"{note_id}.md"
        if not path.exists():
            continue
        note = read_note(path)
        if note.get("by") != ANON:
            continue                                           # signed already, or edited by hand
        model = str((seats.get(seat) or {}).get("model") or "")
        props = {k: v for k, v in note.items() if k not in ("path", "id", "body")}
        props.update(by=seat, model=model)
        who = f"{seat} ({note.get('role') or 'member'}" + (f", {model}" if model else "") + ")"
        body = re.sub(rf"(?m)^(Added \S+ by ){re.escape(ANON)}(?: \([^)]*\))?", lambda m: m.group(1) + who, note["body"], count=1)
        path.write_text(_front(props) + body + "\n", encoding="utf-8", newline="\n")
        done += 1
    return done


def import_notes(src: str, conf: dict | None = None, root: Path | None = None) -> dict:
    """Bring research notes from another memory folder into this one (for example from a project's
    `.council/memory` into an Obsidian vault). Facts already held are merged, not copied twice."""
    src_dir = Path(src).expanduser()
    src_dir = src_dir / "research" if (src_dir / "research").is_dir() else src_dir
    if not src_dir.is_dir():
        raise MemoryError_(f"{str(src_dir)!r} is not a folder")
    conf = conf or settings(root)
    if src_dir.resolve() == (Path(conf["dir"]) / "research").resolve():
        raise MemoryError_("that is this memory's own folder")
    total = {"added": [], "merged": [], "dropped": []}
    for path in sorted(src_dir.glob("*.md")):
        n = read_note(path)
        if n.get("type") != "research":
            continue
        res = add_research([{"claim": n.get("title"), "source": n.get("source"), "quote": n.get("quote"),
                             "kind": n.get("kind"), "confidence": n.get("confidence"), "checked": n.get("checked"),
                             "date": n.get("source_date"), "why": n.get("why"),
                             "tags": [t for t in (n.get("tags") or []) if t != "council/research"]}],
                           by=str(n.get("by") or "import"), role=str(n.get("role") or ""), model=n.get("model"),
                           run=str(n.get("run") or "") or None, conf=conf, root=root)
        for k in total:
            total[k] += res[k]
        if n.get("status") == "retired" and res["added"]:      # a retired fact arrives retired, with its reason
            retire(res["added"][0], str(n.get("retired_because") or "retired before it was imported"), conf=conf, root=root)
    return total


def retire(note_id: str, because: str, by: str | None = None, conf: dict | None = None, root: Path | None = None) -> Path:
    """Mark a fact as no longer held. It stays on disk, with the reason and what replaced it."""
    conf = conf or settings(root)
    d = Path(conf["dir"]) / "research"
    path = d / f"{note_id.removesuffix('.md')}.md"
    if not path.exists():
        raise MemoryError_(f"no research note named {note_id!r} (find it: council memory search \"<words>\")")
    why = tidy(because, 40)
    if len(why.split()) < 3:
        raise MemoryError_('say why it is retired: --because "<reason>"')
    if by and not (d / f"{by.removesuffix('.md')}.md").exists():
        raise MemoryError_(f"the replacement {by!r} is not a research note")
    note = read_note(path)
    props = {k: v for k, v in note.items() if k not in ("path", "id", "body")}
    today = council.today().isoformat()
    props.update(status="retired", retired=today, retired_because=why, **({"superseded_by": by.removesuffix(".md")} if by else {}))
    line = f"RETIRED {today}: {why}" + (f" Replaced by {link(conf, 'research', by.removesuffix('.md'), by.removesuffix('.md'), up=True)}." if by else "")
    body = re.sub(r"(?m)^RETIRED \d{4}-\d\d-\d\d: .*$\n?", "", note["body"]).rstrip()
    path.write_text(_front(props) + body + "\n\n" + line + "\n", encoding="utf-8", newline="\n")
    write_index(conf, root)
    return path


def search(query: str, *, tags: list[str] | None = None, limit: int = 10, conf: dict | None = None,
           root: Path | None = None, retired: bool = False) -> list[dict]:
    """Keyword search over the facts. Plain word overlap, tags count double, newer wins a tie.
    (Add embeddings only when this stops finding things: see reference/context-and-memory.md.)"""
    q, want = tokens(query), {slugify(t, CAPS["tag"]) for t in (tags or [])}
    hits = []
    for n in (facts(conf, root) if retired else live(facts(conf, root))):
        ntags = {str(t) for t in (n.get("tags") or [])}
        if want and not want & ntags:
            continue
        text = tokens(f"{n.get('title', '')} {n.get('quote', '')} {n.get('why', '')}")
        score = len(q & text) + 2 * len(q & {w for t in ntags for w in t.split("-")})
        if score or not q or want:
            hits.append({**n, "score": score})
    hits.sort(key=lambda n: (n["score"], str(n.get("added") or "")), reverse=True)
    return hits[:limit]


def fact_line(n: dict, today: dt.date | None = None) -> str:
    src = str(n.get("source") or "")
    where = re.sub(r"^https?://(www[.])?", "", src).split("/")[0] if re.match(r"https?://", src) else src
    flags = [str(n.get("confidence") or "medium")]
    if not n.get("checked", True):
        flags.append("not re-checked")
    try:
        if ((today or council.today()) - dt.date.fromisoformat(str(n.get("added")))).days > STALE_DAYS:
            flags.append(f"added {n.get('added')}: recheck")
    except (TypeError, ValueError):
        pass
    return f"- {n.get('title')} ({where}, {n.get('source_date') or 'n.d.'}; {', '.join(flags)})"


def digest(about: str, *, run: str | None = None, limit: int = 14, conf: dict | None = None,
           root: Path | None = None) -> dict:
    """The facts a seat should see: everything this run added, then older notes that match the brief."""
    conf = conf or settings(root)
    if not conf.get("ok", True) or not Path(conf["dir"]).is_dir():
        return {"items": [], "lines": [], "dir": conf.get("dir")}
    notes = live(facts(conf, root))
    mine = [n for n in notes if run and str(n.get("run")) == run]
    rank = {"high": 0, "medium": 1, "low": 2}
    mine.sort(key=lambda n: (rank.get(str(n.get("confidence")), 1), n["id"]))
    q = tokens(about)
    older = [n for n in notes if n not in mine]
    scored = sorted(((len(q & tokens(f"{n.get('title', '')} {' '.join(map(str, n.get('tags') or []))}")), n) for n in older),
                    key=lambda x: (-x[0], x[1]["id"]))
    picked = (mine + [n for s, n in scored if s >= 2])[:limit]
    return {"items": picked, "lines": [fact_line(n) for n in picked], "dir": str(Path(conf["dir"]) / "research"),
            "total": len(notes), "from_this_run": len(mine)}


# ----------------------------------------------------------------------------- commons (thoughts)
def clean_note(item) -> tuple[dict | None, str | None]:
    if isinstance(item, str):
        item = {"to": "all", "text": item}
    if not isinstance(item, dict):
        return None, "not an object"
    text = tidy(item.get("text"), CAPS["note"])
    if len(text.split()) < 3:
        return None, "empty note"
    if ORDER_RE.search(text):
        return None, f"reads like an instruction to other members: {text[:60]!r}"
    to = str(item.get("to") or "all").lower().strip()
    to = {"researchers": "research", "researcher": "research", "everyone": "all", "council": "all"}.get(to, to)
    return {"to": to if to in AUDIENCES else "all", "text": text}, None


def write_commons(run: dict, conf: dict | None = None, root: Path | None = None, signed: bool = False) -> Path | None:
    """Rewrite the run's commons page from the posts held in run.json (the run is the source of truth).
    While the run is live the page names roles only; `signed` adds the seat ids, at the end."""
    posts = run.get("commons") or []
    conf = conf or settings(root)
    filed = [n for n in facts(conf, root) if str(n.get("run") or "") == run["id"]] if Path(conf["dir"]).is_dir() else []
    if not posts and not filed:
        return None
    f = store(conf, root) / "commons" / f"{run['id']}.md"
    lines = [_front({"type": "commons", "run": run["id"], "tags": ["council/commons"], "posts": len(posts),
                     "signed": signed}),
             f"# Commons: {run['id']}", "", f"> {tidy(run.get('ask'), 60)}", "",
             "What the council's members said to each other. Other seats read these without names, one step later,",
             "and never inside a blind vote. Questions marked `research` go to the researchers.",
             ("Names were added when the run ended." if signed else "Names are added when the run ends."), ""]
    step = None
    for p in posts:
        here = f"Loop {p['loop']} · {p['phase']}"
        if here != step:
            lines += ([""] if step else []) + [f"## {here}", ""]
            step = here
        if p["role"] == "chair":
            who = "the chair"
        else:
            who = f"{p['by']} ({p['role']})" if signed or p["role"] in ("researcher", "mapper") else f"{ANON} ({p['role']})"
        lines.append(f"- **{who} → {p['to']}**: {p['text'] if signed else unsigned(p['text'])}")
    if not posts:
        lines.append("No notes were posted in this run.")
    if filed:
        lines += ["", "## Research filed in this run", ""]
        lines += [f"- {link(conf, 'research', n['id'], str(n.get('title') or n['id']), up=True)} ({n.get('confidence')})" for n in filed]
    f.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    return f


# ----------------------------------------------------------------------------- index, map copy
def write_index(conf: dict | None = None, root: Path | None = None, map_path: str | None = None) -> Path:
    conf = conf or settings(root)
    d = store(conf, root)
    every = facts(conf, root)
    notes = sorted(live(every), key=lambda n: (str(n.get("added") or ""), n["id"]), reverse=True)
    runs = sorted((p.stem for p in (d / "commons").glob("*.md")), reverse=True)
    if conf.get("backend") == "obsidian":
        write_base(conf, d)
    out = [_front({"type": "index", "tags": ["council/index"], "updated": council.today().isoformat()}),
           "# Council memory", "",
           "Shared by every council run in this project. Written only by `council`; safe to read, link and tag.", "",
           f"- **Research** ({len(notes)} notes" + (f", plus {len(every) - len(notes)} retired" if len(every) > len(notes) else "") +
           "): one sourced fact per note, in `research/`."
           + (" As a table: open `Research.base`." if conf.get("backend") == "obsidian" else ""),
           f"- **Commons** ({len(runs)} runs): what members said to each other, in `commons/`."]
    if (d / "architecture.md").exists():
        out.append("- **Codebase map**: " + link(conf, "", "architecture", "architecture"))
    elif map_path:
        out.append(f"- **Codebase map**: `{map_path}` in the project.")
    out += ["", "## Newest research", ""]
    out += [f"- {link(conf, 'research', n['id'], str(n.get('title') or n['id']))} "
            f"({n.get('confidence')}, {n.get('source_date') or 'n.d.'})" for n in notes[:40]] or ["- none yet"]
    if runs:
        out += ["", "## Commons by run", ""] + [f"- {link(conf, 'commons', r, r)}" for r in runs[:40]]
    out += ["", "## Use it", "",
            "- Find: `council memory search \"<words>\"`",
            "- Add by hand: `council memory add --claim \"…\" --source <url or file> --quote \"…\"`",
            "- Where this folder lives: `council memory status`"]
    (d / "index.md").write_text("\n".join(out) + "\n", encoding="utf-8", newline="\n")
    return d / "index.md"


def write_base(conf: dict, d: Path) -> Path:
    """An Obsidian Bases file: every research note as a sortable table. Plain YAML, written by hand so
    that the filter strings are quoted the way Obsidian's own examples quote them."""
    folder = (str(conf.get("folder") or "").strip("/") + "/research").replace("\\", "\\\\").replace('"', '\\"').replace("'", "''")
    text = f"""filters:
  and:
    - 'file.inFolder("{folder}")'
    - 'type == "research"'
properties:
  title:
    displayName: Fact
  source_date:
    displayName: Published
  checked:
    displayName: Opened
views:
  - type: table
    name: "Facts held"
    filters:
      and:
        - 'status != "retired"'
    order:
      - title
      - confidence
      - kind
      - source_date
      - source
      - run
      - checked
    groupBy:
      property: confidence
      direction: ASC
  - type: table
    name: "By run"
    order:
      - title
      - confidence
      - source
      - added
    groupBy:
      property: run
      direction: DESC
  - type: table
    name: "Re-check these"
    filters:
      or:
        - 'checked == false'
        - 'confidence == "low"'
    order:
      - title
      - source
      - added
  - type: table
    name: "Retired"
    filters:
      and:
        - 'status == "retired"'
    order:
      - title
      - retired
      - retired_because
      - superseded_by
"""
    f = d / "Research.base"
    f.write_text(text, encoding="utf-8", newline="\n")
    return f


def publish_map(text: str, source_path: str, conf: dict | None = None, root: Path | None = None) -> Path | None:
    """Copy the codebase map into the memory when the memory is an Obsidian vault, so it can be read there."""
    conf = conf or settings(root)
    if conf.get("backend") != "obsidian":
        return None
    f = store(conf, root) / "architecture.md"
    body = re.sub(r"\A<!--.*?-->\s*", "", text, flags=re.S)
    f.write_text(_front({"type": "architecture", "tags": ["council/architecture"], "source": source_path,
                         "updated": council.today().isoformat()}) + body, encoding="utf-8", newline="\n")
    return f


# ----------------------------------------------------------------------------- CLI glue
def cmd_setup(obsidian: bool, vault: str | None, folder: str | None, files: bool, directory: str | None,
              project: bool) -> int:
    """Write the `memory:` section of the user's config (or the project's, with --project)."""
    import yaml
    root = council.project_root()
    path = root / ".council" / "config.yaml" if project else user_config_path()
    if obsidian:
        if not vault:
            raise MemoryError_('give the vault: council memory setup --obsidian --vault "<path to your vault>"')
        v = Path(vault).expanduser()
        if not v.is_dir():
            raise MemoryError_(f"{str(v)!r} is not a folder. Open Obsidian, pick the vault, and pass its folder path")
        section = {"backend": "obsidian", "vault": str(v), **({"folder": folder} if folder else {})}
    elif files:
        section = {"backend": "files", **({"dir": directory} if directory else {})}
    else:
        raise MemoryError_("say which: --obsidian --vault <path>, or --files [--dir <path>]")
    data = {}
    if path.exists():
        try:
            data = yaml.safe_load(path.read_text(encoding="utf-8")) or {}
        except Exception as err:  # noqa: BLE001
            raise MemoryError_(f"{path} is not valid YAML: {err}") from err
    data["memory"] = section
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(yaml.safe_dump(data, sort_keys=False), encoding="utf-8", newline="\n")
    conf = settings(root)
    print(f"saved to {path} ({'this project' if project else 'your user settings: every project on this machine'})")
    return cmd_status(conf)


def cmd_status(conf: dict | None = None) -> int:
    conf = conf or settings()
    print(f"memory: {conf['backend']} at {conf['dir']}  (set by: {conf['source']})")
    if not conf.get("ok", True):
        print("PROBLEM:", conf["problem"])
        return 1
    d = Path(conf["dir"])
    n = len(list((d / "research").glob("*.md"))) if (d / "research").is_dir() else 0
    c = len(list((d / "commons").glob("*.md"))) if (d / "commons").is_dir() else 0
    print(f"research notes: {n} · runs with commons: {c} · links: {conf['links']}")
    if conf["backend"] == "obsidian":
        print(f"In Obsidian: open the vault, then the folder \"{conf['folder']}\". Start at index.")
    return 0


def cmd_add(claim, source, quote, kind, confidence, tags, date, why, by) -> int:
    res = add_research([{"claim": claim, "source": source, "quote": quote, "kind": kind, "confidence": confidence,
                         "tags": [t for t in re.split(r"[,\s]+", tags or "") if t], "date": date, "why": why}],
                       by=by or "chair", role="by hand")
    if res["dropped"]:
        raise MemoryError_("not added: " + "; ".join(res["dropped"]))
    print("added " + ", ".join(res["added"]) if res["added"] else "already held: " + ", ".join(res["merged"]))
    return 0


def cmd_import(src: str) -> int:
    res = import_notes(src)
    print(f"imported: {len(res['added'])} added, {len(res['merged'])} already held, {len(res['dropped'])} refused")
    for why in res["dropped"][:10]:
        print("  refused:", why)
    return 0


def cmd_retire(note_id: str, because: str, by: str | None) -> int:
    path = retire(note_id, because, by)
    print(f"retired: {path.stem} (kept on disk; seats no longer receive it)")
    return 0


def cmd_search(query: str, tag: str | None, limit: int, retired: bool = False) -> int:
    hits = search(query, tags=[tag] if tag else None, limit=limit, retired=retired)
    for n in hits:
        print(fact_line(n) + ("  [RETIRED: " + str(n.get("retired_because")) + "]" if n.get("status") == "retired" else ""))
        print(f"    {n.get('source')}  ·  {n['path']}")
    if not hits:
        print("nothing found")
    return 0
