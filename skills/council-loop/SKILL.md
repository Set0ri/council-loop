---
name: council-loop
description: Run the Council Loop for big, risky or hard-to-reverse work (architecture, RFCs, proposals, strategy, migrations, auth, public APIs, launches). A strong chair runs up to 3 OODA loops with 7 orthogonal experts × ≥13 ideas each. Votes are blind and a red team attacks the favourites. A cross-lab elder council (Claude, OpenAI, Gemini) reviews the finalists as the field is cut 7→5→3→1. The winner is then built inside a verify loop. Picks models per role from Artificial Analysis data (solo, fan-out per family, cross-lab × max, balanced, efficient). Applies context, sizing and memory rules. Use when the user says "council", "full council", "lite council", "elder review", or the task trips a tripwire.
license: MIT
metadata:
  version: "0.1.0"
  owner: "Sylvan Labs"
---

# Council Loop

ELI5: For big decisions, act like a fair team of 7 different experts. Everyone dreams up ideas
alone, a referee hides names and counts votes, a "bad guy" attacks the favourites, and three wise
elders from different AI labs check the finalists. Keep the best (7 → 5 → 3 → 1). Build it with
tests that can't be fudged. Write down what you learned.

Scripts live next to this file: `S=${CLAUDE_SKILL_DIR}/scripts` (outside Claude Code, use the
skill folder path). The script does the bookkeeping. You do the thinking. Never fake a step the
script can do.

## 1. Gate first: print one line before any work

```bash
python3 $S/council.py plan "<task in Logan's words>" --mode crosslab --profile balanced
# -> GATE tier=full mode=crosslab profile=balanced loops=3 rel_cost=1.0 (raised by tripwire(s): ...)
```

| Tier | When | Shape |
| --- | --- | --- |
| Direct | Small, clear, reversible | 1 writer. Tests first, then locked. Verify loop of ≤5 iterations |
| Lite | Medium decisions | 3 lenses × 5 ideas, 1 loop, 1 elder |
| Full | High stakes, irreversible, public, architecture, strategy | Logan's process: 7 lenses × ≥13 ideas, ≤3 loops, 3 elders, 7→5→3→1 |

- Tripwires (migrations, auth, infra, deletes, public APIs, publishing, architecture) only RAISE the tier.
  Lowering a tier needs a Logan-approved PR after ≥10 runs of that task type.
- Logan's phrases override everything: "full council", "lite", "direct", "solo", "max",
  "efficient", "4 loops", "skip elders", "dry run".
- "dry run" = print the plan and stop. Spend no tokens on ideas.

## 2. Pick models: never hard-code them

```bash
python3 $S/council.py models --mode crosslab --profile balanced   # roles -> models, from models/lock.yaml
```

- Modes: `solo` (current model plays every role) · `fanout:claude|gemini|openai|openrouter|open|mix`
  (strong chair + cheaper workhorses) · `crosslab` (experts from mix + one elder per lab).
- Profiles: `max` · `balanced` · `efficient`. Rule for every pick: quality − k·log2(cost per task),
  read from Artificial Analysis data. Details: `reference/model-modes.md`.
- Claude seats: spawn the `council-expert` / `council-red-team` / `council-elder` subagents with the
  model override (`fable`, `opus`, `sonnet`, `haiku`). Other families: `python3 $S/council.py ask --provider openai|gemini|openrouter --prompt-file <f>`.
- If a provider is unavailable, substitute the strongest available model in a fresh context and
  LABEL it as a stand-in in every artifact. Never claim a cross-lab check that did not happen.
- If `models/lock.yaml` is over 30 days old, say so and suggest `council.py refresh`.

## 3. The Full loop (Lite = same steps with 3 lenses × 5 ideas, 1 loop)

**Stage 0: Intent (chair).** Fill `templates/intent-brief.md`:
- Logan's ask, verbatim.
- A ≤3-line restatement and a one-line ELI5.
- A rubric of 5–7 weighted criteria, written BEFORE any ideas.
- An evidence plan: what tests, data or sources can check each criterion.
- 7 orthogonal lenses. Each lens is defined by objective + constraint + method, not a job title.
  If two lenses' top ideas overlap more than half, re-seed one.
- If Logan is present, he approves the brief (one screen). If not, proceed and say so.

