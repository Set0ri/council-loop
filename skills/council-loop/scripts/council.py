#!/usr/bin/env python3
"""council - the one enforcement path for the Council Loop (CLI + importable functions).

ELI5: the AI does the thinking; this script does the bookkeeping that should never be left to an
AI's good intentions: remembering where the run is, hiding who wrote what, shuffling, counting
votes, cutting 7 -> 5 -> 3 -> 1, checking the rules were followed, and locking test files.

Every surface (Claude Code skill, plugin hook, MCP server, AGENTS.md tools, CI) calls these same
functions, so the rules are enforced the same way everywhere.

Run loop (resumable; state lives in .council/runs/<id>/run.json)
  doctor        what can be called from here (CLIs, keys, stale data) and which mode fits
  init          start a run: gate, seats, files. Prints the GATE line
  next          prepare the current step (writes prompt packets) and say exactly what to do
  dispatch      run the seats that live behind a CLI or API (codex, gemini, openrouter, claude -p)
  ingest        validate what came back; advance when it is all valid (--drop names a seat to go on without)
  cut           tally + elders + wildcard -> survivors; writes the loop ledger
  status        one-screen digest of the run and what is still missing (use it after a context reset)
  deviation     record that the run left the protocol, and why
  audit         re-count every vote from the ballots on disk and compare with the record

Building blocks (also used on their own)
  plan          dry-run the gate (no tokens spent)
  models        model picks for a mode/profile from models/lock.yaml
  blind         anonymise + shuffle candidate cards into a judge packet (seeded), enforce caps
  tally         count blind ballots as head-to-heads weighted by stake; checks beat votes; agreement; cut
  check         validate a run ledger against the protocol
  lock-tests    record sha256 of files agents may not edit (tests, migrations, ...)
  verify-locks  fail if a locked file changed (use --against <git ref> in CI)
  ask / elders  one prompt to one provider / one blinded packet to the elder council
  refresh       rebuild models/lock.yaml from Artificial Analysis data
Run `council <command> -h` for options. Python 3.10+, PyYAML.
"""
from __future__ import annotations

import argparse
import fnmatch
import hashlib
import json
import math
import os
import random
import re
import shutil
import subprocess
import sys
from pathlib import Path

VERSION = "0.4.0"                                                # the kit's version: the same in Cargo.toml, pyproject.toml, plugin.json and SKILL.md
SKILL_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(Path(__file__).resolve().parent))

TIERS = ("direct", "lite", "full")
PROFILES = ("max", "balanced", "efficient")
FAMILIES = ("claude", "gemini", "openai", "openrouter", "open", "mix")

# Shape of each tier. start = candidates fused in loop 1; keep = cards each expert keeps.
SHAPES = {
    "full": {"experts": 7, "ideas_min": 13, "keep": 3, "elders": 3, "start": 7, "loops": 3, "rubric": (5, 7)},
    "lite": {"experts": 3, "ideas_min": 5, "keep": 2, "elders": 1, "start": 3, "loops": 1, "rubric": (3, 7)},
}
CAPS = {"idea": 25, "card": 120, "title": 10, "eli5": 25, "finding": 40, "fix": 30}
GRACE = 1.1  # models count words loosely: up to 10% over a cap is a warning, more is an error

# Text tripwires. They can only RAISE the tier. Keep them deterministic; add your own in
# .council/config.yaml (tripwires: {name: regex}, tripwire_paths: {name: [globs]}).
TRIPWIRES = {
    "migration": r"\b(schema|database|db|data|sql)\s+migrat\w*|\bmigrations?\b|/migrations?/|\balter\s+table\b|\bbackfill\w*"
                 r"|\bmigrat\w*\s+(\w+\s+){0,3}(databases?|db|schemas?|data|tables?)\b",
    "auth/security": r"\bauth(n|z)?\b|\bauthenticat\w*|\bauthoriz\w*|\boauth\w*|\bsso\b|\bsecrets?\b|\bcredentials?\b"
                     r"|\bpermissions?\b|\bencrypt\w*|\bpasswords?\b|\bapi\s+keys?\b",
    "infra/deploy": r"\binfra(structure)?\b|\bterraform\b|\bk8s\b|\bkubernetes\b|\bhelm\b|\.github/workflows|\bdns\b"
                    r"|\bdeploy\w*\s+(\w+\s+){0,2}prod\w*|\bprod(uction)?\s+(deploy|rollout|config|database)\w*",
    "data loss/irreversible": r"\b(delete|drop|truncate|purge|wipe|destroy)\w*\s+(\w+\s+){0,3}"
                              r"(tables?|databases?|db|records?|rows?|buckets?|volumes?|backups?|customers?|users?|accounts?|data|prod\w*)\b"
                              r"|\brm\s+-rf\b|\birreversibl\w*|\bforce[- ]push\w*",
    "public API/contract": r"\bpublic\s+api\b|\bopenapi\b|\bbreaking\s+change\w*|\bapi\s+contract\b|\bsdk\s+release\b"
                           r"|\bdeprecat\w*\s+(\w+\s+){0,2}(endpoints?|apis?)\b",
    "external publication": r"\bpress\s+release\b|\bblog\s+post\b|\bcustomer[- ]facing\b|\bannounce\w*\b|\bnpm\s+publish\b"
                            r"|\bpublic\s+(launch|announcement|release)\b|\bpublish\w*\s+(\w+\s+){0,3}(publicly|externally|package|release|blog|site)\b",
    "architecture/strategy": r"\barchitectur\w*|\brfc\b|\bstrateg\w*|\bproposal\b|\bdesign\s+doc\b|\bplatform\s+choice\b"
                             r"|\bvendor\s+(choice|selection)\b|\broadmap\b",
}
TRIPWIRE_PATHS = {
    "migration": ["*/migrations/*", "migrations/*", "*.sql"],
    "auth/security": ["*/auth/*", "auth/*", "*secret*", "*.pem", "*credentials*"],
    "infra/deploy": ["infra/*", "terraform/*", "*.tf", ".github/workflows/*", "k8s/*", "helm/*", "Dockerfile*"],
}


# ----------------------------------------------------------------------------- helpers
def words(text) -> int:
    return len(str(text or "").split())


def read_text(path) -> str:
    """A text file as UTF-8 with \\n line ends, whatever the machine's locale says. A byte-order mark is
    tolerated (Windows PowerShell's `>` writes UTF-16, some editors put a mark before UTF-8) and a
    stray byte is replaced, so a file made on another machine loads instead of ending the run."""
    data = Path(path).read_bytes()
    if data[:2] in (b"\xff\xfe", b"\xfe\xff"):
        return data.decode("utf-16", errors="replace").replace("\r\n", "\n")
    return data.decode("utf-8-sig", errors="replace").replace("\r\n", "\n")


def utf8_console() -> None:
    """Print UTF-8 with \\n line ends whatever the system. On Windows a pipe defaults to a legacy code
    page that has no arrow, so `7→5→3→1` would end the command with an encoding error, and to \\r\\n,
    so the same command would print other bytes there than everywhere else. Never fatal."""
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(encoding="utf-8", errors="replace", newline="\n")
        except (AttributeError, ValueError, OSError):
            pass


def now():
    """The local time now, or the frozen time in COUNCIL_NOW (2026-11-13T12:00:00). Tests and the parity
    suite freeze the clock so that two runs of the same steps write the same ids, dates and ledgers."""
    import datetime as dt
    frozen = os.environ.get("COUNCIL_NOW")
    if frozen:
        try:
            return dt.datetime.strptime(frozen.strip(), "%Y-%m-%dT%H:%M:%S")
        except ValueError:
            pass
    return dt.datetime.now()


def today():
    return now().date()


def load_yaml(path: Path):
    import yaml  # PyYAML
    return yaml.safe_load(read_text(path))


def labels_for(n: int) -> list[str]:
    out, i = [], 0
    while len(out) < n:
        q, r = divmod(i, 26)
        out.append((chr(65 + q - 1) if q else "") + chr(65 + r))
        i += 1
    return out


