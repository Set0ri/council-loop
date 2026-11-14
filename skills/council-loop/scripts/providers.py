#!/usr/bin/env python3
"""providers - send prompts to other model families for the council (experts and elders).

ELI5: one small adapter per AI company. It calls the official command-line tools you already
use (claude, codex, gemini) or OpenRouter's API, in parallel, and collects the answers.

Model choice comes from models/lock.yaml. Native model IDs change often, so CLIs use their own
default model unless you pin one with an environment variable:
  COUNCIL_CLAUDE_MODEL   e.g. fable | opus | sonnet | haiku | full model id  (default: from lock)
  COUNCIL_OPENAI_MODEL   passed to `codex exec -m`        (default: Codex CLI default)
  COUNCIL_GEMINI_MODEL   passed to `gemini -m`            (default: Gemini CLI default)
  OPENROUTER_API_KEY     enables provider "openrouter"; model = lock openrouter_id or --model
Data leaving your machine: every provider here sends the prompt to that company. Use mode
`solo` or `fanout:<one family>` for code you may not share across vendors.
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

ELDER_INSTRUCTIONS = """You are an independent Elder Council reviewer. Review the blinded packet below.
Rules: judge only against the rubric; ignore length and style; no nitpicks; flag blockers as S1.
If unsure, say so. Do not try to guess which model or person wrote a candidate.
Return ONLY JSON:
{"scores": {"<label>": {"<criterion>": 1-10}}, "verdict": {"<label>": "ship|revise|kill"},
 "preferred": "<label>", "confidence": 0.0-1.0,
 "top_risks": [{"risk": "...", "severity": "S1|S2|S3"}], "key_fix": "...", "own_idea": "...",
 "eli5": "<=20 words"}
"""

CLAUDE_ALIASES = ("fable", "opus", "sonnet", "haiku")
LAB_OF_PROVIDER = {"claude": "Anthropic", "openai": "OpenAI", "gemini": "Google"}


def claude_alias(model_name: str | None) -> str | None:
    if not model_name:
        return None
    low = model_name.lower()
    return next((a for a in CLAUDE_ALIASES if a in low), None)


def _cmd(provider: str, prompt: str, model: str | None, out_file: Path) -> list[str]:
    if provider == "claude":
        m = os.environ.get("COUNCIL_CLAUDE_MODEL") or claude_alias(model) or "opus"
        return ["claude", "-p", prompt, "--model", m, "--output-format", "text"]
    if provider == "openai":
        m = os.environ.get("COUNCIL_OPENAI_MODEL")
        return ["codex", "exec", "--skip-git-repo-check", "-o", str(out_file), *(["-m", m] if m else []), prompt]
    if provider == "gemini":
        m = os.environ.get("COUNCIL_GEMINI_MODEL")
        return ["gemini", *(["-m", m] if m else []), "--output-format", "json", "-p", prompt]
    raise ValueError(provider)


def _openrouter(prompt: str, model: str, timeout: int) -> str:
    key = os.environ["OPENROUTER_API_KEY"]
    body = json.dumps({"model": model, "messages": [{"role": "user", "content": prompt}]}).encode()
    req = urllib.request.Request("https://openrouter.ai/api/v1/chat/completions", data=body,
                                 headers={"Authorization": f"Bearer {key}", "Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return json.load(resp)["choices"][0]["message"]["content"]


def ask(provider: str, prompt: str, model: str | None = None, timeout: int = 900,
        dry_run: bool = False) -> dict:
    """Send one prompt. Returns {"ok", "provider", "model", "text"|"error", "command"?}."""
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
        cmd = _cmd(provider, prompt, model, out_file)
        shown = [c if len(c) < 80 else c[:60] + "...<prompt>" for c in cmd]
        if dry_run:
            return {"ok": True, "provider": provider, "model": model, "dry_run": True, "command": shown}
        if not shutil.which(cmd[0]):
            return {"ok": False, "provider": provider, "error": f"CLI not found: {cmd[0]}", "command": shown}
        try:
            proc = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", timeout=timeout)
        except subprocess.TimeoutExpired:
            return {"ok": False, "provider": provider, "error": f"timeout after {timeout}s"}
        text = out_file.read_text(encoding="utf-8") if out_file.exists() else proc.stdout
        if provider == "gemini":
            try:
                text = json.loads(proc.stdout).get("response", proc.stdout)
            except json.JSONDecodeError:
                pass
        ok = proc.returncode == 0 and bool(text.strip())
        return {"ok": ok, "provider": provider, "model": model, "text": text,
                **({} if ok else {"error": proc.stderr[-2000:]})}


def parse_json(text: str) -> dict | None:
    if not text:
        return None
    decoder = json.JSONDecoder()
    for match in re.finditer(r"\{", text):
        try:
            obj, _ = decoder.raw_decode(text[match.start():])
            if isinstance(obj, dict):
                return obj
        except json.JSONDecodeError:
            continue
    return None


def elder_review(packet: str, profile: str = "balanced", providers: list[str] | None = None,
                 out_dir: Path = Path(".council/elders"), dry_run: bool = False, timeout: int = 900) -> dict:
    """Send the same blinded packet to each elder in parallel; summarise agreement and dissent."""
    import council  # local module: shares the lock reader
    providers = providers or ["claude", "openai", "gemini"]
    picks = council.models_for("crosslab", profile)
    elders = {e["model"]: e for e in picks.get("elders", [])} if "elders" in picks else {}

    def model_for(p: str) -> str | None:
        lab = LAB_OF_PROVIDER.get(p)
        return next((m for m in elders if lab and m.lower().startswith({"Anthropic": "claude",
                     "OpenAI": "gpt", "Google": "gemini"}[lab])), None)

    prompt = ELDER_INSTRUCTIONS + "\n\n" + packet
    out_dir.mkdir(parents=True, exist_ok=True)
    with cf.ThreadPoolExecutor(max_workers=len(providers)) as pool:
        futs = {p: pool.submit(ask, p, prompt, model_for(p), timeout, dry_run) for p in providers}
        results = {p: f.result() for p, f in futs.items()}
    reviews = {}
    for p, r in results.items():
        (out_dir / f"{p}.json").write_text(json.dumps(r, indent=1), encoding="utf-8")
        if r.get("ok") and not r.get("dry_run"):
            reviews[p] = parse_json(r.get("text", ""))
    preferred = [v.get("preferred") for v in reviews.values() if v and v.get("preferred")]
    s1 = [risk for v in reviews.values() if v for risk in v.get("top_risks", []) if risk.get("severity") == "S1"]
    split = len(set(preferred)) > 1 and len(preferred) >= 2
    missing = [p for p, r in results.items() if not r.get("ok")]
    return {"ok": not missing or dry_run, "dry_run": dry_run, "results": {p: {k: v for k, v in r.items()
            if k != "text"} for p, r in results.items()}, "preferred": preferred, "split": split,
            "s1_risks": s1, "missing": missing,
            "stand_in_note": ("Missing providers must be replaced with labelled stand-ins "
                              "(e.g. another strong model in a fresh context)") if missing else None,
            "next": ("Elders disagree or an S1 is unrebutted: stop and ask Logan ONE targeted question."
                     if (split or s1) else "Proceed: synthesise elder feedback into the next loop.")}
