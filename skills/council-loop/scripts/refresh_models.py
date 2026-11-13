#!/usr/bin/env python3
"""Refresh Council Loop model picks from Artificial Analysis data.

ELI5: reads the rules in models/policy.yaml, looks up today's model
scores and costs, and writes the best model for every role into lock.yaml.

Usage
  python3 scripts/refresh_models.py                       # live: needs AA_API_KEY
  python3 scripts/refresh_models.py --offline models/snapshot-2026-11-13.json
  python3 scripts/refresh_models.py --check               # exit 1 if lock.yaml is stale or missing
  python3 scripts/refresh_models.py --table balanced      # print a markdown table of picks
(paths are relative to skills/council-loop/)

Data: Artificial Analysis (https://artificialanalysis.ai). Attribution is required by their API terms.
Only the Python standard library + PyYAML are needed.
"""
from __future__ import annotations

import argparse
import datetime as dt
import json
import math
import os
import re
import sys
import urllib.request
from dataclasses import dataclass
from pathlib import Path

try:
    import yaml  # PyYAML
except ImportError:  # pragma: no cover
    sys.exit("PyYAML is required: pip install pyyaml")

ROOT = Path(__file__).resolve().parents[1]           # .../skills/council-loop
POLICY = ROOT / "models" / "policy.yaml"
LOCK = ROOT / "models" / "lock.yaml"
ROLES = ("chair", "experts", "red_team", "elders", "verifier")
EXPERT_SEATS = 7
EFFORT_RE = re.compile(r"\s*\((max|xhigh|high|medium|low|minimal)\)\s*$", re.I)


@dataclass(frozen=True)
class Model:
    name: str
    creator: str
    quality: float | None
    cost: float | None          # AA cost per task, USD
    minutes: float | None       # AA time per task, minutes
    open_weights: bool = False
    restricted: bool = False
    openrouter_id: str | None = None

    @property
    def base(self) -> str:
        return EFFORT_RE.sub("", self.name).strip()

    def card(self) -> dict:
        return {"model": self.name, "quality": self.quality, "cost_per_task": self.cost}


# ---------------------------------------------------------------- data loading
def _dig(obj, *path, default=None):
    for key in path:
        if not isinstance(obj, dict) or key not in obj:
            return default
        obj = obj[key]
    return obj


def load_live(policy: dict) -> tuple[list[Model], dict]:
    key = os.environ.get("AA_API_KEY")
    if not key:
        sys.exit("Set AA_API_KEY (free key from artificialanalysis.ai) or pass --offline SNAPSHOT.json")
    base = policy["data"]["api_url"].rstrip("/")
    payload, last_err = None, None
    for url in (base, base + "/free"):            # free-tier sibling path, per AA's OpenAPI spec
        try:
            req = urllib.request.Request(url, headers={"x-api-key": key, "accept": "application/json"})
            with urllib.request.urlopen(req, timeout=60) as resp:
                payload = json.load(resp)
            break
        except Exception as err:  # noqa: BLE001 - report and try the next path
            last_err = err
    if payload is None:
        sys.exit(f"Artificial Analysis request failed: {last_err}")
    models = []
    for row in payload.get("data", []):
        cost = _dig(row, "artificial_analysis_intelligence_index_cost", "cost_per_task", "total_cost")
        if cost is None:  # fall back to blended price: still comparable within one refresh
            cost = _dig(row, "pricing", "price_1m_blended_3_to_1")
        secs = _dig(row, "performance_v3", "time_per_task_seconds")
        models.append(Model(
            name=row.get("name") or row.get("slug", "?"),
            creator=_dig(row, "model_creator", "name", default="?"),
            quality=_dig(row, "evaluations", "artificial_analysis_intelligence_index"),
            cost=cost,
            minutes=(secs / 60.0) if secs else None,
            open_weights=bool(_dig(row, "licensing", "is_open_weights", default=False)),
            restricted=bool(row.get("trusted_access", False)),
            openrouter_id=row.get("openrouter_api_id"),
        ))
    today = dt.date.today().isoformat()
    meta = {"source": "Artificial Analysis API (artificialanalysis.ai)", "collected": today,
            "index": "AA Intelligence Index (live)"}
    snap = ROOT / "models" / f"snapshot-{today}.json"
    snap.write_text(json.dumps({**meta, "models": [m.__dict__ for m in models]}, indent=1), encoding="utf-8")
    return models, meta


