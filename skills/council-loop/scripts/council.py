#!/usr/bin/env python3
"""council - the one enforcement path for the Council Loop (CLI + importable functions).

ELI5: the AI does the thinking; this script does the bookkeeping that should never be
left to an AI's good intentions: hiding who wrote what, shuffling, counting votes,
cutting 7 -> 5 -> 3 -> 1, checking the rules were followed, and locking test files.

Every surface (Claude Code skill, plugin hooks, MCP server, AGENTS.md for Codex/Gemini/Cursor,
CI) calls these same functions, so the rules are enforced the same way everywhere.

Commands
  plan          dry-run the gate: tier, mode, profile, models, cut schedule, checks (no tokens spent)
  models        show model picks for a mode/profile from models/lock.yaml
  blind         anonymise + shuffle candidate cards into a judge packet (seeded), enforce word caps
  tally         Borda count with self-vote exclusion; verifiers outrank votes; keep N + wildcard
  check         validate a run ledger (.council/runs/<id>/run.json) against the protocol
  lock-tests    record sha256 of files that agents may not edit (tests, migrations, ...)
  verify-locks  fail (exit 1) if a locked file changed - run this in CI, not only in hooks
  ask           send one prompt to a provider CLI/API (claude, openai, gemini, openrouter)
  elders        send one blinded packet to the elder council in parallel and summarise
  refresh       rebuild models/lock.yaml from Artificial Analysis data
Run `council.py <command> -h` for options. Python 3.10+, PyYAML.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import random
import re
import sys
from pathlib import Path

SKILL_DIR = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(Path(__file__).resolve().parent))

LOCK_FILE = Path(".council/locks.json")
DEFAULT_SCHEDULE = [7, 5, 3, 1]
TIERS = ("direct", "lite", "full")
PROFILES = ("max", "balanced", "efficient")
FAMILIES = ("claude", "gemini", "openai", "openrouter", "open", "mix")

# Tripwires can only RAISE the tier. Edit to match your codebase; keep them deterministic.
TRIPWIRES = {
    "migration": r"\bmigrat(e|ion)s?\b|/migrations?/|\.sql\b",
    "auth/security": r"\bauth(n|z|entication|orization)?\b|\bsecret|\bcredential|\bpermission|\bencrypt",
    "infra/deploy": r"\binfra\b|terraform|\bk8s\b|kubernetes|helm|\bdeploy|\.github/workflows",
    "delete/irreversible": r"\bdelete\b|\bdrop\b|\btruncate\b|\brm -rf\b|\bpurge\b|irreversible",
    "public API/contract": r"\bpublic api\b|openapi|\bschema change\b|breaking change|\bsdk\b",
    "external publication": r"\bpublish\b|\brelease\b|\bpress\b|\bannounce|\bblog post\b|\bcustomer[- ]facing",
    "architecture/strategy": r"\barchitecture\b|\brfc\b|\bstrategy\b|\bproposal\b|\bdesign doc\b|\bplatform choice",
}
TIER_FOR_TRIPWIRE = "full"


# ----------------------------------------------------------------------------- helpers
def _words(text: str) -> int:
    return len(re.findall(r"\S+", text or ""))


def _load_yaml(path: Path):
    import yaml  # PyYAML
    return yaml.safe_load(path.read_text())


def _labels(n: int) -> list[str]:
    out, i = [], 0
    while len(out) < n:
        q, r = divmod(i, 26)
        out.append((chr(65 + q - 1) if q else "") + chr(65 + r))
        i += 1
    return out


# ----------------------------------------------------------------------------- gate / plan
def tripwires_hit(task: str, paths: list[str] | None = None) -> list[str]:
    text = (task or "") + "\n" + "\n".join(paths or [])
    return [name for name, rx in TRIPWIRES.items() if re.search(rx, text, re.I)]


def models_for(mode: str = "crosslab", profile: str = "balanced", lock_path: Path | None = None) -> dict:
    lock_path = lock_path or SKILL_DIR / "models" / "lock.yaml"
    if not lock_path.exists():
        return {"error": f"{lock_path} missing: run `council.py refresh`"}
    lock = _load_yaml(lock_path)
    key = mode if mode in ("solo", "crosslab") else (mode if mode.startswith("fanout:") else f"fanout:{mode}")
    cfg = lock["configs"].get(profile, {}).get(key)
    if cfg is None:
        return {"error": f"no config for profile={profile} mode={key}"}
    return {"mode": key, "profile": profile, "data_collected": lock["data"].get("collected"),
            "attribution": lock["data"].get("attribution"), "warnings": lock.get("warnings", []), **cfg}


def plan(task: str, tier: str = "auto", mode: str = "crosslab", profile: str = "balanced",
         loops: int = 3, paths: list[str] | None = None) -> dict:
    """Dry run of the gate. Spends no model tokens. The chair still judges direct vs lite vs full
    for tasks with no tripwire; tripwires can only raise the tier, never lower it."""
    hits = tripwires_hit(task, paths)
    chosen = tier if tier in TIERS else "lite"
    reason = "explicit" if tier in TIERS else "default (chair may lower to direct for small, clear, reversible work)"
    if hits and TIERS.index(TIER_FOR_TRIPWIRE) > TIERS.index(chosen):
        chosen, reason = TIER_FOR_TRIPWIRE, f"raised by tripwire(s): {', '.join(hits)}"
    shape = {
        "direct": {"experts": 0, "ideas_each": 0, "loops": 0, "elders": 0,
                   "steps": ["write/lock acceptance tests", "implement", "verify loop <=5 iterations"]},
        "lite": {"experts": 3, "ideas_each": 5, "loops": 1, "elders": 1, "schedule": [3, 1],
                 "steps": ["intent brief + rubric", "3 lenses x 5 ideas", "blind vote", "red-team",
                           "1 elder review", "write-up", "build + verify loop"]},
        "full": {"experts": 7, "ideas_each": 13, "loops": min(loops, 3) if loops <= 3 else loops,
                 "elders": 3, "schedule": DEFAULT_SCHEDULE,
                 "steps": ["intent brief + rubric (Logan approves when present)",
                           "per loop: 7 lenses x >=13 ideas -> self-critique -> merge -> blind Borda vote -> "
                           "red-team pre-mortem -> refine -> fuse to 7 -> rank -> elders review top 2 (top 1 "
                           "in last loop) -> cut", "elder synthesis + plain-language write-up",
                           "build + verify loop", "retro: <=3 lessons via PR"]},
    }[chosen]
    picks = models_for(mode, profile) if chosen != "direct" else models_for(mode, profile)
    gate = (f"GATE tier={chosen} mode={picks.get('mode', mode)} profile={profile} "
            f"loops={shape.get('loops', 0)} rel_cost={picks.get('relative_cost', 'n/a')} ({reason})")
    return {"gate": gate, "tier": chosen, "reason": reason, "tripwires": hits, "shape": shape,
            "models": picks, "checks": ["verifiers outrank votes", "blind + shuffled + self-vote excluded",
                                        "word caps", "ELI5 on every artifact", "locked files unchanged"]}


# ----------------------------------------------------------------------------- blind
def blind(cards: list[dict], seed: int = 0, max_words: int = 120) -> dict:
    """Anonymise and shuffle candidate cards. Returns the judge packet and a private mapping.
    Each card: {"id", "author", "title", "body", "eli5"}. Authors never reach the judges."""
    errors = []
    for c in cards:
        n = _words(c.get("body", ""))
        if n > max_words:
            errors.append(f"{c.get('id')}: body {n} words > cap {max_words}")
        if not c.get("eli5"):
            errors.append(f"{c.get('id')}: missing ELI5")
    order = list(range(len(cards)))
    random.Random(seed).shuffle(order)
    labels = _labels(len(cards))
    mapping, packet_cards = {}, []
    for label, idx in zip(labels, order):
        c = cards[idx]
        mapping[label] = {"id": c["id"], "author": c.get("author")}
        packet_cards.append({"label": label, "title": c.get("title", ""), "body": c.get("body", ""),
                             "eli5": c.get("eli5", "")})
    lengths = [_words(c["body"]) for c in packet_cards]
    if lengths and max(lengths) > 1.5 * max(1, min(lengths)):
        errors.append(f"length spread {min(lengths)}-{max(lengths)} words: equalise to cut verbosity bias")
    text = "\n\n".join(f"### Candidate {c['label']}: {c['title']}\n{c['body']}\nELI5: {c['eli5']}"
                       for c in packet_cards)
    return {"ok": not errors, "errors": errors, "seed": seed, "packet": text,
            "cards": packet_cards, "mapping": mapping}


# ----------------------------------------------------------------------------- tally + cut
def tally(votes: list[dict], mapping: dict, scores: dict | None = None,
          checks: dict | None = None, keep: int | None = None) -> dict:
    """Borda count. votes: [{"voter": "E3", "ranking": ["B", "A", ...]}] using blinded labels.
    mapping: label -> {"id", "author"}. A voter's own candidates are removed from their ballot.
    scores (optional): label -> {criterion: number} rubric scores, used for the wildcard.
    checks (optional): label -> "pass" | "fail". Failing candidates rank last: verify beats vote."""
    labels = list(mapping)
    points = {l: 0.0 for l in labels}
    ballots = {l: 0 for l in labels}
    excluded = 0
    for v in votes:
        ranking = [l for l in v["ranking"] if l in mapping]
        own = [l for l in ranking if mapping[l].get("author") == v["voter"]]
        excluded += len(own)
        ranking = [l for l in ranking if l not in own]
        n = len(ranking)
        for pos, l in enumerate(ranking):
            points[l] += (n - 1 - pos) / max(1, n - 1)   # normalised 1.0 (top) .. 0.0 (bottom)
            ballots[l] += 1
    avg = {l: (points[l] / ballots[l] if ballots[l] else 0.0) for l in labels}
    failed = {l for l, r in (checks or {}).items() if str(r).lower() == "fail"}
    ranked = sorted(labels, key=lambda l: (l in failed, -avg[l], l))
    result = {"ranking": [{"label": l, "id": mapping[l]["id"], "borda": round(avg[l], 3),
                           "ballots": ballots[l], "check": (checks or {}).get(l, "n/a")} for l in ranked],
              "self_votes_excluded": excluded}
    if keep is not None:
        result.update(cut(ranked, keep, scores or {}, failed))
    return result


def cut(ranked: list[str], keep: int, scores: dict, failed: set | None = None) -> dict:
    """Keep the top `keep`, but swap in ONE wildcard: a cut candidate that is the unique best on
    some rubric criterion (Pareto diversity, as in GEPA / MAP-Elites). Failed checks never return."""
    failed = failed or set()
    survivors, dropped = list(ranked[:keep]), list(ranked[keep:])
    wildcard = None
    criteria = sorted({c for s in scores.values() for c in s})
    for crit in criteria:
        vals = {l: s.get(crit) for l, s in scores.items() if s.get(crit) is not None}
        if not vals:
            continue
        top = max(vals.values())
        leaders = [l for l, v in vals.items() if v == top]
        if len(leaders) == 1 and leaders[0] in dropped and leaders[0] not in failed and survivors:
            wildcard = {"label": leaders[0], "criterion": crit, "replaces": survivors[-1]}
            dropped.remove(leaders[0])
            dropped.append(survivors.pop())
            survivors.append(leaders[0])
            break
    return {"survivors": survivors, "eliminated": dropped, "wildcard": wildcard}


# ----------------------------------------------------------------------------- check a run
def check_run(run: dict) -> dict:
    """Validate a run ledger (dict) against the protocol. Returns errors and warnings."""
    errs, warns = [], []
    tier = run.get("tier", "full")
    min_ideas = {"full": 13, "lite": 5}.get(tier, 0)
    max_loops = run.get("max_loops", 3)
    loops = run.get("loops", [])
    if len(loops) > max_loops:
        errs.append(f"{len(loops)} loops > max {max_loops} (Logan must approve more)")
    for f in ("intent_brief", "rubric"):
        if tier != "direct" and not run.get(f):
            errs.append(f"missing {f}")
    if tier != "direct" and not run.get("original_ask"):
        errs.append("missing original_ask (elders must see Logan's words, not only the brief)")
    schedule = run.get("schedule") or (DEFAULT_SCHEDULE if tier == "full" else [3, 1])
    for i, loop in enumerate(loops):
        tag = f"loop {i + 1}"
        for expert, ideas in (loop.get("ideas") or {}).items():
            if len(ideas) < min_ideas:
                errs.append(f"{tag}: {expert} has {len(ideas)} ideas < {min_ideas}")
            long = [x for x in ideas if _words(x) > 25]
            if long:
                warns.append(f"{tag}: {expert} has {len(long)} ideas over 25 words")
        for card in loop.get("candidates", []):
            if not card.get("eli5"):
                errs.append(f"{tag}: candidate {card.get('id')} missing ELI5")
            if _words(card.get("body", "")) > 120:
                errs.append(f"{tag}: candidate {card.get('id')} over 120 words")
        if not loop.get("eli5"):
            errs.append(f"{tag}: ledger missing ELI5")
        if not loop.get("red_team"):
            errs.append(f"{tag}: no red-team findings recorded")
        n_surv = len(loop.get("survivors", []))
        expected = schedule[min(i + 1, len(schedule) - 1)]
        if n_surv and n_surv != expected:
            warns.append(f"{tag}: {n_surv} survivors, schedule expects {expected}")
        if tier == "full" and len(loop.get("elder_reviews", [])) < 3:
            warns.append(f"{tag}: fewer than 3 elder reviews (label any stand-ins)")
    if tier != "direct" and not run.get("retro"):
        warns.append("no retro / lessons recorded")
    return {"ok": not errs, "errors": errs, "warnings": warns}


# ----------------------------------------------------------------------------- locks
def _sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def lock_files(paths: list[str], lock_file: Path = LOCK_FILE) -> dict:
    data = json.loads(lock_file.read_text()) if lock_file.exists() else {}
    for p in paths:
        fp = Path(p)
        files = [x for x in fp.rglob("*") if x.is_file()] if fp.is_dir() else [fp]
        for f in files:
            data[str(f)] = _sha(f)
    lock_file.parent.mkdir(parents=True, exist_ok=True)
    lock_file.write_text(json.dumps(data, indent=1, sort_keys=True))
    return {"locked": len(data), "file": str(lock_file)}


def verify_locks(lock_file: Path = LOCK_FILE) -> dict:
    if not lock_file.exists():
        return {"ok": True, "changed": [], "note": "no locks"}
    data = json.loads(lock_file.read_text())
    changed = [p for p, h in data.items() if not Path(p).exists() or _sha(Path(p)) != h]
    return {"ok": not changed, "changed": changed}


def is_locked(path: str, lock_file: Path = LOCK_FILE) -> bool:
    if not lock_file.exists():
        return False
    data = json.loads(lock_file.read_text())
    try:
        rel = str(Path(path).resolve().relative_to(Path.cwd().resolve()))
    except ValueError:
        rel = path
    return rel in data or path in data


# ----------------------------------------------------------------------------- CLI
def _read_json(arg: str):
    return json.loads(Path(arg).read_text()) if Path(arg).exists() else json.loads(arg)


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(prog="council", description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)

    p = sub.add_parser("plan", help="dry-run the gate (no tokens spent)")
    p.add_argument("task")
    p.add_argument("--tier", default="auto", choices=("auto",) + TIERS)
    p.add_argument("--mode", default="crosslab", help="solo | crosslab | fanout:<" + "|".join(FAMILIES) + ">")
    p.add_argument("--profile", default="balanced", choices=PROFILES)
    p.add_argument("--loops", type=int, default=3)
    p.add_argument("--paths", nargs="*", default=[])

    p = sub.add_parser("models", help="show model picks")
    p.add_argument("--mode", default="crosslab")
    p.add_argument("--profile", default="balanced", choices=PROFILES)

    p = sub.add_parser("blind", help="anonymise + shuffle cards")
    p.add_argument("cards", help="JSON file or string: list of cards")
    p.add_argument("--seed", type=int, default=0)
    p.add_argument("--max-words", type=int, default=120)
    p.add_argument("--out", help="write packet markdown here; mapping goes to <out>.mapping.json")

    p = sub.add_parser("tally", help="Borda tally + cut")
    p.add_argument("votes")
    p.add_argument("mapping")
    p.add_argument("--scores")
    p.add_argument("--checks")
    p.add_argument("--keep", type=int)

    p = sub.add_parser("check", help="validate a run ledger JSON")
    p.add_argument("run")

    p = sub.add_parser("lock-tests", help="lock files/dirs agents may not edit")
    p.add_argument("paths", nargs="+")

    sub.add_parser("verify-locks", help="exit 1 if a locked file changed (use in CI)")

    p = sub.add_parser("ask", help="one prompt to one provider")
    p.add_argument("--provider", required=True, choices=("claude", "openai", "gemini", "openrouter"))
    p.add_argument("--model")
    p.add_argument("--prompt-file", required=True)
    p.add_argument("--dry-run", action="store_true")

    p = sub.add_parser("elders", help="blinded packet -> elder council")
    p.add_argument("--packet", required=True)
    p.add_argument("--profile", default="balanced", choices=PROFILES)
    p.add_argument("--providers", default="claude,openai,gemini")
    p.add_argument("--out", default=".council/elders")
    p.add_argument("--dry-run", action="store_true")

    p = sub.add_parser("refresh", help="rebuild models/lock.yaml from Artificial Analysis")
    p.add_argument("--offline")

    a = ap.parse_args(argv)
    if a.cmd == "plan":
        out = plan(a.task, a.tier, a.mode, a.profile, a.loops, a.paths)
        print(out["gate"])
        print(json.dumps({k: v for k, v in out.items() if k != "gate"}, indent=1, default=str))
        return 0
    if a.cmd == "models":
        print(json.dumps(models_for(a.mode, a.profile), indent=1, default=str))
        return 0
    if a.cmd == "blind":
        out = blind(_read_json(a.cards), a.seed, a.max_words)
        if a.out:
            Path(a.out).write_text(out["packet"])
            Path(a.out + ".mapping.json").write_text(json.dumps(out["mapping"], indent=1))
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
        out = check_run(_read_json(a.run))
        print(json.dumps(out, indent=1))
        return 0 if out["ok"] else 1
    if a.cmd == "lock-tests":
        print(json.dumps(lock_files(a.paths)))
        return 0
    if a.cmd == "verify-locks":
        out = verify_locks()
        print(json.dumps(out))
        return 0 if out["ok"] else 1
    if a.cmd == "ask":
        import providers
        out = providers.ask(a.provider, Path(a.prompt_file).read_text(), a.model, dry_run=a.dry_run)
        print(json.dumps(out, indent=1))
        return 0 if out.get("ok") else 1
    if a.cmd == "elders":
        import providers
        out = providers.elder_review(Path(a.packet).read_text(), a.profile,
                                     [p.strip() for p in a.providers.split(",") if p.strip()],
                                     Path(a.out), dry_run=a.dry_run)
        print(json.dumps(out, indent=1))
        return 0 if out.get("ok") else 1
    if a.cmd == "refresh":
        import refresh_models
        args = ["--offline", a.offline] if a.offline else []
        sys.argv = ["refresh_models.py", *args]
        return refresh_models.main()
    return 1


if __name__ == "__main__":
    sys.exit(main())
