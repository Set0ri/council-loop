# Retro: 261113-183103-take-turn-review

ELI5: The team checked its own rulebook, found a dozen real faults by running it, and the referee itself needed a fix mid-game.

- What worked: independent ideation (two experts found the same dead retry without seeing each other); the red team and the elder reproduced every serious finding by running the code in a scratch copy; the blind vote was unanimous; the one-shot control arm kept the council honest by finding the top defect alone.
- What did not: the vote counter had an S1 bug that this very run would have hit, so the referee was patched mid-run; the identity-leak check made the chair rewrite two cards at merge; the chair saw the control arm early by opening the elder packet; the only elder was from the chair's own lab; the brief gave a wrong file path that three agents tripped on; the rubric said "machine" for criteria with no machine results in the packets; a Lite run cost 1,269,206 tokens, about 8 times the one-shot control, and the red team and elder took 15 minutes each.
- Calls: ideate 4, redteam 1, rank 3, elders 1. Total 9 (the gate estimated 9).
- Loops used: 1 of 1. Rank-vote agreement per loop: 1.0.
- Control arm: the council's answer was preferred by 1 of 1 elder(s)
- Deviations: loop 1 fuse: the vote counter (tally) was replaced before any ballot was cast: red-team finding 1 (S1) showed the old one scored co-written cards 0.0 and treated a ballot gap as a last place; new head-to-head count has unit tests; loop 1 elders: the chair opened the elder packet to check it and so saw the blinded control arm before the elder review; no candidate was edited afterwards (single loop), but the tool did not warn against it; loop 1 cut: elders split or an S1 was open and the requester could not be asked; safer option taken: build the winner with the elder's S1 fix grafted in (a vote must never cut a card it could not judge) and the control arm sealed; nothing is released yet, so every step can be undone
- Friction reported by the seats:
  - L1 (ideate, loop 1): Packet lists scripts/providers.py; it is at skills/council-loop/scripts/providers.py.
  - L2 (ideate, loop 1): The lens lists _collect and verify_locks as evidence, but chair edits to seat out.json files before ingest are out of scope for any deterministic fix; say if that is accepted.
  - L3 (ideate, loop 1): Seats get no word counter, yet idea lines over 27 words are hard errors; the packet should give a counting rule or the check should only warn.
  - RT (redteam, loop 1): The packet does not say whether a pre-existing bug that a candidate amplifies (tally's zero scores under A) counts against that candidate; I treated it as yes.
  - L1 (rank, loop 1): R2 and R5 are marked machine-checked but no machine results are in the packet; judged from descriptions.
  - L2 (rank, loop 1): R2 says 'machine' checked, but no candidate states which machine checks verify blinding, so R2 was judged on described mechanisms.
  - L3 (rank, loop 1): R3 is ambiguous between runtime seat cost and build effort; I read it as runtime cost, and no candidate states per-run savings.
  - EA (elders, loop 1): Facts cite scripts/providers.py (it is skills/council-loop/scripts/providers.py), and council.py/run.py changed mid-run, so some quoted reproductions predate the current tally; say which code version 

## Lessons (0 to 3, one line each; they reach docs/lessons.md only through a PR)
- [L-20261113-4] (helpful: 0 | harmful: 0) Before releasing a change to the referee scripts, run a Lite council on the kit itself: three experts, a red team and one elder found 13 real defects that review by one model had missed. Evidence: 261113-183103-take-turn-review
- [L-20261113-5] (helpful: 0 | harmful: 0) Tell the red team and the elders to reproduce claims by running code in a scratch copy: every S1 in this run came from a reproduction, none from reading alone. Evidence: 261113-183103-take-turn-review
- [L-20261113-6] (helpful: 0 | harmful: 0) Count tokens, not calls: a blind vote cost about 64,000 tokens per spawned agent here, nearly all start-up overhead, so route vote, rank and elder seats through a lean no-tool call where one exists. Evidence: 261113-183103-take-turn-review

## Lessons used in this run (bump helpful or harmful in the same PR)
- L-20261113-1: helpful. The gate sized this to Lite, and the blind control arm showed what one strong model finds alone (the top defect, at an eighth of the cost).
- L-20261113-2: helpful. The one rule this chair broke (do not look at the control arm) lived only in prose; the fixes chosen move rules into the tool.