def load_offline(path: Path) -> tuple[list[Model], dict]:
    raw = json.loads(path.read_text(encoding="utf-8"))
    models = [Model(name=r["name"], creator=r["creator"], quality=r.get("quality"),
                    cost=r.get("cost_per_task"), minutes=r.get("minutes"),
                    open_weights=r.get("open_weights", False), restricted=r.get("restricted", False),
                    openrouter_id=r.get("openrouter_id")) for r in raw["models"]]
    meta = {k: raw.get(k) for k in ("source", "collected", "index")}
    return models, meta


# ---------------------------------------------------------------- selection
def usable(models: list[Model], policy: dict) -> list[Model]:
    access = policy.get("access") or {}
    bans = policy.get("bans") or []
    out = []
    for m in models:
        if m.quality is None or any(m.name.startswith(b) for b in bans):
            continue
        if m.restricted and not any(m.name.startswith(k) and v.get("has_access") for k, v in access.items()):
            continue
        out.append(m)
    return out


def in_family(models: list[Model], fam: dict, warnings: list[str], name: str) -> list[Model]:
    if fam.get("open_weights_only"):
        return [m for m in models if m.open_weights]
    if fam.get("requires") == "openrouter_id":
        known = [m for m in models if m.openrouter_id]
        if known:
            return known
        warnings.append(f"{name}: OpenRouter availability unknown in this snapshot; using all usable models. "
                        "Run a live refresh to filter by openrouter_api_id.")
        return list(models)
    creators = fam.get("creators", "any")
    return list(models) if creators == "any" else [m for m in models if m.creator in creators]


def score(m: Model, k: float) -> float | None:
    if m.cost is None or m.cost <= 0:
        return None
    return m.quality - k * math.log2(m.cost)


def best(pool: list[Model], k: float, tie: float) -> Model | None:
    scored = [(score(m, k), m) for m in pool]
    scored = [(s, m) for s, m in scored if s is not None]
    if not scored:
        return None
    top = max(s for s, _ in scored)
    tied = [m for s, m in scored if top - s <= tie]
    tied.sort(key=lambda m: (m.minutes if m.minutes is not None else 1e9, -m.quality))
    return tied[0]


def pick_config(cands: list[Model], all_usable: list[Model], prof: dict, policy: dict,
                crosslab: bool) -> dict:
    tie = policy["tie_break"]["within_points"]
    out: dict = {}

    # chair
    c = prof["chair"]
    pool = cands
    if "floor_vs_best" in c:
        top_q = max(m.quality for m in cands)
        pool = [m for m in cands if m.quality >= c["floor_vs_best"] * top_q]
    chair = best(pool, c["k"], tie)
    if chair is None:
        return {"error": "no model with cost data in this family"}
    out["chair"] = chair

    # experts: one primary workhorse + diversity seats
    e = prof["experts"]
    pool = [m for m in cands if m.quality >= e["floor"] * chair.quality]
    primary = best(pool, e["k"], tie) or chair
    creators = {m.creator for m in pool}
    group = (lambda m: m.creator) if len(creators) > 1 else (lambda m: m.base)
    seats: list[Model] = []
    if crosslab:
        # cross-lab mode exists for decorrelated errors: one seat per other lab that clears the
        # quality floor, even if it is pricier than the workhorse (no score margin).
        for lab in policy["crosslab_labs"]:
            if lab != primary.creator:
                pick = best([m for m in pool if m.creator == lab], e["k"], tie)
                if pick:
                    seats.append(pick)
    else:
        for g in sorted({group(m) for m in pool} - {group(primary)}):
            # a seat must score within `diversity_margin` points of the main workhorse
            floor_s = (score(primary, e["k"]) or 0) - e.get("diversity_margin", 12)
            ok = [m for m in pool if group(m) == g and (score(m, e["k"]) or -1e9) >= floor_s]
            pick = best(ok, e["k"], tie)
            if pick:
                seats.append(pick)
        seats.sort(key=lambda m: score(m, e["k"]) or -1e9, reverse=True)
        seats = seats[: e.get("diversity_seats", 0)]
    out["experts"] = [primary] * (EXPERT_SEATS - len(seats)) + seats

    # red team: another family when possible, else another model
    r = prof["red_team"]
    floor_q = r.get("floor", 0) * chair.quality
    tiers = [[m for m in cands if m.creator != chair.creator]] if r.get("other_family") else []
    tiers += [[m for m in cands if m.base != chair.base], [m for m in cands if m.base == chair.base]]
    red = None
    for pool in tiers:          # other lab > other model > same model in a fresh context
        red = best([m for m in pool if m.quality >= floor_q], r["k"], tie)
        if red:
            break
    out["red_team"] = red or chair

    # elders
    el = prof["elders"]
    elders: list[Model] = []
    if crosslab:
        for lab in policy["crosslab_labs"]:
            lab_pool = [m for m in all_usable if m.creator == lab]
            if not lab_pool:
                continue
            top_q = max(m.quality for m in lab_pool)
            pick = best([m for m in lab_pool if m.quality >= el["floor"] * top_q], el["k"], tie)
            if pick:
                elders.append(pick)
    else:
        top_q = max(m.quality for m in cands)
        pool = [m for m in cands if m.quality >= el["floor"] * top_q]
        per_base = {}
        for m in pool:
            s = score(m, el["k"])
            if s is not None and (m.base not in per_base or s > score(per_base[m.base], el["k"])):
                per_base[m.base] = m
        elders = sorted(per_base.values(), key=lambda m: score(m, el["k"]), reverse=True)[:3]
        while elders and len(elders) < 3:            # fresh contexts of the best model
            elders.append(elders[0])
    out["elders"] = elders

    # verifier: fast and good enough
    v = prof["verifier"]
    fast = [m for m in cands if m.minutes is not None and m.minutes <= v["max_minutes"]
            and m.quality >= v.get("min_quality", 0)]
    out["verifier"] = best(fast, v["k"], tie) or best(
        [m for m in cands if m.quality >= v.get("min_quality", 0)], v["k"], tie) or chair
    return out


