# Agent instructions: Council Loop

<!-- Read by Codex, Gemini CLI, Cursor, Copilot and other AGENTS.md tools. Claude Code: import with @AGENTS.md. -->

For big, risky or hard-to-reverse work, follow the Council Loop in `skills/council-loop/SKILL.md`
(read it before starting). Short version:

ELI5: Facts first. Lots of ideas, made alone. Fair, blind votes. A red team. Elders from three AI
labs. Keep the best (7→5→3→1). Build with locked tests. Write down the lesson.

Before changing code in any tier, read `docs/ARCHITECTURE.md` (the codebase map).

`C` below is `council`, the binary (`cargo install --path .` in this repo). Without it, `C` is
`sh skills/council-loop/scripts/council`: that launcher runs the binary when it finds one and the
Python reference when it does not (Python 3.10+ and PyYAML). From cmd or PowerShell: `bin\council.cmd`.

1. **Start.** `C init "<the ask, verbatim>" --stakes low|medium|high --reversible yes|no`. Show the GATE
   line (tier, models, seat calls, tokens) and the MEMBERS line (researchers, mapper, memory) it prints. Tripwires (migrations, auth, infra, deletes, public
   APIs, publishing, architecture) only RAISE the tier. `C plan "<ask>"` is a free dry run.
2. **Loop.** `C next` says exactly what to do and writes each seat's prompt file. Do that, then
   `C ingest`. Repeat. `C cut --eli5 "<one line>"` ends each loop. `C status` after a context reset.
   - Direct: one writer with tests first, a verify loop of ≤5 iterations.
   - Lite: 3 lenses × 5 ideas, 1 loop, 1 elder. About 9 seat calls (12 with two researchers and the mapper).
   - Full: 7 lenses × ≥13 ideas, ≤3 loops, elders review the top 2/2/1, cut 7→5→3→1. About 76 seat calls (79 with two researchers and the mapper).
   - Both: researchers (2 by default) bring sourced facts first, and a mapper rewrites the codebase map
     when the code's structure changed. Settings: `--research N`, `--no-research`, `--map stale|always|never`,
     `--no-map`, or `members:` in `.council/config.yaml`. `C config` shows what is in force.
3. **Seats.** Give each seat a fresh context and the one-line prompt `next` prints. No sub-agents here?
   `C dispatch` runs them through the claude, codex or gemini CLI. A seat that fails twice:
   `C ingest --drop <seat> --reason "<why>"`.
4. **Verify beats vote.** Tests, data and cited sources outrank opinions. Never claim a check you did not run.
5. **Shared memory.** A seat may add `research` (facts, each with a source) and `notes` (thoughts) to
   its output; the tool files them. Facts reach every seat; thoughts arrive one step later, unsigned,
   never inside a blind vote. You: `C note "<text>" [--to research]`, `C memory add|search|retire`.
   Never write the memory folder or the map's first line by hand.
6. **Independence.** Experts ideate alone. Votes are blind and the tool counts them; a voter's own card
   earns nothing from that voter. You never rank. Do not open `control/` or the elder packets before the final.
7. **Models.** The tool assigns every seat from `models/lock.yaml`. Label any stand-in. Never hard-code model names.
8. **Two-speed writing.** Iteration outputs are capped and terse, each with a one-line ELI5. Only the final
   is written in plain language, with an ELI5 and one diagram.
9. **Honest record.** Anything done outside the protocol: `C deviation "<what and why>"`. `C audit`
   re-counts every vote from the ballots on disk.
10. **Locks and the map.** Never edit files listed in `.council/locks.json`. CI enforces this with
   `C verify-locks --against origin/<base>`, and `C map --check` fails when the map is behind the code.
11. **Learn.** End with ≤3 lessons appended to `docs/lessons.md` through a PR. Never edit your own rubric or tests.

Max 3 loops unless the requester says otherwise. Agents get no production or publishing credentials.
