# Loop 1 ledger · 261113-183103-take-turn-review

ELI5: Three experts found real stalls and a vote-counting bug; all three ranked the mid-size fix package first; the elder agreed but asked for one more guard.

- Ideas: 31 from 3 experts -> 6 kept -> pool of 5 -> 5 to the red team -> 3 fused
- Red team: S1 2, S2 3, S3 5. Responses: fixed 5
- Rank vote: 3 blind ballots, 7 head-to-heads counted, 2 sat out (the voter helped write one side); agreement 1.0 (strong)
  - C3 0.78: Defects plus four changes that remove typing
  - C2 0.47: Full hardening in dependency order
  - C1 0.25: Fix the five proven defects, then release
- Elder EA (Claude Opus 5.5 (max)): prefers C3; C3 revise, C2 revise; confidence 0.6. Fix: Graft B's guard, re-specified for the current head-to-head tally: a vote never cuts what it could not judge. If a top-2 candidate has a head-to-head no unconflicted voter could judge (or fewer than 2 judges), send the top 2 to the elders even in the last Full loop, keep such cards on the shortlist, and log a deviation. Test: a unanimous favourite written by 6 of 7 seats must reach the elders.
- Cut: kept C3; eliminated C2, C1
- Control arm: preferred by 0 of 1 elder(s)
- Unattended at a judgment point; safer option taken: build the winner with the elder's S1 fix grafted in (a vote must never cut a card it could not judge) and the control arm sealed; nothing is released yet, so every step can be undone