def relative_cost(cfg: dict, cm: dict) -> float:
    mean = lambda xs: sum(x.cost for x in xs) / len(xs)  # noqa: E731
    per_loop = (EXPERT_SEATS * cm["expert_passes_per_loop"] * mean(cfg["experts"])
                + cm["red_team_passes_per_loop"] * cfg["red_team"].cost
                + len(cfg["elders"]) * cm["elder_passes_per_loop"] * mean(cfg["elders"])
                + cm["chair_passes_per_loop"] * cfg["chair"].cost)
    return cm["loops"] * per_loop + cm["final_chair_passes"] * cfg["chair"].cost


def resolve(models: list[Model], policy: dict) -> tuple[dict, list[str]]:
    warnings: list[str] = []
    ok = usable(models, policy)
    configs: dict = {}
    for pname, prof in policy["profiles"].items():
        configs[pname] = {}
        for fname, fam in policy["families"].items():
            cands = in_family(ok, fam, warnings, f"fanout:{fname}")
            if not cands:
                configs[pname][f"fanout:{fname}"] = {"error": "no usable models"}
                continue
            configs[pname][f"fanout:{fname}"] = pick_config(cands, ok, prof, policy, crosslab=False)
        configs[pname]["crosslab"] = pick_config(
            in_family(ok, policy["families"]["mix"], warnings, "crosslab"), ok, prof, policy, crosslab=True)
    # relative cost, normalised so balanced/crosslab = 1.00
    ref = relative_cost(configs["balanced"]["crosslab"], policy["cost_model"])
    for pname in configs:
        for key, cfg in configs[pname].items():
            if "error" not in cfg:
                cfg["relative_cost"] = round(relative_cost(cfg, policy["cost_model"]) / ref, 2)
    for pname, efforts in policy["solo_efforts"].items():
        configs[pname]["solo"] = {role: f"current model @ {effort}" for role, effort in efforts.items()}
    return configs, sorted(set(warnings))


# ---------------------------------------------------------------- output
def to_plain(configs: dict) -> dict:
    def conv(v):
        if isinstance(v, Model):
            return v.card()
        if isinstance(v, list):
            return [conv(x) for x in v]
        if isinstance(v, dict):
            return {k: conv(x) for k, x in v.items()}
        return v
    return conv(configs)


def flat_picks(lock: dict) -> dict:
    flat = {}
    for p, fams in (lock.get("configs") or {}).items():
        for f, cfg in fams.items():
            for role in ROLES:
                val = cfg.get(role) if isinstance(cfg, dict) else None
                if isinstance(val, list):
                    flat[f"{p}/{f}/{role}"] = ", ".join(sorted({x["model"] for x in val}))
                elif isinstance(val, dict):
                    flat[f"{p}/{f}/{role}"] = val["model"]
                elif isinstance(val, str):
                    flat[f"{p}/{f}/{role}"] = val
    return flat