def project_root(start: Path | None = None) -> Path:
    """Nearest ancestor holding .council/ or .git; else the current directory. Your own settings folder
    (~/.council) does not count: it must not turn the home directory into the project for every
    folder beneath it that has no .git yet."""
    p = (start or Path.cwd()).resolve()
    try:
        own = user_config_path().parent.resolve()
    except OSError:
        own = None
    for cand in (p, *p.parents):
        if (cand / ".git").exists() or ((cand / ".council").is_dir() and (cand / ".council").resolve() != own):
            return cand
    return p


def config(root: Path | None = None) -> dict:
    path = (root or project_root()) / ".council" / "config.yaml"
    try:
        data = load_yaml(path) if path.exists() else None
    except Exception:  # noqa: BLE001 - a broken optional config must not break the gate
        return {}
    return data if isinstance(data, dict) else {}       # a file whose top level is a list is not a config


def authors_of(card: dict) -> list[str]:
    a = card.get("authors", card.get("author"))
    if a is None:
        return []
    return [a] if isinstance(a, str) else list(a)


# ----------------------------------------------------------------------------- doctor
def doctor(root: Path | None = None, probe: bool = False) -> dict:
    """What can be called from here? Recommends a mode. Spends no tokens unless probe=True, which
    sends one tiny test prompt to each installed CLI (an installed CLI may still be logged out)."""
    notes_probe: list[str] = []
    cli = {name: bool(shutil.which(name)) for name in ("claude", "codex", "gemini")}
    claude_code = os.environ.get("CLAUDECODE") == "1"
    openrouter = bool(os.environ.get("OPENROUTER_API_KEY"))
    labs = {"Anthropic": claude_code or cli["claude"], "OpenAI": cli["codex"] or openrouter,
            "Google": cli["gemini"] or openrouter}
    lock_path = SKILL_DIR / "models" / "lock.yaml"
    lock = {"exists": lock_path.exists(), "age_days": None, "stale": None}
    if lock_path.exists():
        import datetime as dt
        try:
            data = load_yaml(lock_path)
            limit = (load_yaml(SKILL_DIR / "models" / "policy.yaml") or {}).get("data", {}).get("stale_after_days", 30)
            age = (today() - dt.date.fromisoformat(str(data["data"]["collected"]))).days
            lock.update(age_days=age, stale=age > limit, collected=str(data["data"]["collected"]))
        except Exception as err:  # noqa: BLE001
            lock["error"] = str(err)
    reachable = [lab for lab, ok in labs.items() if ok]
    if len(reachable) >= 2:
        mode = "crosslab"
    elif labs["Anthropic"]:
        mode = "fanout:claude"
    elif labs["OpenAI"]:
        mode = "fanout:openai"
    elif labs["Google"]:
        mode = "fanout:gemini"
    else:
        mode = "solo"
    notes = []
    missing = [lab for lab, ok in labs.items() if not ok]
    if mode == "crosslab" and missing:
        notes.append(f"{', '.join(missing)} not reachable: those seats become labelled stand-ins")
    if mode.startswith("fanout:") or mode == "solo":
        notes.append("only one lab reachable: elders come from the same family (no cross-lab check). "
                     "Install the codex / gemini CLIs or set OPENROUTER_API_KEY to get one")
    if lock.get("stale"):
        notes.append(f"model data is {lock['age_days']} days old: run `council refresh`")
    if not lock["exists"]:
        notes.append("models/lock.yaml missing: run `council refresh --offline <snapshot>`")
    fresh = "subagents" if claude_code else next((f"cli:{c}" for c in ("claude", "codex", "gemini") if cli[c]), "none")
    forced = os.environ.get("COUNCIL_SEATS_VIA")               # subagents | cli:claude | cli:codex | cli:gemini | none
    if forced:
        fresh = forced
    probes = {}
    if probe:
        import providers
        for name, prov in (("claude", "claude"), ("codex", "openai"), ("gemini", "gemini")):
            if cli[name]:
                res = providers.ask(prov, "Reply with exactly the two letters: OK", timeout=90)
                ok = bool(res.get("ok")) and "OK" in str(res.get("text", "")).upper() and "ERROR" not in str(res.get("text", "")).upper()
                probes[name] = "ok" if ok else f"FAILED: {' '.join(str(res.get('text') or res.get('error') or '').split())[:120]}"
                if not ok:
                    notes_probe.append(f"the {name} CLI is installed but did not answer a test prompt: {probes[name]}")
    if fresh == "none":
        notes.append("no way to start fresh contexts: seats will run in the chair's context (logged as a deviation)")
    notes += notes_probe
    import memory as shared_memory
    mem = shared_memory.settings(root)
    if not mem["ok"]:
        notes.append(mem["problem"])
    return {"claude_code": claude_code, "cli": cli, **({"probes": probes} if probe else {}), "openrouter_key": openrouter,
            "aa_key": bool(os.environ.get("AA_API_KEY")), "labs_reachable": labs, "fresh_contexts": fresh,
            "lock": lock, "recommended_mode": mode, "notes": notes, "python": sys.version.split()[0],
            "memory": {k: mem.get(k) for k in ("backend", "dir", "source", "ok")}}


# ----------------------------------------------------------------------------- gate / plan
def tripwires_hit(task: str, paths: list[str] | None = None, root: Path | None = None) -> list[str]:
    cfg = config(root)
    text_rules = {**TRIPWIRES, **(cfg.get("tripwires") or {})}
    path_rules = {**TRIPWIRE_PATHS, **(cfg.get("tripwire_paths") or {})}
    hits = [name for name, rx in text_rules.items() if re.search(rx, task or "", re.I)]
    for name, globs in path_rules.items():
        if any(fnmatch.fnmatch(p, g) for p in (paths or []) for g in globs) and name not in hits:
            hits.append(name)
    return hits


def schedule_for(start: int, loops: int) -> list[int]:
    """Candidates at the start, then survivors after each loop: cut 2 per loop, end at 1."""
    out, cur = [start], start
    for i in range(loops):
        remaining = loops - i - 1
        cur = 1 if remaining == 0 else max(remaining + 1, cur - 2)
        out.append(cur)
    return out


# ----------------------------------------------------------------------------- support members (settings)
# The researchers and the mapper are options, not fixtures. Every value below can be set by a flag,
# by `members:` in the project's .council/config.yaml, or by `members:` in your own
# ~/.council/config.yaml. A flag beats the project, the project beats your defaults.
RESEARCH_IDS = ("RA", "RB", "RC", "RD")
BEATS = [                                           # the default beats, in the order seats are filled
    ("Papers and primary research",
     "Peer-reviewed papers and preprints (arXiv, ACL Anthology, OpenReview, Semantic Scholar), benchmarks and "
     "their leaderboards, replications, surveys and standards. Find what was measured, on what data, and "
     "whether it held up when others tried it."),
    ("Practice in the field",
     "How teams really do it: engineering blogs and architecture write-ups from companies, postmortems, "
     "official documentation and changelogs, conference talks, and open-source repositories. Find what runs "
     "in production, what broke, and what it cost."),
    ("The case against",
     "How this goes wrong: incident reports and postmortems, critical reviews, negative results, replications "
     "that did not hold, security advisories, and the best arguments of people who chose otherwise. Find how "
     "often it fails, how badly, and what the failure cost."),
    ("The project's own record",
     "What this project already knows: its repository history, issues and pull requests, design notes, "
     "runbooks, the data and dashboards you can reach, and earlier council runs in the shared memory. Find "
     "what was tried here before and what happened."),
]
ONE_BEAT = ("Papers and practice",
            "Both the research literature (papers, benchmarks, replications) and how teams really do it "
            "(engineering posts, official documentation, postmortems, open-source repositories). Find what was "
            "measured and what runs in production, and where the two disagree.")
MEMBER_DEFAULTS = {
    "research": {"seats": 2, "budget": {"lite": 10, "full": 15}, "later_loops": "on_request", "model": "auto", "beats": []},
    "map": {"when": "stale", "model": "chair", "focus_tokens": 700, "skeleton_lines": 420, "path": ""},
    "digest": 14,
}
MEMBER_CHOICES = {"research.later_loops": ("on_request", "always", "never"), "research.model": ("auto", "chair"),
                  "map.when": ("stale", "always", "never"), "map.model": ("chair", "expert")}
MEMBER_RANGES = {"research.seats": (0, 4), "research.budget.lite": (1, 40), "research.budget.full": (1, 40),
                 "map.focus_tokens": (0, 4000), "map.skeleton_lines": (100, 2000), "digest": (0, 40)}


