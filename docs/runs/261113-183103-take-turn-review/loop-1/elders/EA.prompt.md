# Council packet · run 261113-183103-take-turn-review · loop 1 · ELDER REVIEW · seat EA

You are an independent elder reviewing the council's finalists. You do not know who or what wrote
any candidate; do not guess. Judge only against the ask and the rubric below. Ignore length and
polish. If you are unsure, say so in your confidence.

## The ask (the requester's own words, verbatim)
> Take a turn to review and iterate. Im handing the reigns to you, to make this more useful for yourself. Be free

## Rubric (weights sum to 100)
| id | criterion | weight | checked by |
| --- | --- | --- | --- |
| R1 | Fewer wrong, stalled or drifting runs by the executing model | 35 | judgment |
| R2 | Integrity: bias controls hold and the record stays honest | 25 | machine |
| R3 | Lower cost: seat calls, tokens, wall-clock | 15 | source |
| R4 | Keeps the requester's process whole (7 x 13, up to 3 loops, cross-lab elders, 7-5-3-1, ELI5s) | 15 | judgment |
| R5 | Small and testable: can ship today with a unit test | 10 | machine |

## Facts
Repo root: <repo>. Read these: skills/council-loop/SKILL.md (what the chair reads), skills/council-loop/reference/protocol.md, skills/council-loop/scripts/run.py (state machine), skills/council-loop/scripts/council.py (gate, blind, tally, cut, locks), scripts/providers.py (CLI adapters), skills/council-loop/templates/packets/*.md (what seats read), tests/test_run.py and tests/sim.py. Numbers: a Full run is about 76 seat calls (25 per loop x 3 + 1 control); Lite about 9. In the Cowork host each subagent spawn carries roughly 60k tokens of overhead before it reads its packet. Known limits today: the codex and gemini CLI paths are written from docs and untested; a nested `claude -p` call fails authentication in this sandbox; README.md, AGENTS.md and mcp/server.py still describe v0.1 and will be rewritten after this council, so do not spend ideas on them.

## Candidates
### Candidate A: Defects plus four changes that remove typing
Ship the five proven defects, then only the four structural changes that take work or risk away from the chair. `next` lists just the agents still missing after a restart. `ingest --drop <agent>` drops one named agent and can never drop the control arm by accident. Authors are derived from `from` and `parents`, so the chair stops typing them and nobody can rank a card they fed. Each voter reads the cards in its own evenly spaced order. Deferred, with reasons recorded: per-voter label letters (suspected bias, unmeasured), attempt counters, overlap warnings. Nine changes, one test each.
ELI5: Fix what broke, plus four changes that mean less typing and fairer votes. Park the rest.

### Candidate B: Stop shared authorship from corrupting the blind tally
Fix council.tally before release: self-vote removal breaks once cards have several authors, and merge and fuse create those by design. Reproduced: a pool card that all seven experts wrote, ranked first on every ballot, lands 11th of 12 and misses the shortlist; three Lite candidates with two authors each all score 0.00, so label order picks the winner and no deviation is logged. Change: skip ballots with under two eligible cards; mark a card with under two eligible voters unranked, send it to the elders, and log a deviation. Also blind-check the control card at ingest_ideate: today an innocent phrase stalls next_elders, unfixable without unblinding. Add three unit tests.
ELI5: When everyone helped write an idea, nobody may vote for it, so it wrongly loses. Fix the counting.

### Candidate C: Full hardening in dependency order
Do every fix the council raised, in an order where each step rests on a tested one. First the five proven defects (vote count, dead retry, leak alarm, elder-score crash, idea-length rejection). Then restart safety: `next` lists only agents still missing, validation becomes a pure function, attempts are counted per agent at ingest, and `ingest --drop <agent>` replaces bare `--force`. Then vote integrity: authors are derived from `from` and `parents`, every kept card must reach the pool, and each voter gets its own evenly spaced order and its own label letters. Roughly twelve changes, one test each; nothing is deferred.
ELI5: Fix everything the team found, one tested step at a time.

## Do this
- Score every candidate 1 to 10 on every rubric id.
- Give each a verdict: ship, revise or kill.
- Name the one you prefer.
- List at most 3 risks (S1 = blocks shipping, S2 = must fix soon, S3 = minor). No nitpicks.
- Give the one fix that matters most, and one idea of your own that no candidate has.

## Output
Write ONLY this JSON to the file `<repo>/.council/runs/261113-183103-take-turn-review/loop-1/elders/EA.out.json` (create it; overwrite it if it exists). Then reply with one line: done EA.out.json
{"scores": {"<label>": {"R1": 0, "R2": 0}},
 "verdict": {"<label>": "ship|revise|kill"},
 "preferred": "<label>",
 "confidence": 0.0,
 "top_risks": [{"risk": "...", "severity": "S1|S2|S3", "candidate": "<label or all>"}],
 "key_fix": "...",
 "own_idea": "...",
 "eli5": "<20 words at most>",
 "friction": "<optional: one line on anything unclear in this packet>"}
