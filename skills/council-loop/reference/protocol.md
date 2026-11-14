# Council Loop protocol (v0.3.0)

ELI5: This is the full rulebook. SKILL.md tells the chair how to run it; this file says what each
step is, what it writes, and what the script checks.

## Roles

| Role | Seats | Does | Never |
| --- | --- | --- | --- |
| Chair | 1 (you) | Writes the brief and rubric, merges duplicates, answers the red team, fuses candidates, makes the cut call, writes the final and the retro | Ranks, votes, or grades candidates |
| Expert | L1…L7 (Full) / L1…L3 (Lite) | One lens each. Writes ideas alone in a fresh context, keeps its best cards, votes blind | Sees another seat's work before the vote |
| Red team | RT | Pre-mortem on the shortlist, with severity tags | Proposes its own winner |
| Elder | EA, EB, EC (Full) / EA (Lite) | Blind review of the top 2 (top 1 in the last Full loop) plus the control card; adds one idea of its own | Knows who wrote what, or which card is the control |
| Control | CTRL | The chair's model in a fresh context answers the ask once, alone, with the same facts the council has | Sees any council work, or shares anything |
| Researcher | RA, RB (0 to 4: RA…RD) | Brings back sourced facts on its beat before anyone has ideas; answers members' questions before later loops | Proposes or votes |
| Mapper | MAP | Writes the codebase map from a script-made skeleton, when the map is missing or behind the code | Proposes or votes |
| Writer | 1 | Builds the winner in a verify loop | Edits locked tests |

The researchers and the mapper are settings, not fixtures: `reference/members-and-memory.md`.

A lens = what it maximises + what it must respect + how it works + the evidence it draws on.
A starting kit for engineering work (change it per task):
1. User value
2. Correctness and verification
3. Simplicity and maintainability
4. Cost, performance and scale
5. Security and failure modes
6. Contrarian (first principles, or 10x)
7. Delivery and operations

## Phases

`brief → [research → ideate → merge → vote → redteam → fuse → rank → elders → cut] × loops → final → retro → done`

`research` runs before the first ideation when the run has researchers or the map is due. Before a
later loop it runs only if a member put a question to the researchers (or the setting says always).