def user_config_path() -> Path:
    return Path(os.environ.get("COUNCIL_HOME") or Path.home() / ".council") / "config.yaml"


def _members_layer(raw) -> dict:
    """One layer's `members:` section as flat {dotted key: value}, shorthands spelled out."""
    out: dict = {}
    if not isinstance(raw, dict):
        return out
    r = raw.get("research")
    if isinstance(r, bool):
        out["research.seats"] = 2 if r else 0
    elif isinstance(r, int):
        out["research.seats"] = r
    elif isinstance(r, dict):
        for k in ("seats", "later_loops", "model", "beats"):
            if k in r:
                out[f"research.{k}"] = r[k]
        b = r.get("budget")
        if isinstance(b, int) and not isinstance(b, bool):
            out["research.budget.lite"] = out["research.budget.full"] = b
        elif isinstance(b, dict):
            out.update({f"research.budget.{k}": v for k, v in b.items() if k in ("lite", "full")})
    m = raw.get("map")
    if isinstance(m, bool):
        out["map.when"] = "stale" if m else "never"
    elif isinstance(m, str):
        out["map.when"] = m
    elif isinstance(m, dict):
        out.update({f"map.{k}": m[k] for k in ("when", "model", "focus_tokens", "skeleton_lines", "path") if k in m})
    if "digest" in raw:
        out["digest"] = raw["digest"]
    return out


def members(root: Path | None = None, research=None, mapper=None, research_budget: int | None = None,
            research_later: str | None = None, focus_tokens: int | None = None) -> dict:
    """The settings for the support members of a new run, each with where it came from.

    research: None (as configured), False or 0 (none), True, or how many researchers (1 to 4).
    mapper:   None (as configured), False ("never"), True, or "stale" | "always" | "never".
    Returns {"research": {...}, "map": {...}, "digest": n, "origin": {dotted key: who set it},
    "problems": [values that were refused, with the default used instead]}."""
    root = root or project_root()
    flat = {"research.seats": 2, "research.budget.lite": 10, "research.budget.full": 15,
            "research.later_loops": "on_request", "research.model": "auto", "research.beats": [],
            "map.when": "stale", "map.model": "chair", "map.focus_tokens": 700, "map.skeleton_lines": 420,
            "map.path": "", "digest": 14}
    origin = {k: "default" for k in flat}
    problems: list[str] = []
    try:
        user = (load_yaml(user_config_path()) or {}).get("members") if user_config_path().exists() else None
    except Exception:  # noqa: BLE001 - a broken optional file must not stop the gate
        user, problems = None, [f"{user_config_path()} is not valid YAML: your own defaults were ignored"]
    flags: dict = {}
    if research is not None:
        flags["research.seats"] = (2 if research else 0) if isinstance(research, bool) else research
    if mapper is not None:
        flags["map.when"] = ("stale" if mapper else "never") if isinstance(mapper, bool) else mapper
    if research_budget is not None:
        flags["research.budget.lite"] = flags["research.budget.full"] = research_budget
    if research_later is not None:
        flags["research.later_loops"] = research_later
    if focus_tokens is not None:
        flags["map.focus_tokens"] = focus_tokens
    legacy = (config(root).get("map") or {}).get("path")          # the older place for the map's path
    layers = (("your settings", _members_layer(user)),
              ("project config", {**({"map.path": legacy} if legacy else {}), **_members_layer(config(root).get("members"))}),
              ("flag", flags))
    for name, layer in layers:
        for key, value in layer.items():
            ok = True
            if key in MEMBER_CHOICES:
                ok = value in MEMBER_CHOICES[key]
            elif key in MEMBER_RANGES:
                lo, hi = MEMBER_RANGES[key]
                ok = isinstance(value, int) and not isinstance(value, bool) and lo <= value <= hi
            elif key == "research.beats":
                value = [{"name": " ".join(str(b.get("name") or "").split()), "text": " ".join(str(b.get("text") or "").split())}
                         for b in (value if isinstance(value, list) else []) if isinstance(b, dict)]
                ok = bool(value) and all(b["name"] and len(b["text"].split()) >= 5 for b in value)
            elif key == "map.path":
                ok = isinstance(value, str) and value.endswith(".md") and not Path(value).is_absolute() and ".." not in Path(value).parts
            if key not in flat or not ok:
                allowed = MEMBER_CHOICES.get(key) or MEMBER_RANGES.get(key) or "see `council config --example`"
                problems.append(f"members.{key} = {value!r} ({name}) was refused (allowed: {allowed}); using {flat.get(key)!r}")
                continue
            flat[key], origin[key] = value, name
    out: dict = {"research": {"budget": {}}, "map": {}}
    for key, value in flat.items():
        parts = key.split(".")
        node = out
        for part in parts[:-1]:
            node = node.setdefault(part, {})
        node[parts[-1]] = value
    out["origin"], out["problems"] = origin, problems
    return out


def beats_for(settings: dict) -> list[tuple[str, str]]:
    """One beat per researcher: the project's own beats first, then the defaults. A lone researcher
    covers papers and practice together."""
    n = settings["research"]["seats"]
    own = [(b["name"], b["text"]) for b in settings["research"].get("beats") or []]
    if n == 1 and not own:
        return [ONE_BEAT]
    return (own + [b for b in BEATS if b[0] not in {x[0] for x in own}])[:n]


def members_report(settings: dict, mem: dict, root: Path) -> list[str]:
    """`council config`: every setting, its value and who set it."""
    lines = ["# Support members (flags beat the project's .council/config.yaml, which beats your ~/.council/config.yaml)"]
    for key, who in settings["origin"].items():
        node = settings
        for part in key.split("."):
            node = node[part]
        shown = f"{len(node)} of your own" if key == "research.beats" and node else ("built in" if key == "research.beats" else node)
        if key == "map.path" and not node:
            import codemap
            shown = codemap.map_path(root).relative_to(root).as_posix()
        lines.append(f"members.{key} = {shown}    ({who})")
    who = {"user": "your settings", "project": "project config", "env": "environment variable"}.get(mem["source"], mem["source"])
    lines += ["beats this would seat: " + ("; ".join(b[0] for b in beats_for(settings)) or "none"),
              "", "# Shared memory (your own settings beat the project's here: a vault path is personal)",
              f"memory.backend = {mem['backend']}    ({who})", f"memory.dir = {mem['dir']}    ({who})"]
    if not mem.get("ok", True):
        lines.append(f"PROBLEM: {mem['problem']}")
    lines += [f"PROBLEM: {p}" for p in settings["problems"]]
    return lines


def estimate_calls(tier: str, loops: int, researchers: int = 0, mapper: bool = False) -> int:
    if tier not in SHAPES:
        return 1
    s = SHAPES[tier]
    vote = s["experts"] if tier == "full" else 0          # lite skips the shortlist vote
    per_loop = s["experts"] + vote + 1 + s["experts"] + s["elders"]   # ideate, vote, red team, rank, elders
    support = int(researchers) + (1 if mapper else 0)        # once, before the first ideation
    return per_loop * loops + 1 + support                    # + the control arm


def elders_verify(tier: str, profile: str | None) -> bool:
    return tier == "lite" or profile == "max"


def estimate_tokens(tier: str, loops: int, policy: dict | None = None, profile: str = "balanced",
                    researchers: int = 0, mapper: bool = False) -> int | None:
    """Rough tokens for the seat calls of a run, from MEASURED per-call numbers in models/policy.yaml
    (measured_tokens_per_call). None when there are no measurements. The chair's own tokens are extra.
    The researchers and the mapper are not measured yet: they come from assumed_tokens_per_call."""
    if tier not in SHAPES:
        return None
    if policy is None:
        try:
            policy = load_yaml(SKILL_DIR / "models" / "policy.yaml") or {}
        except Exception:  # noqa: BLE001
            return None
    m = policy.get("measured_tokens_per_call") or {}
    if not all(isinstance(m.get(k), (int, float)) for k in ("ideate", "vote", "rank", "redteam", "elders", "control")):
        return None
    s = SHAPES[tier]
    vote = s["experts"] * m["vote"] if tier == "full" else 0
    elder = m.get("elders_verify", m["elders"]) if elders_verify(tier, profile) else m["elders"]
    per_loop = s["experts"] * (m["ideate"] + m["rank"]) + vote + m["redteam"] + s["elders"] * elder
    guess = policy.get("assumed_tokens_per_call") or {}
    support = int(researchers) * (guess.get("research") or 0) + ((guess.get("map") or 0) if mapper else 0)
    return int(per_loop * loops + m["control"] + support)


