#!/usr/bin/env python3
"""providers - give a seat a fresh context on any model family.

ELI5: one small adapter per AI company. It calls the official command-line tools you already
use (claude, codex, gemini) or OpenRouter's API, and returns the answer as text.

Which model actually runs
  The registry (models/lock.yaml) names models the way Artificial Analysis does, e.g.
  "GPT-6.1 Sol (high)". CLIs want their own ids. Resolution order for a seat:
    1. env override: COUNCIL_CLAUDE_MODEL / COUNCIL_OPENAI_MODEL / COUNCIL_GEMINI_MODEL
    2. `native_ids` in models/policy.yaml (you maintain this small map)
    3. Claude only: the family alias (fable | opus | sonnet | haiku)
    4. the CLI's own default model - reported as "cli default" so the ledger stays honest
  Effort ("(high)" in the name) is passed to `claude --effort`. The codex and gemini CLIs get no
  effort flag from here: set it in their own config if you need it.

Data leaving your machine: every provider here sends the prompt to that company. Use mode
`solo` or a single-family fan-out for code you may not share across vendors.
"""
from __future__ import annotations

import concurrent.futures as cf
import json
import os
import re
import shutil
import subprocess
import tempfile
import urllib.request
from pathlib import Path

CLAUDE_ALIASES = ("fable", "opus", "sonnet", "haiku")
EFFORTS = ("low", "medium", "high", "xhigh", "max")
EFFORT_RE = re.compile(r"\s*\((max|xhigh|high|medium|low|minimal)\)\s*$", re.I)
CREATOR_PROVIDER = {"Anthropic": "claude", "OpenAI": "openai", "Google": "gemini"}
GEMINI_NUDGE = "Follow the instructions above exactly. Reply with only the JSON they ask for."

ELDER_INSTRUCTIONS = """You are an independent Elder Council reviewer. Review the blinded packet below.
Rules: judge only against the rubric; ignore length and style; no nitpicks; flag blockers as S1.
If unsure, say so. Do not try to guess which model or person wrote a candidate.
Return ONLY JSON:
{"scores": {"<label>": {"<criterion>": 1-10}}, "verdict": {"<label>": "ship|revise|kill"},
 "preferred": "<label>", "confidence": 0.0-1.0,
 "top_risks": [{"risk": "...", "severity": "S1|S2|S3"}], "key_fix": "...", "own_idea": "...",
 "eli5": "<=20 words"}
"""


def base_name(model_name: str | None) -> str:
    return EFFORT_RE.sub("", model_name or "").strip()


def effort_of(model_name: str | None) -> str | None:
    m = EFFORT_RE.search(model_name or "")
    e = m.group(1).lower() if m else None
    return e if e in EFFORTS else None


def claude_alias(model_name: str | None) -> str | None:
    low = (model_name or "").lower()
    return next((a for a in CLAUDE_ALIASES if a in low), None)


def native_id(provider: str, model_name: str | None, policy: dict | None = None) -> tuple[str | None, str]:
    """Return (id to pass to the CLI, how it was chosen)."""
    env = os.environ.get(f"COUNCIL_{provider.upper()}_MODEL")
    if env:
        return env, "env override"
    ids = (policy or {}).get("native_ids") or {}
    if base_name(model_name) in ids:
        return ids[base_name(model_name)], "policy native_ids"
    if provider == "claude":
        alias = claude_alias(model_name)
        if alias:
            return alias, "family alias"
    return None, "cli default"


def _command(provider: str, model: str | None, effort: str | None, tools: str, out_file: Path) -> list[str]:
    if provider == "claude":
        cmd = ["claude", "-p", "--output-format", "text", "--no-session-persistence"]
        if model:
            cmd += ["--model", model]
        if effort in EFFORTS:
            cmd += ["--effort", effort]
        allowed = {"read": "Read,Grep,Glob", "research": "Read,Grep,Glob,WebSearch,WebFetch"}.get(tools)
        cmd += ["--allowedTools", allowed] if allowed else ["--tools", ""]
        return cmd                                             # prompt arrives on stdin
    if provider == "openai":
        return ["codex", "exec", "--skip-git-repo-check", "-o", str(out_file), *(["-m", model] if model else []), "-"]
    if provider == "gemini":
        return ["gemini", *(["-m", model] if model else []), "-o", "json", "-p", GEMINI_NUDGE]
    raise ValueError(provider)