**Each loop (max 3), mapped to OODA:**
1. **Observe.** Gather facts into a facts ledger. Quotes, numbers, paths and test output stay verbatim.
2. **Orient.**
   - a. Each expert, in its own fresh context, writes ≥13 ideas, one line each (≤25 words). In loops 2–3,
     the ideas improve, graft, simplify or invert the survivors instead of starting over.
   - b. Self-critique against the rubric, citing evidence. Keep the top 3, refined.
   - c. The chair merges duplicates into idea families.
   - d. Blind vote: `council.py blind` (shuffled, equal-length, names hidden), then each expert ranks.
     `council.py tally` runs Borda with self-votes excluded.
   - e. Red team (`council-red-team`) runs a pre-mortem on the leaders: "it failed in 6 months, why?".
     Findings are S1/S2/S3, with no nitpicks.
   - f. Authors fix, rebut with evidence, or accept the risk in writing.
   - g. The chair fuses the strengths into exactly 7 candidate cards (`templates/candidate-card.md`,
     ≤120 words + ELI5), then ranks them. Machine checks beat votes (`--checks`).
3. **Decide.** Elders review the top 2 (top 1 in the last loop): `council.py elders --packet <file>`.
   They see a blind packet that includes Logan's original ask, and return JSON scores, risks and one
   idea of their own. Cut with `council.py tally --keep 5` (then 3, then 1). This keeps one Pareto
   wildcard: a cut candidate that is the unique best on some criterion.
4. **Act.** Write the loop ledger (`templates/ledger.md`, ≤1 page, with ELI5). Feed elder ideas into
   the next loop. If an automated scorer exists (benchmark, test suite, metric), run the candidates and
   score them (Tournament mode).

**Stop early** when the leader wins on every criterion and all elders say "ship" with no open S1.
Also stop when no candidate improved since the last loop. Never exceed 3 loops unless Logan says so.

**Ask, don't fake consensus.** If the elders split, or an S1 is left unrebutted, stop and ask Logan
ONE targeted question. Log the answer as a label. When unattended: choose the safer option (the
higher tier, nothing irreversible) and flag it at the top of the write-up.

**Final.**
1. The elders' synthesis merges the winner with the best compatible ideas from the runners-up.
2. Slip in the control arm: a one-shot answer from the chair, blinded and length-matched, judged
   against the winner. Record the result.
3. Write the result up with `templates/final-writeup.md`.

**Build.** The single writer implements. Tests are written first, then locked
(`council.py lock-tests <paths>`). Run a verify loop of ≤5 iterations. An independent agent writes or
mutation-checks the tests. CI runs `council.py verify-locks`.

**Learn.**
- Add ≤3 lesson bullets to `docs/lessons.md`. Each has an ID, evidence, and helpful/harmful counters.
  Propose them as a PR; never auto-merge.
- The winner states falsifiable success metrics and kill criteria, checked again at 30 and 90 days.
- The loop never edits its own rubric, tests or locks.

## 4. Non-negotiables

1. Verify > vote. Tests, data and cited sources outrank opinions. Never claim a check you did not run.
2. Experts think alone first. Judges never grade their own work. Votes change only for new evidence.
3. Blind, shuffled, equal-length candidates. Ranked (Borda) voting. Dissent is recorded, not averaged away.
4. Read-only experts, red team and elders. One writer. Agents get no production or publishing
   credentials. Locks are enforced in CI, not only by hooks.
5. Every artifact carries a one-line ELI5.

## 5. Context, sizing and memory (details: `reference/context-and-memory.md`)

- Size the effort to the stakes (the gate). Keep each agent's context roughly 40–60% full. Every
  expert gets a fresh context.
- Subagents return ≤1–2k-token cards. They write details to `.council/runs/<id>/` and pass the path,
  not the text.
- Compress the conversation, never the evidence.
- Build one item per loop.
- Code: search with grep/glob before reaching for embeddings. Documents under ~200k tokens: put them
  in context. Larger: contextual retrieval (chunk + context + BM25 + rerank).
- Memory layers:
  - CLAUDE.md: rules, under 200 lines.
  - This skill: procedure, loaded on demand.
  - Run ledgers: the state of one run.
  - `docs/lessons.md`: itemized, counted lessons.

## 6. Two-speed writing

- During the loops: terse, salient, capped (idea ≤25 words, card ≤120, elder review ≤250, ledger ≤1
  page), each with an ELI5.
- The final write-up only: plain, matter-of-fact language. The answer first, then an ELI5, one diagram
  of the decision, the evidence, the risks, the next steps, and the dissent. Animation or video only on
  request.

## 7. Files

- `reference/protocol.md`: every stage, cap, schema and stop rule.
- `reference/model-modes.md`: modes × profiles, rules, refresh, extending.
- `reference/context-and-memory.md`: sizing, context, chunking, memory, with sources.
- `reference/research.md`: the evidence behind each rule.
- `templates/`: brief, ideas, card, ledger, elder packet, final write-up, retro, `run.example.json`.
- `models/policy.yaml`: the selection rules (edit these).
- `models/lock.yaml`: the generated picks (never edit by hand).

If you cannot run scripts (for example on claude.ai), do the same bookkeeping by hand. Shuffle,
relabel, exclude self-votes and keep the caps, and say that you did it manually.