def _short(n: int | None) -> str:
    return "n/a" if n is None else f"{n / 1e6:.1f}M" if n >= 1e6 else f"{n / 1e3:.0f}k"


def models_for(mode: str = "crosslab", profile: str = "balanced", lock_path: Path | None = None) -> dict:
    lock_path = lock_path or SKILL_DIR / "models" / "lock.yaml"
    if not lock_path.exists():
        return {"error": f"{lock_path} missing: run `council refresh`"}
    lock = load_yaml(lock_path)
    key = mode if mode in ("solo", "crosslab") or mode.startswith("fanout:") else f"fanout:{mode}"
    cfg = lock["configs"].get(profile, {}).get(key)
    if cfg is None:
        return {"error": f"no config for profile={profile} mode={key}"}
    return {"mode": key, "profile": profile, "data_collected": str(lock["data"].get("collected")),
            "attribution": lock["data"].get("attribution"), "warnings": lock.get("warnings", []), **cfg}


def plan(task: str, tier: str = "auto", mode: str | None = None, profile: str = "balanced",
         loops: int | None = None, paths: list[str] | None = None, stakes: str | None = None,
         reversible: str | None = None, override: str | None = None, research=None, mapper=None,
         research_budget: int | None = None, research_later: str | None = None, focus_tokens: int | None = None) -> dict:
    """Dry run of the gate. Spends no model tokens. Tripwires only raise the tier; only the requester's own
    words (`override`) can lower it. The chair records its judgment with stakes / reversible."""
    hits = tripwires_hit(task, paths)
    if tier in TIERS:
        chosen, reason = tier, "explicit"
    elif stakes == "high" or reversible == "no":
        chosen, reason = "full", "chair: " + ("stakes=high" if stakes == "high" else "not reversible")
    elif stakes == "low" and reversible != "no":
        chosen, reason = "direct", "chair: stakes=low, reversible"
    elif stakes == "medium":
        chosen, reason = "lite", "chair: stakes=medium"
    else:
        chosen, reason = "lite", "default: pass --stakes and --reversible to record your judgment"
    if hits and TIERS.index(chosen) < TIERS.index("full"):
        if override:
            reason = f'tripwire(s) {", ".join(hits)} waived by the requester: "{override}"'
        else:
            chosen, reason = "full", f"raised by tripwire(s): {', '.join(hits)}"
    elif override:
        reason = f'the requester: "{override}"'
    env = doctor()
    mode = mode or env["recommended_mode"]
    shape = dict(SHAPES.get(chosen, {}))
    n_loops = 0
    if shape:
        n_loops = loops or shape["loops"]
        shape["loops"] = n_loops
        shape["schedule"] = schedule_for(shape["start"], n_loops)
    picks = models_for(mode, profile)
    support = members(None, research, mapper, research_budget, research_later, focus_tokens)
    if not shape:                                            # the researchers and the mapper sit with a council only
        support["research"]["seats"], support["map"]["when"] = 0, "never"
    support["map"].update(due=False, reason="switched off")
    if support["map"]["when"] != "never":
        import codemap
        st = codemap.stale(project_root())
        always = support["map"]["when"] == "always" and st["applies"]
        support["map"].update(due=bool(always or (st["applies"] and st["stale"])),
                              reason="set to run every time" if always and not st["stale"] else st["reason"])
    n_res = support["research"]["seats"]
    calls = estimate_calls(chosen, n_loops, n_res, support["map"]["due"])
    tokens = estimate_tokens(chosen, n_loops, profile=profile, researchers=n_res, mapper=support["map"]["due"])
    gate = (f"GATE tier={chosen} mode={picks.get('mode', mode)} profile={profile} loops<={n_loops} "
            f"calls~{calls} tokens~{_short(tokens)} rel_cost={picks.get('relative_cost', 'n/a')} ({reason})")
    return {"gate": gate, "tier": chosen, "reason": reason, "tripwires": hits, "mode": picks.get("mode", mode),
            "profile": profile, "shape": shape, "calls_estimate": calls, "tokens_estimate": tokens, "models": picks,
            "support": support, "environment_notes": env["notes"]}


def members_line(support: dict, tier: str, memory_conf: dict | None = None) -> str:
    """One line under the GATE line: who else sits at the table, and where the shared memory is."""
    if tier not in SHAPES:
        return "MEMBERS none (direct tier: no council)"
    beats = beats_for(support)
    res = f"researchers={len(beats)}" + (f" ({'; '.join(b[0].lower() for b in beats)}; about "
                                         f"{support['research']['budget'][tier]} searches each)" if beats else " (off)")
    m = support["map"]
    mp = "mapper=off" if m["when"] == "never" else f"mapper={'yes' if m.get('due') else 'not needed'} ({m.get('reason')})"
    if memory_conf is None:
        import memory as shared_memory
        memory_conf = shared_memory.settings()
    where = memory_conf.get("folder") if memory_conf.get("backend") == "obsidian" else str(memory_conf.get("dir"))
    root = str(project_root())
    if where.startswith(root + os.sep):                          # inside the project: shown from its root, with forward
        where = where[len(root) + 1:].replace(os.sep, "/")       # slashes, so the line reads the same on every system
    return f"MEMBERS {res} {mp} memory={memory_conf.get('backend')}:{where}"


# ----------------------------------------------------------------------------- blind
# Identity leaks: a card that says who wrote it cannot be judged blind. Three shapes only, so that
# ordinary prose ("a seat call costs", "use the rubric as a lens") does not trip the check:
#   first person ("my lens", "our card"), a claimed role ("as the cost expert"), a real seat id ("seat L3").
SEAT_ID = r"(?-i:L\d{1,2}|RT|E[A-G]|CTRL)"
LEAK_RE = re.compile(r"\b(?:my|our)\s+(?:own\s+)?(?:lens|seat|cards?|ideas?|proposals?)\b"
                     r"|\bas\s+(?:the|an?)\s+(?:[\w-]+\s+){1,3}(?:expert|seat)\b"
                     rf"|\b(?:seat|expert|elder|voter|lens)\s+{SEAT_ID}(?![\w-])", re.I)


def blind(cards: list[dict], seed: int = 0, max_words: int = CAPS["card"], forbidden: list[str] | None = None,
          spread: float = 1.5) -> dict:
    """Anonymise and shuffle candidate cards. Returns the judge packet and a private mapping.
    Card: {"id", "title", "body", "eli5", "author" | "authors"}. Authors never reach the judges.
    `forbidden` = extra strings (seat ids, model names) that must not appear in a card.
    `spread` = how much longer the longest body may be than the shortest (long cards win votes)."""
    errors, warnings = [], []
    for c in cards:
        n = words(c.get("body"))
        if n > max_words * GRACE:
            errors.append(f"{c.get('id')}: body {n} words > cap {max_words}")
        elif n > max_words:
            warnings.append(f"{c.get('id')}: body {n} words (cap {max_words})")
        if not str(c.get("eli5") or "").strip():
            errors.append(f"{c.get('id')}: missing ELI5")
        text = f"{c.get('title', '')} {c.get('body', '')} {c.get('eli5', '')}"
        for token in [*authors_of(c), *(forbidden or [])]:
            if token and re.search(rf"(?<![\w-]){re.escape(str(token))}(?![\w-])", text):
                errors.append(f"{c.get('id')}: identity leak: the card names '{token}'. Reword it "
                              "(\"one expert\", \"another card\"): cards are judged blind")
        leak = LEAK_RE.search(text)
        if leak:
            errors.append(f"{c.get('id')}: identity leak: '{leak.group(0)}'. Reword it without saying who "
                          "wrote or holds what: cards are judged blind")
    order = list(range(len(cards)))
    random.Random(seed).shuffle(order)
    mapping, packet_cards = {}, []
    for label, idx in zip(labels_for(len(cards)), order):
        c = cards[idx]
        mapping[label] = {"id": c["id"], "authors": authors_of(c)}
        packet_cards.append({"label": label, "title": c.get("title", ""), "body": c.get("body", ""),
                             "eli5": c.get("eli5", "")})
    lengths = [words(c["body"]) for c in packet_cards]
    if lengths and max(lengths) > spread * max(1, min(lengths)):
        errors.append(f"length spread {min(lengths)}-{max(lengths)} words: equalise the cards (long ones win votes)")
    text = render_packet(packet_cards)
    return {"ok": not errors, "errors": errors, "warnings": warnings, "seed": seed, "packet": text,
            "cards": packet_cards, "mapping": mapping}


