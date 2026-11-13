# Agent instructions: Council Loop

<!-- Read by Codex, Gemini CLI, Cursor, Copilot and other AGENTS.md tools. Claude Code: import with @AGENTS.md. -->

For big, risky or hard-to-reverse work, follow the Council Loop in `skills/council-loop/SKILL.md`
(read it before starting). Short version:

ELI5: Lots of ideas, made alone. Fair, blind votes. A red team. Elders from three AI labs.
Keep the best (7→5→3→1). Build with locked tests. Write down the lesson.

1. **Gate first.** Run `python3 skills/council-loop/scripts/council.py plan "<task>"` and print its GATE
   line. Tripwires (migrations, auth, infra, deletes, public APIs, publishing, architecture) only RAISE
   the tier.
2. **Tiers.**
   - Direct: one writer with tests first, a verify loop of ≤5 iterations.
   - Lite: 3 lenses × 5 ideas, 1 loop.
   - Full: 7 lenses × ≥13 ideas, ≤3 loops, elders review the top 2/2/1, cut 7→5→3→1.
3. **Verify beats vote.** Tests, data and cited sources outrank opinions. Never claim a check you did not run.
4. **Independence.** Experts ideate alone. Votes are blind (`council.py blind`) and tallied with self-votes
   excluded (`council.py tally`). Elders get Logan's original words.
5. **Models.** Run `council.py models --mode <solo|crosslab|fanout:FAMILY> --profile <max|balanced|efficient>`.
   Label any stand-in model. Never hard-code model names.
6. **Two-speed writing.** Iteration outputs are capped and terse, each with a one-line ELI5. Only the final
   is written in plain language, with an ELI5 and one diagram.
7. **Locks.** Never edit files listed in `.council/locks.json`. CI enforces this with `council.py verify-locks`.
8. **Learn.** End with ≤3 lessons appended to `docs/lessons.md` through a PR. Never edit your own rubric or tests.

Max 3 loops unless Logan says otherwise. Agents get no production or publishing credentials.