| Phase | Who | Input the script prepares | Output the script validates |
| --- | --- | --- | --- |
| brief | Chair | `brief.yaml` template, lessons from `docs/lessons.md` | Rubric of 5–7 criteria (Lite: 3–7) with weights summing to 100; one lens per expert seat; `approved_by`; `lessons_applied` |
| research | Researchers + MAP | `loop-N/research/<seat>.prompt.md`: the questions (from the brief, else one per criterion, plus members' questions), the beat, what the memory already holds, searches already run; for MAP, `skeleton.md` | Researcher: a `synthesis` and at least one usable fact or one reported gap; a fact needs a claim and a source. Mapper: the required sections, and fewer than three paths that do not exist. Quorum 0: any of them may be dropped by name. Facts are filed in the shared memory; the map is installed |
| ideate | Experts + CTRL (loop 1) | `loop-N/ideate/<seat>.prompt.md`, `control/CTRL.prompt.md` | ≥13 ideas (Lite ≥5); exactly 3 cards kept (Lite 2); card body 60–132 words (aim 90–110), title, ELI5. An idea over 25 words is a warning, never a retry. The control card is checked for identity leaks and sealed (hashed) |
| merge | Chair | `cards.md` (all kept cards + likely overlaps), draft `pool.json` | Pool of ≥2 cards, each with `from` (the kept cards it came from). The tool derives `authors` from `from`, and every kept card must be in some pool card. `reviewed: true`; blindable (no seat ids, similar lengths) |
| vote | Experts | Blind packet of the pool, in a different order for each voter | Full only, and only when the pool has more than N+2 cards. A ballot must rank at least 80% of the labels; a gap counts as no opinion. Shortlist = top N+2, plus any card the vote could not judge |
| redteam | RT | Blind packet of the shortlist | ≤10 findings; severity S1/S2/S3; S1/S2 need a fix. The red team reproduces claims in a scratch copy |
| fuse | Chair | `fuse-input.md` (shortlist, vote scores, findings, last loop's elder feedback), draft `candidates.json` | Exactly N candidates, each with `parents` (the pool cards it was built from; the tool derives `authors`). Every S1/S2 finding answered `fixed`, `rebutted` or `accepted` with a note; optional machine `checks` |
| rank | Experts | Blind packet of the N candidates, in a different order for each voter, with any machine-check results | Ballots, counted as head-to-heads weighted by stake; a failed check ranks last; `best_on` per criterion |
| elders | Elders | Blind packet: top 2 (top 1 in the last Full loop), any card the vote could not judge against them, and the sealed control card | Verdict per card, preferred card, risks, one fix, one own idea; scores per criterion (a gap is a warning) |
| cut | Chair | The standing; judgment points and early-stop hints | `cut --eli5 …` writes `ledger.md`, keeps the survivors, starts the next loop or the final. A judgment point needs `--asked` or `--unattended` |
| final | Chair | `final.md` pre-filled with dropped options, risks, dissent, control result, seats, deviations | No `TODO(chair)` left; answer, ELI5, risks, dissent, success metrics present |
| retro | Chair | `retro.md` pre-filled with call counts, agreement, deviations, seat friction notes | 0–3 lesson lines in the standard format, written to `lessons.proposed.md`. The run is unblinded into `unblinded.json` |

N per loop comes from the schedule. Full with 3 loops: fuse 7, 5, 3 and keep 5, 3, 1. Lite: fuse 3,
keep 1. With `--loops 4`: 7 → 5 → 3 → 2 → 1.

## How votes are counted

A ballot is a ranking of blind labels. The tool reads it as a set of head-to-heads: "this card above
that card".

1. **Stake.** A voter's stake in a card is 1 if it alone wrote it, 1/n if it is one of n authors, 0
   otherwise. Authors come from provenance (`from`, `parents`), never from what the chair types.
2. **Weight.** A head-to-head counts with weight `1 − |stake in one card − stake in the other|`.
   Wrote neither, or an equal share of both: full weight. Wrote one of them alone: no weight (the
   voter sits it out). In between: part weight.
3. **Gaps.** A card missing from a ballot is in none of that voter's head-to-heads. A gap is no
   opinion, never a last place. Label spelling is cleaned first ("Candidate B", "b." → B).
4. **Score.** A card's score is its mean win share over every opponent, where each head-to-head
   starts as one win each: `(wins + 1) / (weight + 2)`. So a card few voters could judge is pulled
   towards 0.5 and cannot win by default. With complete ballots and no stakes, the order equals a
   Borda count.
5. **Thin head-to-heads.** A pair that got less than one full vote is listed as `thin`. A vote may
   not cut what it could not judge: a card below a line that is thin against a card above it stays
   on the shortlist, goes to the elders, or survives the loop, and the run logs a deviation.
6. **Agreement** is Kendall's tau between voters, pooled over the head-to-heads both judged: 1 =
   same order, 0 = unrelated, −1 = opposite. Below 0.2, treat close ranks as ties and lean on
   checks and elders. It is "not measurable" when no two voters judged the same pair, which is
   normal in Lite when each expert wrote one candidate.
7. **Order.** Every voter and every elder reads the same labels in a different order, at evenly
   spaced offsets, so no card is first on every ballot.

## How the cut is decided

1. Start from the rank vote. A failed machine check ranks last.
2. A candidate that most elders mark `kill` drops to last.
3. If the elders reviewed two candidates and more of them prefer the runner-up, it takes the lead.
4. Keep the top N. When N ≥ 3, one cut candidate that the voters named the single best on some
   criterion returns as the **wildcard**, replacing the weakest survivor (highest-weight criterion
   first). The final cut is never touched by the wildcard.
5. The control card is not a candidate. The script counts how many elders preferred it. At the end:
   more than half → "the one-shot baseline won"; exactly half → tie; otherwise the council won.

**Judgment points.** The cut refuses to run on the count alone when the elders split, when an elder
raised an S1, or when the final leader was never reviewed by the elders (they killed the one they
did review). Pass the requester's answer with `--asked "Q -> A"`, or, if they cannot be asked, the safer
option you are taking with `--unattended "<option>"`. Both are written to the ledger.

**Stop early** (`--stop`) when every elder prefers the leader and says ship with no S1, or when the
loop changed nothing. Stopping without that agreement is logged as a deviation.

## Retries, drops and restarts

- A rejected output is moved aside (`<seat>.rejected-1.json`), a retry prompt is written
  (`<seat>.retry.md`: the packet plus the exact problems), and the rejection is counted once.
  Running `ingest` again while the retry is out changes nothing.
- A seat gets one retry. After two rejected outputs `dispatch` stops sending it.
- `ingest --drop <seat> --reason "<why>"` goes on without a named seat. It refuses a seat that
  returned a valid output, a seat on its first rejection, and any drop that would leave fewer than
  two thirds of the experts (one elder is the minimum for the elder step). It never drops a seat
  you did not name, so the control arm cannot disappear by accident.
- `next` and `status` only read. After a restart `next` lists the seats that still owe an output.
- The control card is hashed when it is ingested and read back from disk for the elders. If the
  file is missing or changed the run stops; `next --without-control "<what happened>"` goes on
  without it and logs that.

## What members share, and when

Every seat's output may carry `research` (facts with sources) and `notes` (thoughts, addressed to
`all`, `chair` or `research`). The script files them when the step is accepted; seats never write
the memory themselves. Facts and the codebase map reach every seat that proposes or judges. Thoughts
reach the experts and the red team one step later, unsigned. Nothing shared reaches a blind vote,
and thoughts never reach the elders or the control arm. Facts and notes from voting seats carry no
seat id and no model name until the run is unblinded. A fact with no source, or a line that reads
like an instruction, is refused. Details and the reasons: `reference/members-and-memory.md`.

## Later loops

Experts receive the survivors and the elders' feedback. Every idea must improve, graft, simplify or
invert a survivor; nobody starts over. If a member asked the researchers a question, they answer it
first, and the experts see the answer. The control arm stays the same card in every elder packet,
so each loop shows whether the council is ahead of a single one-shot answer yet.

## Tournament mode

When a numeric scorer exists (benchmark, latency, accuracy, cost), execute each candidate at the
fuse step and record the result under `checks` (pass/fail against a threshold you set in the brief).
Add the measured number to the card's evidence so voters see it.

## Build (Direct tier, or after a Full or Lite run)

1. A different agent writes the acceptance tests first, or mutation-checks the writer's tests.
2. `council lock-tests tests/ <other paths>`. The plugin hook then blocks agent edits to those files
   and to the lock manifest. CI runs `council verify-locks --against origin/<base>`, which reads
   the manifest from the base branch, so editing a test and its hash in the same PR still fails.
   An approved test change carries the PR label `council-unlock`.
3. Verify loop of ≤5 iterations: implement, run the checks, fix.
4. Still failing, or a real design fork appeared: raise the tier. Never lower it silently.

## Learn

- **Retro:** 0–3 lessons, each `- [L-YYYYMMDD-n] (helpful: 0 | harmful: 0) Text. Evidence: <run id>`.
  They reach `docs/lessons.md` only through a PR. `init` shows the top lessons at the next brief.
- **Counters:** when a lesson was applied, the retro says whether it helped or hurt; bump its counter
  in the same PR. Remove a lesson once harmful exceeds helpful.
- **Friction notes:** every seat can report one line about what was unclear in its packet. They
  land in the retro. Fix the packet templates, not the seats.
- **Outcome ledger:** add the winner's success metric, kill criterion and control-arm result to
  `docs/outcomes.md`; check again at 30 and 90 days. Resolved cases join the golden set.
- **Golden set:** 20–50 past tasks with known-good outcomes. Replay it before merging a change to
  packets, policy or models. Merge only if quality per dollar does not regress.
- **Fire drill, monthly:** canary tasks try to edit a locked test, run a migration and publish. A
  gate that fails to fire blocks the next merge.

## Files of a run (`.council/runs/<id>/`)

```text
run.json                      state, written only by the script
brief.yaml  facts.md          the chair's brief; optional longer facts
focus-map.md                  the code this ask points at, ranked (empty when the ask names no code)
control/CTRL.prompt.md .out.json
loop-1/research/RA.prompt.md  RA.out.json  MAP.prompt.md  MAP.out.md  skeleton.md
loop-1/ideate/L1.prompt.md  L1.out.json  (L1.retry.md and L1.rejected-1.json after a rejection)
loop-1/cards.md  pool.json
loop-1/vote/packet.md  L1.prompt.md  L1.out.json
loop-1/redteam/packet.md  RT.prompt.md  RT.out.json
loop-1/fuse-input.md  candidates.json
loop-1/rank/…   loop-1/elders/…
loop-1/ledger.md
final.md  retro.md  lessons.proposed.md
unblinded.json                who wrote which blinded card, written when the run ends
```

While a run is live, who wrote which blinded card is stored outside the project, in
`~/.council-private/<run id>.json` (or `$COUNCIL_PRIVATE_DIR`), so a seat that reads the run folder
cannot find it by accident. Blinding here guards against bias, not against a seat that sets out to
cheat. Labels are re-derived from the run id, so a lost private store is rebuilt by `next`.

Outside the run folder, kept across runs: the shared memory (`.council/memory/` or an Obsidian
vault: `research/` one fact per note, `commons/` one page per run, `index.md`), the codebase map
(`docs/ARCHITECTURE.md`) and what it was checked against (`.council/map/state.json`).

When the run ends the mapping is copied into `unblinded.json`, and the names are added to the
facts and the commons page that voting seats wrote. `council audit` then re-counts
every vote from the ballots on disk, compares the order with the record, and checks that each
accepted seat output and the sealed control card still match the hashes taken at ingest. It exits 1
on any mismatch, so it can run in CI.

## Seat output schemas

```json
ideate:  {"lens": "L1", "eli5": "…", "ideas": ["…"], "cards": [{"title": "…", "body": "…", "wins": ["R1"], "evidence": "…", "eli5": "…"}], "friction": "…"}
vote:    {"voter": "L1", "ranking": ["B", "A", "C"], "reasons": {"B": "R1: …"}, "best_on": {"R1": "B"}, "friction": "…"}
redteam: {"findings": [{"candidate": "A", "severity": "S1", "finding": "…", "evidence": "…", "fix": "…"}], "eli5": "…"}
elder:   {"scores": {"A": {"R1": 8}}, "verdict": {"A": "ship"}, "preferred": "A", "confidence": 0.7, "top_risks": [{"risk": "…", "severity": "S2"}], "key_fix": "…", "own_idea": "…", "eli5": "…"}
control: {"title": "…", "body": "…", "eli5": "…"}
research: {"synthesis": "…", "research": [{"claim": "…", "source": "https://… | path | test: cmd", "quote": "…", "date": "…", "kind": "paper", "confidence": "high", "tags": ["…"], "why": "…"}], "conflicts": ["…"], "gaps": ["…"], "queries": ["…"], "notes": [{"to": "all", "text": "…"}], "eli5": "…"}
map:     a Markdown document (MAP.out.md), not JSON
```

Any of ideate, redteam and elder may add `"research": [facts]` and `"notes": [{"to": "all|chair|research", "text": "…"}]`;
a ballot may add `notes`. Both are optional.

The script accepts JSON wrapped in prose or a code fence. A seat is sent back only for a fault the
run would act on: too few ideas, a malformed card, a ballot that ranks under 80% of the cards, a
missing verdict. Word caps on cards have 10% grace (a warning). Idea length, missing elder scores
and small ballot gaps are warnings, because a retry is a whole new agent.