def seat_orders(labels: list[str], seats: list[str]) -> dict[str, list[str]]:
    """The order in which each voter reads the cards. Same labels for everyone; each voter starts at
    its own evenly spaced offset (voter index x cards // voters), so no card is first on every ballot
    and every card's mean position stays near the middle (LLM judges favour early positions)."""
    k, m = len(labels), max(1, len(seats))
    out = {}
    for i, seat in enumerate(sorted(seats)):
        off = (i * k) // m if k else 0
        out[seat] = labels[off:] + labels[:off]
    return out


def render_packet(cards: list[dict], order: list[str] | None = None) -> str:
    by = {c["label"]: c for c in cards}
    return "\n\n".join(f"### Candidate {l}: {by[l]['title']}\n{by[l]['body']}\nELI5: {by[l]['eli5']}"
                       for l in (order or list(by)))


# ----------------------------------------------------------------------------- tally + cut
LABEL_RE = re.compile(r"^(?:candidate|card|option)?\s*[:#-]?\s*([A-Za-z]{1,2})\s*[:.)-]?$", re.I)


def clean_label(raw, labels) -> str | None:
    """'Candidate B', 'b', 'B.' -> 'B'. None when it names no card."""
    s = str(raw if raw is not None else "").strip()
    if s in labels:
        return s
    m = LABEL_RE.match(s)
    return m.group(1).upper() if m and m.group(1).upper() in labels else None


def stake(voter: str, card: dict) -> float:
    """How much of a card is the voter's: 1 for a sole author, 1/n for one of n authors, else 0."""
    authors = authors_of(card)
    return 1 / len(authors) if voter in authors else 0.0


def tally(votes: list[dict], mapping: dict, scores: dict | None = None, checks: dict | None = None,
          keep: int | None = None, kills: list[str] | None = None, weights: dict | None = None) -> dict:
    """Count blind ballots as head-to-heads, weighted by stake. With complete ballots and no stakes
    this is a Borda count.
    votes:   [{"voter": "L3", "ranking": ["B", "A", ...], "best_on": {"R1": "B"}}]
    mapping: label -> {"id", "authors"}.
    A ballot is read as a set of head-to-heads ("this card above that card"). Each one counts with
    weight 1 - |stake in one card - stake in the other|: full weight when the voter wrote neither card
    or an equal share of both, nothing when the voter alone wrote one of them, part weight in between.
    So nobody can vote their own card up, and a card that several voters fed still gets judged.
    A card the voter left off the ballot is in none of that voter's head-to-heads: a gap is no opinion,
    never a last place.
    A card's score is its mean win share over every opponent. Each head-to-head starts as one win each
    ((wins + 1) / (weight + 2)), so a card that few voters could judge cannot win by default.
    `thin` lists head-to-heads that got less than one full vote: the vote could not judge them, so it
    must not be what cuts either card (see `protected`).
    checks:  label -> "pass" | "fail". A failed machine check ranks last: verify beats vote.
    kills:   labels the elders killed by majority; they rank last too.
    agreement: Kendall's tau between voters, pooled over the head-to-heads both judged (1 = same order,
    0 = unrelated, -1 = opposite). None when no two voters judged the same head-to-head."""
    labels = list(mapping)
    wins = {a: {b: 0.0 for b in labels if b != a} for a in labels}
    prefs: dict[str, dict] = {}
    problems, counted, recused, gaps = [], 0, 0, 0
    best_votes: dict[str, dict[str, int]] = {}
    for v in votes:
        v = v if isinstance(v, dict) else {}                  # a ballot of the wrong shape is an empty ballot, not a crash
        voter = str(v.get("voter"))
        ranking: list[str] = []
        for raw in v["ranking"] if isinstance(v.get("ranking"), list) else []:
            l = clean_label(raw, mapping)
            if l is None:
                problems.append(f"{voter}: unknown label {raw!r}")
            elif l in ranking:
                problems.append(f"{voter}: {l} ranked twice")
            else:
                ranking.append(l)
        mine = {l: stake(voter, mapping[l]) for l in labels}
        gaps += len(labels) - len(ranking)
        seen = prefs.setdefault(voter, {})
        for i, a in enumerate(ranking):
            for b in ranking[i + 1:]:
                w = 1 - abs(mine[a] - mine[b])
                if w <= 1e-9:                                 # the voter alone wrote one side: sit it out
                    recused += 1
                    continue
                wins[a][b] += w
                counted += 1
                seen[(a, b) if a < b else (b, a)] = (1 if a < b else -1, w)
        for crit, raw in (v["best_on"] if isinstance(v.get("best_on"), dict) else {}).items():
            l = clean_label(raw, mapping)
            if l and not mine[l]:
                best_votes.setdefault(crit, {}).setdefault(l, 0)
                best_votes[crit][l] += 1

    def weight(a, b):
        return wins[a][b] + wins[b][a]

    k, m = len(labels), len(votes)
    score = {a: (sum((wins[a][b] + 1) / (weight(a, b) + 2) for b in labels if b != a) / (k - 1) if k > 1 else 0.5)
             for a in labels}
    thin = [[a, b] for i, a in enumerate(labels) for b in labels[i + 1:] if weight(a, b) < 1 - 1e-9]
    failed = {l for l, r in (checks or {}).items() if str(r).lower() == "fail"}
    killed = set(kills or [])
    ranked = sorted(labels, key=lambda l: (l in failed or l in killed, -score[l], l))
    ties = [[a, b] for a, b in zip(ranked, ranked[1:]) if abs(score[a] - score[b]) < 1e-9
            and (a in failed or a in killed) == (b in failed or b in killed)]
    num = den = 0.0
    names = list(prefs)
    for i, u in enumerate(names):
        for w_ in names[i + 1:]:
            for key in prefs[u].keys() & prefs[w_].keys():
                both = min(prefs[u][key][1], prefs[w_][key][1])
                num += prefs[u][key][0] * prefs[w_][key][0] * both
                den += both
    agreement = round(num / den, 2) if den else None
    best_on = {}
    for crit, counts in best_votes.items():
        top = max(counts.values())
        leaders = [l for l, c in counts.items() if c == top]
        if len(leaders) == 1 and top >= max(2, math.ceil(0.4 * m)):
            best_on[crit] = leaders[0]
    result = {"ranking": [{"label": l, "id": mapping[l]["id"], "score": round(score[l], 3),
                           "check": (checks or {}).get(l, "n/a"), **({"killed": True} if l in killed else {})}
                          for l in ranked],
              "ballots": m, "head_to_heads": counted, "recused": recused, "gaps": gaps, "thin": thin,
              "ties": ties, "agreement": agreement,
              "agreement_note": ("not measurable: no two voters judged the same head-to-head" if agreement is None
                                 else "strong" if agreement >= 0.5 else "moderate" if agreement >= 0.2
                                 else "weak: treat close ranks as ties"),
              "best_on": best_on, "problems": problems}
    if keep is not None:
        result.update(cut(ranked, keep, scores or {}, failed | killed, best_on, weights))
    return result


def protected(order: list[str], keep: int, thin: list[list[str]], barred: set | None = None) -> list[str]:
    """Cards below a cut line that the vote could not judge against a card above it. A vote may never
    cut what it could not judge: these stay in (on the shortlist, in the elder packet, among the
    survivors) and the run logs a deviation. `barred` = cards that failed a check or were killed."""
    above, pairs = set(order[:keep]), {frozenset(p) for p in thin}
    return [c for c in order[keep:] if c not in (barred or set())
            and any(frozenset((c, a)) in pairs for a in above)]


