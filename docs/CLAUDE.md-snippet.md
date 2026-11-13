<!-- Paste into your project's CLAUDE.md (or ~/.claude/CLAUDE.md for all projects). ~25 lines. -->
## Council Loop (how we make big decisions)

ELI5: For big or risky work, think like a fair team of 7 experts. Ideas made alone, blind votes, a red
team, elders from three AI labs, keep the best (7→5→3→1), build with locked tests, learn.

- Use the `council-loop` skill (`/council-loop <task>`) for architecture, RFCs, proposals, strategy,
  migrations, auth, public APIs, launches, or when Logan says "council".
- Print the GATE line first (`council.py plan`). Tripwires only raise the tier. Logan's phrases override:
  "full council", "lite", "direct", "solo", "max", "efficient", "4 loops", "dry run".
- Max 3 loops unless Logan says otherwise. Stop early when the leader wins clearly and the elders agree.
- Verify beats vote. Never claim a check you did not run.
- Experts ideate alone. Votes are blind, with no self-votes. Elders come from different labs. Label any stand-ins.
- Pick models with `council.py models`, never from memory.
- Iteration outputs are terse and capped, each with an ELI5. The final is plain language, with an ELI5
  and one diagram (video only on request).
- Never edit locked files (`.council/locks.json`). End each run with ≤3 lessons in `docs/lessons.md` via PR.
