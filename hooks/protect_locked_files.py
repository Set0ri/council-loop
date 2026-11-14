#!/usr/bin/env python3
"""PreToolUse hook: block agent edits to files locked with `council.py lock-tests`.

ELI5: the answer key is locked in a drawer. The AI can read it but not change it.
Unlock: Logan sets COUNCIL_ALLOW_LOCKED=1 for the session, or removes the entry from
.council/locks.json in a reviewed PR. CI runs `council.py verify-locks` as the real backstop.
"""
import json
import os
import re
import sys
from pathlib import Path


def main() -> int:
    if os.environ.get("COUNCIL_ALLOW_LOCKED") == "1":
        return 0
    try:
        event = json.load(sys.stdin)
    except json.JSONDecodeError:
        return 0
    project = Path(os.environ.get("CLAUDE_PROJECT_DIR") or event.get("cwd") or ".")
    lock = project / ".council" / "locks.json"
    if not lock.exists():
        return 0
    locked = {p.replace("\\", "/") for p in json.loads(lock.read_text(encoding="utf-8-sig"))}
    tool = event.get("tool_name", "")
    inp = event.get("tool_input") or {}
    hit = None
    if tool in ("Edit", "Write", "MultiEdit", "NotebookEdit"):
        target = inp.get("file_path") or inp.get("notebook_path") or inp.get("path") or ""
        try:
            rel = Path(target).resolve().relative_to(project.resolve()).as_posix()
        except ValueError:
            rel = target.replace("\\", "/")
        hit = rel if (rel in locked or target.replace("\\", "/") in locked) else None
    elif tool == "Bash":
        cmd = inp.get("command", "")
        writes = re.search(r"(>|>>|\btee\b|\bsed\s+-i|\brm\b|\bmv\b|\bcp\b|\btruncate\b|\bgit\s+(checkout|restore)\b)", cmd)
        if writes:
            cmd_norm = cmd.replace("\\", "/")
            hit = next((p for p in locked if p in cmd or p in cmd_norm), None)
    if not hit:
        return 0
    print(json.dumps({"hookSpecificOutput": {
        "hookEventName": "PreToolUse",
        "permissionDecision": "deny",
        "permissionDecisionReason": f"{hit} is locked by the Council Loop (tests/answer key). "
                                    "Fix the code, not the test. Ask Logan to unlock if the test is wrong."}}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