def cut(ranked: list[str], keep: int, scores: dict | None = None, failed: set | None = None,
        best_on: dict | None = None, weights: dict | None = None) -> dict:
    """Keep the top `keep`. When keep >= 3, swap in ONE wildcard: a cut candidate that is the single
    best on some rubric criterion (keeps diversity alive, as in GEPA / MAP-Elites). The wildcard
    replaces the weakest survivor, never the top two. Failed or killed candidates never return.
    The final cut (keep < 3) is never touched: a wildcard must not replace the winner."""
    failed = failed or set()
    survivors, dropped = list(ranked[:keep]), list(ranked[keep:])
    wildcard = None
    if keep >= 3 and dropped:
        claims = dict(best_on or {})                            # criterion -> label (from voters)
        for crit in sorted({c for s in (scores or {}).values() for c in s}):
            vals = {l: s.get(crit) for l, s in scores.items() if s.get(crit) is not None}
            if vals:
                top = max(vals.values())
                leaders = [l for l, v in vals.items() if v == top]
                if len(leaders) == 1:
                    claims.setdefault(crit, leaders[0])
        order = sorted(claims, key=lambda c: (-(weights or {}).get(c, 0), c))
        for crit in order:
            label = claims[crit]
            if label in dropped and label not in failed:
                wildcard = {"label": label, "criterion": crit, "replaces": survivors[-1]}
                dropped.remove(label)
                dropped.append(survivors.pop())
                survivors.append(label)
                break
    return {"survivors": survivors, "eliminated": dropped, "wildcard": wildcard}


# ----------------------------------------------------------------------------- check a run
def check_run(run: dict) -> dict:
    """Validate a run ledger (run.json as a dict) against the protocol."""
    errs, warns = [], []
    tier = run.get("tier", "full")
    if tier == "direct":
        return {"ok": True, "errors": [], "warnings": []}
    shape = {**SHAPES.get(tier, SHAPES["full"]), **(run.get("shape") or {})}
    max_loops = run.get("max_loops", shape["loops"])
    loops = run.get("loops", [])
    if len(loops) > max_loops:
        errs.append(f"{len(loops)} loops > max {max_loops} (only the requester can raise the cap)")
    if not (run.get("ask") or run.get("original_ask")):
        errs.append("missing ask (elders must see the requester's words, not only the brief)")
    brief = run.get("brief") or {}
    if not brief and not run.get("intent_brief"):
        errs.append("missing intent brief")
    rubric = brief.get("rubric") or run.get("rubric")
    if not rubric:
        errs.append("missing rubric")
    schedule = run.get("schedule") or schedule_for(shape["start"], max_loops)
    for i, loop in enumerate(loops):
        tag = f"loop {i + 1}"
        for expert, ideas in (loop.get("ideas") or {}).items():
            if len(ideas) < shape["ideas_min"]:
                errs.append(f"{tag}: {expert} has {len(ideas)} ideas < {shape['ideas_min']}")
            long = [x for x in ideas if words(x) > CAPS["idea"] * GRACE]
            if long:
                warns.append(f"{tag}: {expert} has {len(long)} ideas over {CAPS['idea']} words")
        for card in loop.get("candidates", []):
            if not card.get("eli5"):
                errs.append(f"{tag}: candidate {card.get('id')} missing ELI5")
            if words(card.get("body")) > CAPS["card"] * GRACE:
                errs.append(f"{tag}: candidate {card.get('id')} over {CAPS['card']} words")
        done = "survivors" in loop
        if done and ("red_team" not in loop or "red_team_skipped" in loop):
            errs.append(f"{tag}: no red-team step recorded")
        if done and not loop.get("eli5"):
            errs.append(f"{tag}: ledger missing ELI5")
        open_s1 = [f for f in loop.get("red_team") or [] if f.get("severity") in ("S1", "S2")
                   and not f.get("response")]
        if done and open_s1:
            errs.append(f"{tag}: {len(open_s1)} S1/S2 finding(s) without a response")
        n_surv = len(loop.get("survivors", []))
        expected = schedule[min(i + 1, len(schedule) - 1)]
        if n_surv and n_surv != expected and not loop.get("stopped_early"):
            warns.append(f"{tag}: {n_surv} survivors, schedule expects {expected}")
        reviews = loop.get("elder_reviews", [])
        if done and len(reviews) < shape["elders"]:
            warns.append(f"{tag}: {len(reviews)} elder review(s), expected {shape['elders']}")
        standins = [r.get("seat") for r in reviews if r.get("stand_in_for")]
        if standins:
            warns.append(f"{tag}: stand-in elders {standins} (label them in the write-up)")
    first = loops[0] if loops else {}
    if any(v.get("role") == "researcher" for v in (run.get("seats") or {}).values()) and "ideas" in first:
        found = first.get("research") or {}
        if not [x for x in found.get("seats", []) if x != "MAP"]:
            warns.append("loop 1: no researcher reported (the council worked from the brief's facts only)")
    if (run.get("map") or {}).get("left_out") is not None:
        warns.append("the codebase map was due but the mapper was left out")
    if run.get("phase") == "done" and not run.get("retro"):
        warns.append("no retro / lessons recorded")
    for d in run.get("deviations", []):
        warns.append(f"deviation: {d}")
    return {"ok": not errs, "errors": errs, "warnings": warns}


# ----------------------------------------------------------------------------- locks
def _sha(path: Path) -> str:
    """Hash of a file with line ends normalised, so a lock taken on Windows (CRLF in the working copy)
    still matches the same file checked in CI on Linux (LF)."""
    return hashlib.sha256(path.read_bytes().replace(b"\r\n", b"\n")).hexdigest()


def _lock_path(root: Path | None = None) -> Path:
    return (root or project_root()) / ".council" / "locks.json"


def lock_files(paths: list[str], root: Path | None = None) -> dict:
    """Record sha256 for files (or every file under a directory). Paths are stored relative to the
    project root, so the lock means the same thing from any working directory and in CI."""
    root = (root or project_root()).resolve()
    lock = _lock_path(root)
    data = json.loads(lock.read_text(encoding="utf-8")) if lock.exists() else {}
    for p in paths:
        fp = (Path.cwd() / p).resolve() if not Path(p).is_absolute() else Path(p).resolve()
        files = sorted(x for x in fp.rglob("*") if x.is_file()) if fp.is_dir() else [fp]
        for f in files:
            data[f.relative_to(root).as_posix()] = _sha(f)
    lock.parent.mkdir(parents=True, exist_ok=True)
    lock.write_text(json.dumps(data, indent=1, sort_keys=True) + "\n", encoding="utf-8", newline="\n")
    return {"locked": len(data), "file": str(lock)}


def verify_locks(root: Path | None = None, against: str | None = None) -> dict:
    """Compare locked files with their recorded hashes.
    against=<git ref> reads the lock manifest from that ref (e.g. origin/main), so a change that
    edits a test AND its hash in the same PR is still caught. Use that form in CI."""
    root = (root or project_root()).resolve()
    note = None
    if against:
        proc = subprocess.run(["git", "-C", str(root), "show", f"{against}:.council/locks.json"],
                              capture_output=True, encoding="utf-8", errors="replace")
        if proc.returncode != 0:
            return {"ok": True, "changed": [], "note": f"no lock manifest at {against}"}
        data = json.loads(proc.stdout)
        note = f"manifest from {against}"
    else:
        lock = _lock_path(root)
        if not lock.exists():
            return {"ok": True, "changed": [], "note": "no locks"}
        data = json.loads(lock.read_text(encoding="utf-8"))
    changed = [p for p, h in data.items() if not (root / p).exists() or _sha(root / p) != h]
    return {"ok": not changed, "changed": changed, **({"note": note} if note else {})}


# ----------------------------------------------------------------------------- CLI
def _read_json(arg: str):
    try:
        is_file = Path(arg).exists()
    except OSError:                                           # a long JSON string is not a file name
        is_file = False
    return json.loads(Path(arg).read_text(encoding="utf-8")) if is_file else json.loads(arg)


