# Council packet · run 261113-183103-take-turn-review · CONTROL ARM · seat CTRL

Answer the ask below on your own, in one pass. Give your single best answer.

## The ask (the requester's own words)
> Take a turn to review and iterate. Im handing the reigns to you, to make this more useful for yourself. Be free

## Brief
Decide the changes that would most improve the council-loop kit (v0.2, in this repo) for the AI that runs it: the chair, and the seats that receive its packets. Pick what to change before release.


## Rubric (your answer is judged on this; weights sum to 100)
| id | criterion | weight | checked by |
| --- | --- | --- | --- |
| R1 | Fewer wrong, stalled or drifting runs by the executing model | 35 | judgment |
| R2 | Integrity: bias controls hold and the record stays honest | 25 | machine |
| R3 | Lower cost: seat calls, tokens, wall-clock | 15 | source |
| R4 | Keeps Logan's process whole (7 x 13, up to 3 loops, cross-lab elders, 7-5-3-1, ELI5s) | 15 | judgment |
| R5 | Small and testable: can ship today with a unit test | 10 | machine |

## Facts
Repo root: <repo>. Read these: skills/council-loop/SKILL.md (what the chair reads), skills/council-loop/reference/protocol.md, skills/council-loop/scripts/run.py (state machine), skills/council-loop/scripts/council.py (gate, blind, tally, cut, locks), scripts/providers.py (CLI adapters), skills/council-loop/templates/packets/*.md (what seats read), tests/test_run.py and tests/sim.py. Numbers: a Full run is about 76 seat calls (25 per loop x 3 + 1 control); Lite about 9. In the Cowork host each subagent spawn carries roughly 60k tokens of overhead before it reads its packet. Known limits today: the codex and gemini CLI paths are written from docs and untested; a nested `claude -p` call fails authentication in this sandbox; README.md, AGENTS.md and mcp/server.py still describe v0.1 and will be rewritten after this council, so do not spend ideas on them.

## Do this
Write your one best answer as a card:
- title: 8 words at most.
- body: 90 to 110 words. What to do, how it works, and the evidence it rests on.
- eli5: 20 words at most.
You may read project files for evidence. Do not edit any project file.

## Output
Write ONLY this JSON to the file `<repo>/.council/runs/261113-183103-take-turn-review/control/CTRL.out.json` (create it; overwrite it if it exists). Then reply with one line: done CTRL.out.json
{"title": "...", "body": "...", "eli5": "..."}