def _openrouter(prompt: str, model: str, timeout: int) -> str:
    key = os.environ["OPENROUTER_API_KEY"]
    body = json.dumps({"model": model, "messages": [{"role": "user", "content": prompt}]}).encode()
    req = urllib.request.Request("https://openrouter.ai/api/v1/chat/completions", data=body,
                                 headers={"Authorization": f"Bearer {key}", "Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return json.load(resp)["choices"][0]["message"]["content"]


def ask(provider: str, prompt: str, model: str | None = None, effort: str | None = None,
        timeout: int = 900, dry_run: bool = False, tools: str = "none") -> dict:
    """Send one prompt in a fresh context. `model` is a native id/alias (see native_id) or None.
    tools: "none" (judges), "read" (experts may read project files) or "research" (also open web
    pages; only the claude CLI is given the web tools here: codex and gemini run with whatever web
    access their own settings allow). Returns
    {"ok", "provider", "model", "text" | "error", "command"?}."""
    if provider == "openrouter":
        if dry_run:
            return {"ok": True, "provider": provider, "model": model, "dry_run": True,
                    "command": "POST https://openrouter.ai/api/v1/chat/completions"}
        if not os.environ.get("OPENROUTER_API_KEY") or not model:
            return {"ok": False, "provider": provider, "error": "need OPENROUTER_API_KEY and a model slug"}
        try:
            return {"ok": True, "provider": provider, "model": model, "text": _openrouter(prompt, model, timeout)}
        except Exception as err:  # noqa: BLE001
            return {"ok": False, "provider": provider, "error": str(err)}
    with tempfile.TemporaryDirectory() as tmp:
        out_file = Path(tmp) / "last.txt"
        cmd = _command(provider, model, effort, tools, out_file)
        if dry_run:
            return {"ok": True, "provider": provider, "model": model or "cli default", "dry_run": True,
                    "command": cmd, "stdin": f"<prompt, {len(prompt)} chars>"}
        exe = shutil.which(cmd[0])
        if not exe:
            return {"ok": False, "provider": provider, "error": f"CLI not found: {cmd[0]}", "command": cmd}
        try:                                                # the resolved path: on Windows npm tools are .cmd files
            # bytes in and out: a text pipe would turn every \\n of the prompt into \\r\\n on Windows, and read
            # the reply in the machine's own encoding
            proc = subprocess.run([exe, *cmd[1:]], input=prompt.encode("utf-8"), capture_output=True, timeout=timeout)
        except subprocess.TimeoutExpired:
            return {"ok": False, "provider": provider, "error": f"timeout after {timeout}s"}
        stdout, stderr = _printed(proc.stdout), _printed(proc.stderr)
        text = out_file.read_text(encoding="utf-8", errors="replace") if out_file.exists() else stdout
        if provider == "gemini":
            try:
                text = json.loads(stdout).get("response", stdout)
            except json.JSONDecodeError:
                pass
        ok = proc.returncode == 0 and bool((text or "").strip())
        return {"ok": ok, "provider": provider, "model": model or "cli default", "text": text,
                **({} if ok else {"error": (stderr or "empty reply")[-2000:]})}


def _printed(data: bytes | None) -> str:
    """Text a tool printed: UTF-8 with stray bytes replaced, and \\n line ends."""
    return (data or b"").decode("utf-8", errors="replace").replace("\r\n", "\n").replace("\r", "\n")


def parse_json(text: str) -> dict | None:
    """First JSON object in a reply, tolerant of code fences and prose around it."""
    text = text or ""
    try:
        obj = json.loads(text)
        return obj if isinstance(obj, dict) else None
    except json.JSONDecodeError:
        pass
    start = text.find("{")
    while start != -1:
        depth, in_str, esc = 0, False, False
        for i in range(start, len(text)):
            ch = text[i]
            if in_str:
                esc = (ch == "\\") and not esc
                if ch == '"' and not esc:
                    in_str = False
                continue
            if ch == '"':
                in_str = True
            elif ch == "{":
                depth += 1
            elif ch == "}":
                depth -= 1
                if depth == 0:
                    try:
                        obj = json.loads(text[start:i + 1])
                        if isinstance(obj, dict):
                            return obj
                    except json.JSONDecodeError:
                        pass
                    break
        start = text.find("{", start + 1)
    return None


def elder_review(packet: str, profile: str = "balanced", providers: list[str] | None = None,
                 out_dir: Path = Path(".council/elders"), dry_run: bool = False, timeout: int = 900) -> dict:
    """Stand-alone elder review: the same blinded packet to each lab in parallel, then a summary.
    Inside a run, use `council next` / `dispatch` instead: they also blind, record and validate."""
    import council
    providers = providers or ["claude", "openai", "gemini"]
    picks = council.models_for("crosslab", profile)
    policy = council.load_yaml(council.SKILL_DIR / "models" / "policy.yaml") or {}
    by_provider = {CREATOR_PROVIDER.get(e.get("creator")): e["model"] for e in picks.get("elders", [])
                   if isinstance(e, dict)}
    prompt = ELDER_INSTRUCTIONS + "\n\n" + packet
    out_dir.mkdir(parents=True, exist_ok=True)

    def call(p: str) -> dict:
        name = by_provider.get(p)
        mid, how = native_id(p, name, policy)
        res = ask(p, prompt, mid, effort_of(name), timeout, dry_run)
        return {**res, "requested": name, "model_source": how}

    with cf.ThreadPoolExecutor(max_workers=len(providers)) as pool:
        results = dict(zip(providers, pool.map(call, providers)))
    reviews = {}
    for p, r in results.items():
        (out_dir / f"{p}.json").write_text(json.dumps(r, indent=1), encoding="utf-8", newline="\n")
        if r.get("ok") and not r.get("dry_run"):
            reviews[p] = parse_json(r.get("text", ""))
    preferred = [v.get("preferred") for v in reviews.values() if v and v.get("preferred")]
    s1 = [risk for v in reviews.values() if v for risk in v.get("top_risks", []) if risk.get("severity") == "S1"]
    split = len(set(preferred)) > 1
    missing = [p for p, r in results.items() if not r.get("ok")]
    return {"ok": not missing or dry_run, "dry_run": dry_run,
            "results": {p: {k: v for k, v in r.items() if k != "text"} for p, r in results.items()},
            "preferred": preferred, "split": split, "s1_risks": s1, "missing": missing,
            "stand_in_note": ("Replace missing providers with labelled stand-ins "
                              "(another strong model in a fresh context)") if missing else None,
            "next": ("Elders disagree or an S1 is open: stop and ask the requester ONE targeted question."
                     if (split or s1) else "Proceed: feed elder feedback into the next loop.")}
