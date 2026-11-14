#!/usr/bin/env python3
"""Write tests/fixtures/yaml_dump.json: documents and how PyYAML's safe_dump spells them.

The Rust YAML writer is a port of PyYAML's emitter. Its unit test reads this file and must produce
the same text for every case, byte for byte. Re-run this script after adding cases:

    python3 tests/make_yaml_fixture.py
"""
import json
import random
from pathlib import Path

import yaml

WORDS = ("queue retries idempotent ledger shard cache index batch stream replay quorum backoff tenant schema rollout canary "
         "metric budget latency replica snapshot throttle circuit fallback audit").split()
TEXTS = [
    "", " ", "a", "plain text", " leading", "trailing ", "two  spaces", "x: y", "x:y", "a #b", "a#b", "#start", "- item", "-item", "-",
    "? q", "?q", ": c", ":c", "[x]", "{x}", "a, b", "a [b] {c}", "&anchor", "*alias", "!tag", "|", ">", "'quoted'", '"double"', "it's",
    "%percent", "@at", "`tick`", "---", "--- doc", "...", "... end", "null", "Null", "~", "true", "False", "yes", "No", "on", "OFF", "y", "n",
    "123", "0x1F", "010", "1_000", "+5", "-7", "1.5", ".5", "1e3", "1.0e+3", "1:30", ".inf", "-.INF", ".NaN", "<<", "=", "2026-11-13",
    "2026-11-13 10:00:00", "2026-11-13T10:00:00Z", "2026-1-3", "12:30", "v1.2.3", "fanout:claude", "Claude Fable 5.2 (max)",
    "current model @ high", "C:\\Users\\me\\My Vault", "/home/me/a b/c", "tab\there", "line\nbreak", "two\n\nbreaks", "ends with break\n",
    "\nstarts with break", "space \nbreak", "break\n space", "caf\u00e9", "\u2192 arrow", "emoji \U0001f600", "nbsp\u00a0here", "bell\x07",
    "back\\slash", 'quote " inside', "both ' and \"", "a: b: c", "key:", "url http://example.org/a?b=c#d", "100%", "a&b", "a*b", "a|b", "a>b",
    "x" * 130, " ".join(WORDS), " ".join(WORDS * 6), "note: " + " ".join(WORDS * 6), "it's " + " ".join(WORDS * 6),
    "caf\u00e9 " + " ".join(WORDS * 6), "long  double  spaces " + " ".join(WORDS * 5), ("word " * 40).strip(), "a" * 90 + " b " + "c" * 90,
    "'" * 50, "tab\t" + " ".join(WORDS * 6), "\\" * 100 + " x " + "y" * 50, "\u00e9" * 100 + " and more words after the long run of letters",
]


def nested(rng: random.Random, depth: int = 0):
    kind = rng.choice(["str", "str", "num", "list", "map", "map"] if depth < 3 else ["str", "num"])
    if kind == "str":
        return rng.choice(TEXTS)
    if kind == "num":
        return rng.choice([0, 1, -5, 3.5, 62.0, 1e-05, 1e17, 26.54, True, False, None, 10 ** 12, -0.0, 0.1 + 0.2])
    if kind == "list":
        return [nested(rng, depth + 1) for _ in range(rng.randint(0, 4))]
    return {rng.choice(TEXTS[1:] + ["k", "key two", "max", "fanout:mix"]): nested(rng, depth + 1) for _ in range(rng.randint(0, 4))}


def main() -> None:
    rng = random.Random(20261113)
    docs = [{"t": t} for t in TEXTS] + [[t] for t in TEXTS] + [{t: 1} for t in TEXTS] + [{"a": {"b": [t, {"c": t}]}} for t in TEXTS]
    docs += [t for t in TEXTS[:40]] + [1, 2.5, None, True, [], {}, [[]], [{}], {"a": []}, {"a": {}}, [[1, 2], [3, [4, 5]]],
                                       {"a": [[1], {"b": [2, {"c": [3]}]}]}, [{"a": 1, "b": 2}, {"c": [1, 2]}]]
    docs += [nested(rng) for _ in range(400)]
    lock = yaml.safe_load((Path(__file__).resolve().parents[1] / "skills" / "council-loop" / "models" / "lock.yaml").read_text(encoding="utf-8"))
    lock["generated"], lock["data"]["collected"] = str(lock["generated"]), str(lock["data"]["collected"])
    cases = [{"value": d, "width": w, "text": yaml.safe_dump(d, sort_keys=False, width=w)} for d in docs for w in (80, 120)]
    cases.append({"value": lock, "width": 120, "text": yaml.safe_dump(lock, sort_keys=False, width=120)})
    out = Path(__file__).resolve().parent / "fixtures" / "yaml_dump.json"
    out.write_text(json.dumps(cases, ensure_ascii=True, allow_nan=False), encoding="utf-8")
    print(f"{len(cases)} cases -> {out}")


if __name__ == "__main__":
    main()
