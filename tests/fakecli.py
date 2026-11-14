#!/usr/bin/env python3
"""A stand-in for the claude, codex and gemini command-line tools (it answers by the name it is run as).

It reads the prompt on stdin and answers the way tests/sim.py would. FAKE_CLI_PROSE names seats whose
FIRST answer is prose instead of JSON, so tests can drive the reject -> retry -> accept path.
FAKE_CLI_DOWN names tools that fail the way a logged-out tool does.
It checks only that our adapters hold together; it says nothing about the real tools.
"""
import json
import os
import sys
import zlib
from pathlib import Path

sys.path.insert(0, os.environ["FAKE_CLI_TESTS"])
import sim  # noqa: E402

name = Path(sys.argv[0]).stem
for _stream in (sys.stdout, sys.stderr):
    _stream.reconfigure(encoding="utf-8", errors="replace")
prompt = sys.stdin.buffer.read().decode("utf-8", errors="replace")
if name in os.environ.get("FAKE_CLI_DOWN", "").split(","):           # a tool that is installed but logged out
    print(f"not logged in: run `{name} login` first", file=sys.stderr)
    sys.exit(3)
if "Reply with exactly the two letters" in prompt:                 # doctor --probe
    reply = "OK"
else:
    seat = sim.seat_of(prompt)
    state = Path(os.environ["FAKE_CLI_STATE"]) / f"{seat}.{zlib.crc32(prompt.splitlines()[0].encode())}"
    first = not state.exists()
    state.write_text("seen", encoding="utf-8")
    if first and seat in os.environ.get("FAKE_CLI_PROSE", "").split(","):
        reply = "Sure! I had a careful look at the packet and here are my thoughts in plain prose."
    elif "· CODEBASE MAP ·" in prompt:                              # the mapper answers with a document
        reply = sim.codebase_map(prompt)
    else:
        reply = json.dumps(sim.answer(prompt))
if name == "codex":                                                  # codex exec ... -o <file> ... -
    Path(sys.argv[sys.argv.index("-o") + 1]).write_text(reply, encoding="utf-8")
    print("tokens used: 1234")
elif name == "gemini":                                               # gemini -o json -p <nudge>
    print(json.dumps({"response": reply, "stats": {}}))
else:                                                                # claude -p --output-format text
    print(reply)