def apply_pins(lock: dict, pins: dict, models: list[Model]) -> None:
    by_name = {m.name: m for m in models}
    for path, name in (pins or {}).items():
        p, f, role, *rest = path.split("/")
        cfg = lock["configs"].get(p, {}).get(f)
        if cfg is None or name not in by_name:
            print(f"warning: pin {path} -> {name} not applied", file=sys.stderr)
            continue
        card = by_name[name].card()
        if role in ("experts", "elders") and rest:          # e.g. .../elders/Google
            cfg[role] = [card if (x["model"].startswith(rest[0]) or by_name.get(x["model"], None)
                                  and by_name[x["model"]].creator == rest[0]) else x for x in cfg[role]]
        elif role in ("experts", "elders"):
            cfg[role] = [card] * len(cfg[role])
        else:
            cfg[role] = card


def markdown_table(lock: dict, profile: str) -> str:
    rows = ["| Mode | Chair | Experts (7 seats) | Red team | Elders | Rel. cost |",
            "| --- | --- | --- | --- | --- | --- |"]
    for key, cfg in lock["configs"][profile].items():
        if "error" in cfg:
            rows.append(f"| {key} | {cfg['error']} | | | | |")
            continue
        if key == "solo":
            rows.append(f"| solo | {cfg['chair']} | {cfg['experts']} | {cfg['red_team']} | {cfg['elders']} | n/a |")
            continue
        counts: dict[str, int] = {}
        for x in cfg["experts"]:
            counts[x["model"]] = counts.get(x["model"], 0) + 1
        experts = " + ".join(f"{n}x {m}" for m, n in counts.items())
        elders = ", ".join(x["model"] for x in cfg["elders"])
        rows.append(f"| {key} | {cfg['chair']['model']} | {experts} | {cfg['red_team']['model']} | "
                    f"{elders} | {cfg['relative_cost']} |")
    return "\n".join(rows)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--offline", type=Path, help="snapshot JSON instead of the live AA API")
    ap.add_argument("--policy", type=Path, default=POLICY)
    ap.add_argument("--out", type=Path, default=LOCK)
    ap.add_argument("--check", action="store_true", help="exit 1 if the lock is missing or stale")
    ap.add_argument("--table", choices=["max", "balanced", "efficient"], help="print a markdown table")
    args = ap.parse_args()
    policy = yaml.safe_load(args.policy.read_text(encoding="utf-8"))

    if args.check:
        if not args.out.exists():
            print("lock.yaml missing: run refresh_models.py"); return 1
        lock = yaml.safe_load(args.out.read_text(encoding="utf-8"))
        age = (dt.date.today() - dt.date.fromisoformat(str(lock["data"]["collected"]))).days
        limit = policy["data"]["stale_after_days"]
        print(f"model data is {age} days old (limit {limit})")
        return 1 if age > limit else 0

    models, meta = load_offline(args.offline) if args.offline else load_live(policy)
    configs, warnings = resolve(models, policy)
    lock = {"generated": dt.date.today().isoformat(),
            "data": {**meta, "attribution": "Model data: Artificial Analysis (https://artificialanalysis.ai)"},
            "policy_version": policy["version"], "warnings": warnings, "configs": to_plain(configs)}
    apply_pins(lock, policy.get("pins"), models)

    old = flat_picks(yaml.safe_load(args.out.read_text(encoding="utf-8"))) if args.out.exists() else {}
    new = flat_picks(lock)
    changes = [f"  {k}: {old.get(k, '(none)')} -> {v}" for k, v in sorted(new.items()) if old.get(k) != v]
    header = ("# GENERATED by scripts/refresh_models.py from models/policy.yaml (k values and floors are\n"
              "# starting defaults: tune them against your golden set, see reference/model-modes.md).\n"
              "# Do not edit by hand: change policy.yaml (rules, pins, bans) and re-run. Review diffs in a PR.\n")
    args.out.write_text(header + yaml.safe_dump(lock, sort_keys=False, width=120), encoding="utf-8")
    print(f"wrote {args.out} ({len(new)} picks, {len(changes)} changed)")
    if old and changes:
        print("changes since last lock:\n" + "\n".join(changes))
    for w in warnings:
        print("warning:", w)
    if args.table:
        print(markdown_table(lock, args.table))
    return 0


if __name__ == "__main__":
    sys.exit(main())
