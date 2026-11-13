# Council Loop protocol (v2)

ELI5: This is the full rulebook. SKILL.md is the summary; read this when you run a loop.

## Roles

| Role | Count | Does | Never |
| --- | --- | --- | --- |
| Chair | 1 | Writes the intent brief and rubric, picks the lenses, merges duplicates, fuses candidates, runs the gate, writes the final | Authors or grades candidates |
| Expert | 7 (Full) / 3 (Lite) | One lens each. Writes ideas alone in a fresh context, then self-critiques and votes | Sees other experts' ideas before voting |
| Red team | 1 | Pre-mortem on the leaders, with severity tags | Proposes its own winner |
| Elder | 3 | Blind review of the top 2 (top 1 in the last loop), plus one idea of its own | Sees authorship or the chair's preference |
| Writer | 1 | Builds the winner in a verify loop | Edits locked tests |
| Verifier | 1 | Runs checks, tests, fact-checks | Changes the code under test |

Lens = objective to maximize + constraint to respect + method + evidence source.
Default lens kit for engineering work (swap per task):
1. User value
2. Correctness and verification
3. Simplicity and maintainability
4. Cost, performance and scale
5. Security and failure modes
6. Contrarian (first principles or 10x)
7. Delivery and operations

## Stage 0: intent (chair)

The chair fills `templates/intent-brief.md`:
- Logan's original words, verbatim.
- A ≤3-line restatement and a one-line ELI5.
- A rubric of 5–7 criteria with weights summing to 100. Each criterion names its check:
  machine, source or judgment.
- An evidence plan.
- A budget.
- The 7 lenses.

The rubric is frozen for the run. Changing it needs Logan's approval and is logged.

## Loop k (k ≤ 3; Logan can raise the cap)

| Step | Output | Cap | Enforced by |
| --- | --- | --- | --- |
| Observe | Facts ledger: verbatim quotes, numbers, paths | ≤1 page | `check` |
| Diverge | ≥13 ideas per expert (loops 2–3: mutations of survivors) | ≤25 words each | `check` |
| Self-critique | Top 3 per expert, refined, with evidence | ≤120 words each | `check` |
| Merge | Idea families (duplicates merged) | n/a | chair |
| Blind vote | Shuffled, equal-length packet; Borda; self-votes excluded | ≤120 words per card | `blind`, `tally` |
| Red team | Pre-mortem, S1/S2/S3 findings with fixes | ≤10 findings | ledger |
| Refine | Fix, rebut with evidence, or accept each S1/S2 | n/a | ledger |
| Fuse + rank | Exactly 7 cards (loop 1), then survivors + new grafts | ≤120 words + ELI5 | `check` |
| Elders | JSON reviews of the top 2 (top 1 in the last loop) | ≤250 words each | `elders` |
| Cut | 7→5→3→1 with one Pareto wildcard | n/a | `tally --keep` |
| Ledger | What changed, why, what was cut, ELI5 | ≤1 page | `check` |

**Verify beats vote.** A candidate that fails a machine check ranks last whatever its votes. Pass
the results to `tally --checks`.

**Tournament mode.** When a numeric scorer exists (benchmark, latency, accuracy, cost), execute each
candidate and add the score as a rubric criterion. Keep separate "islands" of candidates and migrate
the best between loops (AlphaEvolve, Mind Evolution).

**Stop early** if (a) the leader beats #2 on every criterion and every elder says "ship" with no open
S1, or (b) the best rubric score did not improve since the last loop.

**Split or open S1:** ask Logan one targeted question. When unattended, pick the safer option and
flag it.

## Final

1. The elders' synthesis merges the winner with the best compatible grafts from the runners-up, and
   lists what was deliberately left out.
2. Control arm: the chair answers the original ask one-shot. The answer is blinded and length-matched
   into the final elder comparison. Record the win or loss in `docs/outcomes.md`.
3. Write-up: use `templates/final-writeup.md`, in plain language, answer first. It contains an ELI5,
   one diagram, the evidence, the risks, the dissent, falsifiable success metrics, kill criteria, and
   next steps.

## Build (Direct tier, or after the Full and Lite tiers)

1. An independent agent writes the acceptance tests first, or mutation-checks the writer's tests.
2. Lock them with `council.py lock-tests tests/ path/to/migrations`. The PreToolUse hook blocks agent
   edits, and CI runs `verify-locks`. Unlocking needs Logan's approval (label `council-unlock`).
3. Run a verify loop of ≤5 iterations: implement, run the checks, fix. Use `/ralph-loop "<prompt>"
   --max-iterations 5 --completion-promise "VERIFIED"` if the plugin is installed.
4. If the loop is exhausted or a new fork appears, raise the tier. Never lower it silently.

## Learn

- Retro: use `templates/retro.md`, with at most 3 lessons, each with evidence and an ID, appended to
  `docs/lessons.md` with counters `helpful: 0 | harmful: 0`. Propose them as a PR.
- Outcome ledger: check the success metrics at 30 and 90 days. Resolved cases join the golden set.
- Golden set: 20–50 past tasks with known-good outcomes. Replay it before merging any change to
  prompts, policy or models. Merge only if quality per dollar does not regress.
- Monthly fire drill: canary tasks try to edit a locked test, run a migration, and publish. Any gate
  that fails to fire blocks the next merge.

## Schemas

Elder review (JSON):
```json
{"scores": {"A": {"R1": 8, "R2": 7}}, "verdict": {"A": "ship|revise|kill"}, "preferred": "A",
 "confidence": 0.7, "top_risks": [{"risk": "...", "severity": "S1|S2|S3"}],
 "key_fix": "...", "own_idea": "...", "eli5": "<=20 words"}
```

Votes for `tally`: `[{"voter": "E3", "ranking": ["B", "A", "C"]}]`. Mapping from `blind`:
`{"A": {"id": "C4", "author": "E2"}}`. Run ledger for `check`: see `templates/run.example.json`.