def _add_gate_args(p):
    p.add_argument("--tier", default="auto", choices=("auto",) + TIERS)
    p.add_argument("--mode", help="solo | crosslab | fanout:<" + "|".join(FAMILIES) + ">  (default: what `doctor` recommends)")
    p.add_argument("--profile", default="balanced", choices=PROFILES)
    p.add_argument("--loops", type=int, help="loop cap (default 3 for full, 1 for lite). Only the requester raises it")
    p.add_argument("--stakes", choices=("low", "medium", "high"), help="the chair's judgment, recorded")
    p.add_argument("--reversible", choices=("yes", "no"), help="the chair's judgment, recorded")
    p.add_argument("--override", help="the requester's own words when they override the gate, e.g. \"lite is fine\"")
    p.add_argument("--paths", nargs="*", default=[], help="files the task will touch (path tripwires)")
    p.add_argument("--research", type=int, metavar="N", default=None,
                   help="how many researchers, 0 to 4 (default 2, or what the project's config says)")
    p.add_argument("--no-research", dest="research", action="store_const", const=0, help="same as --research 0")
    p.add_argument("--research-budget", type=int, metavar="N", help="searches or page reads per researcher (default 10 lite, 15 full)")
    p.add_argument("--research-later", choices=("on_request", "always", "never"),
                   help="research again before later loops: when a member asks (default), always, or never")
    p.add_argument("--map", dest="mapper", choices=("stale", "always", "never"), default=None,
                   help="when the codebase mapper runs: only when the map is missing or behind the code (default), always, never")
    p.add_argument("--no-map", dest="mapper", action="store_const", const="never", help="same as --map never")
    p.add_argument("--focus-tokens", type=int, metavar="N",
                   help="size of the ranked list of code that packets carry when the ask names code (default 700; 0 = none)")


