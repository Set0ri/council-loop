# Council packet · run 261113-183103-take-turn-review · loop 1 · RED TEAM · seat RT

You are the council's red team. Your job is to find what would make these candidates fail, before
reality does. Attack ideas, not wording. The candidates are anonymous.

## The ask (the requester's own words)
> Take a turn to review and iterate. Im handing the reigns to you, to make this more useful for yourself. Be free

## Rubric
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
### Candidate A: Derive card authors from provenance, not chair input
Self-vote exclusion in tally() trusts each card's authors list, and the chair types that list by hand twice: in pool.json at merge and in candidates.json at fuse. _check_cards only checks that authors is a non-empty list of expert ids; nothing reads the from or parents fields. A fused card built from two experts' cards but labelled with a third expert's id lets those two rank their own work, unrecorded. Fix: ingest_merge requires every from ref to be a real kept-card ref and sets authors to those experts; ingest_fuse requires parents to be shortlisted pool ids and sets authors to the union of the parents' authors. One unit test covers both.
ELI5: The computer, not the referee, writes whose idea each card came from, so nobody can vote for their own.

### Candidate B: Next resumes: spawn only missing seats
After a context reset the chair runs status, then next. Today next rewrites every prompt and prints SPAWN lines for all seats of the phase, including seats whose valid output is already on disk, so a reset mid-fan-out re-spawns up to seven agents at roughly 60k tokens each and overwrites good work. Fix: next validates existing outputs first and prints only the seats still missing or invalid, with a line such as `5 of 7 in`. It stores an attempt count per agent and prints the exact --force command at the second failure, so the `fails twice` rule no longer lives in chair memory. Status shows the same pending list.
ELI5: After a restart, only redo the seats that have not answered, and let the tool count failures.

### Candidate C: Give each voter its own candidate order
blind() is seeded only by run, loop and phase, so every voter in a phase gets the identical packet, and the same card sits first on all seven ballots. LLM judges show measurable position bias (Zheng et al. 2023, MT-Bench), so a shared order adds the same tilt to every ballot and Borda sums it instead of cancelling it. Fix: keep one label-to-card mapping per phase, but have write_seat_prompts render each seat's copy in a rotated order (Latin square over seats), so each card leads about equally often. Labels, mapping and tally are unchanged; no model calls. Test: with seats at least equal to cards, every label leads some packet.
ELI5: Everyone sees the cards in a different order, so being listed first stops giving a free boost.

### Candidate D: Unblock CLI retries; stop false leak alarms
Two stalls, both reproduced by running the kit in a scratch copy. First: when a CLI or API agent returns invalid output, ingest writes a retry prompt and prints `RUN council.py dispatch`, but cmd_dispatch skips any agent whose output file exists, so it answers `nothing to dispatch` and ingest rejects the same file again. Every cross-lab elder travels this path. Fix: move the rejected file aside when the retry prompt is written, and name the agents in the printed retry line. Second: the identity-leak check flags ordinary cost phrases built on the word for a council place (five of five tried), forcing rewrites at merge. Narrow it to real ids. One unit test for each.
ELI5: Two stuck buttons: a retry that does nothing, and a blind-check alarm that rings at ordinary words.

### Candidate E: Reject agents only for faults the run reads
Every rejected expert is a full respawn: about 60k tokens of host overhead plus the whole task again, and after two failures the chair must drop that voice, breaking the seven-lens rule. Three checks guard nothing downstream: ideas over 25 words (only counted; check_run already treats them as a warning), incomplete elder scores (stored, never read by cut or final), and ballots missing, repeating or misnaming a label (tally already ranks gaps neutrally). Make the first two warnings, repair ballots missing up to one in ten labels, and log each repair in run.json. Self-vote exclusion and blind labels stay untouched. Idea count, card shape and verdicts stay hard errors.
ELI5: Do not make an expert redo everything because one idea line ran long or a ballot skipped one name.

## Do this
1. For each candidate, imagine it shipped and had clearly failed six months later. Why did it fail?
2. Report at most 10 findings in total, the most serious first. For each:
   - candidate: its label.
   - severity: S1 = blocks shipping, S2 = must fix soon, S3 = minor.
   - finding: 40 words at most.
   - evidence: the fact, file, number or reasoning behind it. If you are unsure, start the finding
     with "suspected" and say what check would confirm it.
   - fix: a concrete fix, 30 words at most.
3. No nitpicks, no style notes, no invented problems. Look for hidden costs, security and data
   risks, irreversible steps, failure under load, ways to game the checks, unclear ownership, and
   anything the rubric missed. You may read project files for evidence; edit nothing.

## Output
Write ONLY this JSON to the file `<repo>/.council/runs/261113-183103-take-turn-review/loop-1/redteam/RT.out.json` (create it; overwrite it if it exists). Then reply with one line: done RT.out.json
{"findings": [{"candidate": "<label>", "severity": "S1", "finding": "...", "evidence": "...", "fix": "..."}],
 "eli5": "<the single biggest risk, 20 words at most>",
 "friction": "<optional: one line on anything unclear in this packet>"}
