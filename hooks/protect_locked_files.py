#!/usr/bin/env python3
"""PreToolUse hook: block agent edits to files locked with `council lock-tests`.

ELI5: the answer key is locked in a drawer. The AI can read it but not change it.
Unlock: the project owner sets COUNCIL_ALLOW_LOCKED=1 for the session (needed to run `council lock-tests`
again after an approved test change), or the change goes through a PR labelled council-unlock.
The hook is a convenience and can be bypassed; CI running
`council verify-locks --against origin/<base>` is the real lock.
"""
import json
import os
import re
import sys
from pathlib import Path


def main() -> int:
    if os.environ.get("COUNCIL_ALLOW_LOCKED") == "1":
        return 0
    try:                                                     # bytes, not the locale's idea of text (Windows)
        event = json.loads(sys.stdin.buffer.read().decode("utf-8-sig", errors="replace"))
    except (json.JSONDecodeError, AttributeError):
        return 0
    if not isinstance(event, dict):                          # not an event: a hook that fails must never block the session
        return 0
    project = Path(os.environ.get("CLAUDE_PROJECT_DIR") or event.get("cwd") or ".")
    lock = project / ".council" / "locks.json"
    if not lock.exists():
        return 0
    try:
        locked = set(json.loads(lock.read_text(encoding="utf-8-sig"))) | {".council/locks.json"}   # the manifest is locked too
    except (OSError, ValueError, TypeError):
        return 0
    tool = event.get("tool_name", "")
    inp = event.get("tool_input") if isinstance(event.get("tool_input"), dict) else {}
    hit = None
    if tool in ("Edit", "Write", "MultiEdit", "NotebookEdit"):
        target = str(inp.get("file_path") or inp.get("notebook_path") or inp.get("path") or "")
        names = {target, target.replace("\\", "/")}             # locks are kept with forward slashes
        try:
            rel = Path(target).resolve().relative_to(project.resolve()).as_posix()
            names |= {rel, rel.replace("\\", "/")}
        except (ValueError, OSError):
            pass
        hit = next((n for n in sorted(names) if n in locked), None)
    elif tool == "Bash":
        cmd = inp["command"].replace("\\", "/") if isinstance(inp.get("command"), str) else ""
        writes = re.search(r"(>|>>|\btee\b|\bsed\s+-i|\brm\b|\bmv\b|\bcp\b|\btruncate\b|\bgit\s+(checkout|restore)\b"
                           r"|\bdel\b|\berase\b|\b(Remove|Move|Copy|Rename)-Item\b|\b(Set|Add|Clear)-Content\b|\bOut-File\b)", cmd, re.I)
        if writes:
            hit = next((p for p in sorted(locked) if p in cmd), None)
    if not hit:
        return 0
    print(json.dumps({"hookSpecificOutput": {
        "hookEventName": "PreToolUse",
        "permissionDecision": "deny",
        "permissionDecisionReason": f"{hit} is locked by the Council Loop (tests/answer key). "
                                    "Fix the code, not the test. Ask the requester to unlock if the test is wrong."}}))
    return 0


if __name__ == "__main__":
    try:
        sys.stdout.reconfigure(newline="\n")                    # the same bytes on every system
    except (AttributeError, ValueError, OSError):
        pass
    sys.exit(main())