def main(argv: list[str] | None = None) -> int:
    utf8_console()
    ap = argparse.ArgumentParser(prog="council", description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--version", "-V", action="version", version=f"council {VERSION}")
    sub = ap.add_subparsers(dest="cmd", required=True)

    p = sub.add_parser("doctor", help="what can be called from here; recommended mode")
    p.add_argument("--probe", action="store_true", help="send one tiny test prompt to each installed CLI")
    p = sub.add_parser("plan", help="dry-run the gate (no tokens spent)")
    p.add_argument("task")
    _add_gate_args(p)
    p = sub.add_parser("init", help="start a run (prints the GATE line)")
    p.add_argument("ask", help="the ask, verbatim, in the requester's own words")
    _add_gate_args(p)
    p.add_argument("--obsidian", action="store_true",
                   help="keep this run's shared memory in your Obsidian vault (set it once: memory setup --obsidian)")
    for name, text in (("next", "prepare the current step and say what to do"),
                       ("status", "one-screen digest of the run"),
                       ("dispatch", "run the CLI/API seats of the current step"),
                       ("ingest", "validate outputs of the current step; advance when valid")):
        p = sub.add_parser(name, help=text)
        p.add_argument("--run", help="run id (default: the current run)")
        if name == "ingest":
            p.add_argument("--drop", metavar="SEATS", help="go on without these seats (comma-separated), e.g. L3 or EB,EC")
            p.add_argument("--reason", help="why they are left out (logged as a deviation)")
        if name == "next":
            p.add_argument("--without-control", metavar="WHAT_HAPPENED",
                           help="the sealed control arm is gone for good: go on without it (logged)")
        if name == "dispatch":
            p.add_argument("--seat", help="only this seat")
            p.add_argument("--dry-run", action="store_true")
    p = sub.add_parser("audit", help="re-count every vote from the ballots on disk; exit 1 on a mismatch")
    p.add_argument("--run")
    p = sub.add_parser("deviation", help="record that the run left the protocol, and why")
    p.add_argument("text", help="one line: what was done differently, and why")
    p.add_argument("--run")
    p = sub.add_parser("cut", help="tally + elders + wildcard -> survivors; writes the loop ledger")
    p.add_argument("--run")
    p.add_argument("--eli5", required=True, help="one line: what happened in this loop")
    p.add_argument("--changed", default="", help="one line: what changed since the last loop")
    p.add_argument("--stop", metavar="REASON", help="stop early: converged or stalled (keeps 1)")
    p.add_argument("--asked", metavar="Q_AND_A", help='the one question put to the requester and the answer: "Q -> A"')
    p.add_argument("--unattended", metavar="SAFER_OPTION",
                   help="elders split or an S1 is open and the requester cannot be asked: the safer option you are taking")

    p = sub.add_parser("config", help="show every setting for the researchers, the mapper and the shared memory, and who set it")
    p.add_argument("--example", action="store_true", help="print a commented example config file instead")
    p = sub.add_parser("note", help="post to the run's commons: a thought for the members or a question for the researchers")
    p.add_argument("text")
    p.add_argument("--to", default="all", choices=("all", "chair", "research"))
    p.add_argument("--run")
    p = sub.add_parser("memory", help="the shared memory: research notes (facts with sources) and the commons")
    msub = p.add_subparsers(dest="mcmd", required=True)
    q = msub.add_parser("status", help="where the memory lives and what it holds")
    q = msub.add_parser("setup", help="choose where the memory lives (saved in YOUR settings unless --project)")
    q.add_argument("--obsidian", action="store_true", help="keep it in an Obsidian vault")
    q.add_argument("--vault", help="path of the vault folder")
    q.add_argument("--folder", help='folder inside the vault (default "Council Loop/<project>")')
    q.add_argument("--files", action="store_true", help="plain files (default: .council/memory in the project)")
    q.add_argument("--dir", dest="directory", help="folder for --files")
    q.add_argument("--project", action="store_true", help="save in the project's .council/config.yaml instead")
    q = msub.add_parser("add", help="add one fact by hand")
    q.add_argument("--claim", required=True)
    q.add_argument("--source", required=True, help="a web address, a file in the project, or 'test: <command>'")
    q.add_argument("--quote", default="")
    q.add_argument("--kind", default="other")
    q.add_argument("--confidence", default="medium", choices=("high", "medium", "low"))
    q.add_argument("--tags", default="", help="comma-separated")
    q.add_argument("--date", default="", help="when the source was published")
    q.add_argument("--why", default="")
    q.add_argument("--by", default="chair")
    q = msub.add_parser("search", help="find facts by keyword")
    q.add_argument("query")
    q.add_argument("--tag")
    q.add_argument("--limit", type=int, default=10)
    q.add_argument("--retired", action="store_true", help="also show facts that were retired")
    q = msub.add_parser("retire", help="mark a fact as no longer held (it is kept, with the reason, not deleted)")
    q.add_argument("note", help="the note's file name, without .md")
    q.add_argument("--because", required=True)
    q.add_argument("--by", help="the note that replaces it")
    q = msub.add_parser("import", help="bring research notes from another memory folder into this one")
    q.add_argument("--from", dest="src", required=True)
    p = sub.add_parser("map", help="the codebase map: write the skeleton, or check / validate / accept the map")
    p.add_argument("--check", action="store_true", help="exit 1 when the map is missing or behind the code (for CI)")
    p.add_argument("--validate", action="store_true", help="check the written map: sections, and every path it names")
    p.add_argument("--accept", action="store_true", help="record that the map describes the code as it is now")
    p.add_argument("--out", help="write the skeleton here (default .council/map/skeleton.md)")
    p.add_argument("--budget", type=int, default=420, help="skeleton length in lines")
    p.add_argument("--packet", action="store_true", help="write the mapper's packet, to refresh the map without a council run")
    p.add_argument("--install", metavar="FILE", help="validate a written map and put it in place (records what it was checked against)")
    p.add_argument("--focus", metavar="TEXT", help="print the code that matters most for a task described in TEXT (ranked, about 700 tokens)")
    p.add_argument("--parsers", action="store_true",
                   help="download tree-sitter parsers for this repository's languages (needs: pip install tree-sitter-language-pack)")

    p = sub.add_parser("models", help="show model picks")
    p.add_argument("--mode", default="crosslab")
    p.add_argument("--profile", default="balanced", choices=PROFILES)
    p = sub.add_parser("blind", help="anonymise + shuffle cards")
    p.add_argument("cards", help="JSON file or string: list of cards")
    p.add_argument("--seed", type=int, default=0)
    p.add_argument("--max-words", type=int, default=CAPS["card"])
    p.add_argument("--out", help="write packet markdown here; mapping goes to <out>.mapping.json")
    p = sub.add_parser("tally", help="count blind ballots (head-to-heads weighted by stake) + cut")
    p.add_argument("votes")
    p.add_argument("mapping")
    p.add_argument("--scores")
    p.add_argument("--checks")
    p.add_argument("--keep", type=int)
    p = sub.add_parser("check", help="validate a run ledger JSON")
    p.add_argument("run", nargs="?", help="path to run.json (default: the current run)")
    p = sub.add_parser("lock-tests", help="lock files/dirs agents may not edit")
    p.add_argument("paths", nargs="+")
    p = sub.add_parser("verify-locks", help="exit 1 if a locked file changed")
    p.add_argument("--against", metavar="GIT_REF", help="read the manifest from this ref (use in CI)")
    p = sub.add_parser("ask", help="one prompt to one provider")
    p.add_argument("--provider", required=True, choices=("claude", "openai", "gemini", "openrouter"))
    p.add_argument("--model")
    p.add_argument("--effort")
    p.add_argument("--prompt-file", required=True)
    p.add_argument("--dry-run", action="store_true")
    p = sub.add_parser("elders", help="blinded packet -> elder council (stand-alone use)")
    p.add_argument("--packet", required=True)
    p.add_argument("--profile", default="balanced", choices=PROFILES)
    p.add_argument("--providers", default="claude,openai,gemini")
    p.add_argument("--out", default=".council/elders")
    p.add_argument("--dry-run", action="store_true")
    p = sub.add_parser("refresh", help="rebuild models/lock.yaml from Artificial Analysis")
    p.add_argument("--offline", help="snapshot JSON instead of the live API")
    p.add_argument("--check", action="store_true", help="exit 1 when the model data is missing or stale")
    p.add_argument("--table", choices=["max", "balanced", "efficient"], help="print the table of picks for a profile")
    p.add_argument("--out", help="write the lock file here instead of the skill's models folder")

    if argv is None:                                          # typed as UTF-8 under another locale: read as UTF-8
        argv = [os.fsencode(x).decode("utf-8", errors="replace") if any(0xDC80 <= ord(c) <= 0xDCFF for c in x) else x
                for x in sys.argv[1:]]
    a = ap.parse_args(argv)
    if a.cmd == "doctor":
        out = doctor(probe=a.probe)
        print(f"recommended mode: {out['recommended_mode']} · fresh contexts via: {out['fresh_contexts']}")
        for n in out["notes"]:
            print("note:", n)
        print(json.dumps(out, indent=1))
        return 0
    if a.cmd == "plan":
        out = plan(a.task, a.tier, a.mode, a.profile, a.loops, a.paths, a.stakes, a.reversible, a.override,
                   a.research, a.mapper, a.research_budget, a.research_later, a.focus_tokens)
        print(out["gate"])
        print(members_line(out["support"], out["tier"]))
        print(json.dumps({k: v for k, v in out.items() if k != "gate"}, indent=1, default=str))
        return 0
    if a.cmd == "config":
        if a.example:
            print((SKILL_DIR / "templates" / "config.example.yaml").read_text(encoding="utf-8").rstrip())
            return 0
        import memory as shared_memory
        root = project_root()
        print("\n".join(members_report(members(root), shared_memory.settings(root), root)))
        return 0
    if a.cmd in ("memory", "map"):
        import codemap
        import memory as shared_memory
        try:
            if a.cmd == "map":
                if a.packet:
                    import run as runner
                    return runner.cmd_map_packet()
                return codemap.cmd_map(a.check, a.accept, a.validate, a.out, a.budget, a.install, a.parsers, a.focus)
            if a.mcmd == "status":
                return shared_memory.cmd_status()
            if a.mcmd == "setup":
                return shared_memory.cmd_setup(a.obsidian, a.vault, a.folder, a.files, a.directory, a.project)
            if a.mcmd == "add":
                return shared_memory.cmd_add(a.claim, a.source, a.quote, a.kind, a.confidence, a.tags, a.date, a.why, a.by)
            if a.mcmd == "search":
                return shared_memory.cmd_search(a.query, a.tag, a.limit, a.retired)
            if a.mcmd == "retire":
                return shared_memory.cmd_retire(a.note, a.because, a.by)
            return shared_memory.cmd_import(a.src)
        except shared_memory.MemoryError_ as err:
            print(f"error: {err}", file=sys.stderr)
            return 2
    if a.cmd in ("init", "next", "status", "dispatch", "ingest", "cut", "deviation", "audit", "note"):
        import run as runner
        try:
            if a.cmd == "init":
                return runner.cmd_init(a.ask, a.tier, a.mode, a.profile, a.loops, a.paths, a.stakes,
                                       a.reversible, a.override, a.research, a.mapper, a.obsidian,
                                       a.research_budget, a.research_later, a.focus_tokens)
            if a.cmd == "note":
                return runner.cmd_note(a.run, a.text, a.to)
            if a.cmd == "next":
                return runner.cmd_next(a.run, a.without_control)
            if a.cmd == "audit":
                return runner.cmd_audit(a.run)
            if a.cmd == "status":
                return runner.cmd_status(a.run)
            if a.cmd == "dispatch":
                return runner.cmd_dispatch(a.run, a.seat, a.dry_run)
            if a.cmd == "ingest":
                return runner.cmd_ingest(a.run, a.drop, a.reason)
            if a.cmd == "deviation":
                return runner.cmd_deviation(a.run, a.text)
            return runner.cmd_cut(a.run, a.eli5, a.changed, a.stop, a.asked, a.unattended)
        except runner.RunError as err:
            print(f"error: {err}", file=sys.stderr)
            return 2
    if a.cmd == "models":
        print(json.dumps(models_for(a.mode, a.profile), indent=1, default=str))
        return 0
    if a.cmd == "blind":
        out = blind(_read_json(a.cards), a.seed, a.max_words)
        if a.out:
            Path(a.out).write_text(out["packet"], encoding="utf-8", newline="\n")
            Path(a.out + ".mapping.json").write_text(json.dumps(out["mapping"], indent=1), encoding="utf-8", newline="\n")
            print(f"packet -> {a.out}; mapping (keep away from judges) -> {a.out}.mapping.json")
        else:
            print(json.dumps(out, indent=1))
        for e in out["errors"]:
            print("error:", e, file=sys.stderr)
        return 0 if out["ok"] else 2
    if a.cmd == "tally":
        out = tally(_read_json(a.votes), _read_json(a.mapping),
                    _read_json(a.scores) if a.scores else None,
                    _read_json(a.checks) if a.checks else None, a.keep)
        print(json.dumps(out, indent=1))
        return 0
    if a.cmd == "check":
        if a.run:
            data = _read_json(a.run)
        else:
            import run as runner
            data, _ = runner.load(None)
        out = check_run(data)
        print(json.dumps(out, indent=1))
        return 0 if out["ok"] else 1
    if a.cmd == "lock-tests":
        print(json.dumps(lock_files(a.paths)))
        return 0
    if a.cmd == "verify-locks":
        out = verify_locks(against=a.against)
        print(json.dumps(out))
        return 0 if out["ok"] else 1
    if a.cmd == "ask":
        import providers
        out = providers.ask(a.provider, Path(a.prompt_file).read_text(encoding="utf-8"), a.model, a.effort, dry_run=a.dry_run)
        print(json.dumps(out, indent=1))
        return 0 if out.get("ok") else 1
    if a.cmd == "elders":
        import providers
        out = providers.elder_review(Path(a.packet).read_text(encoding="utf-8"), a.profile,
                                     [p.strip() for p in a.providers.split(",") if p.strip()],
                                     Path(a.out), dry_run=a.dry_run)
        print(json.dumps(out, indent=1))
        return 0 if out.get("ok") else 1
    if a.cmd == "refresh":
        import refresh_models
        sys.argv = ["refresh_models.py", *(["--offline", a.offline] if a.offline else []), *(["--check"] if a.check else []),
                    *(["--table", a.table] if a.table else []), *(["--out", a.out] if a.out else [])]
        return refresh_models.main()
    return 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except BrokenPipeError:                                   # `council plan ... | head -2`: the reader left early
        try:
            sys.stdout.close()
        except Exception:  # noqa: BLE001
            pass
        os._exit(0)
