# Tutorial 1: your first council

> ELI5: before you spend anything, watch a whole council play out in a second, with a script in
> every seat. Then open what it left behind, so that you know what a real run will give you.

**Time:** 5 minutes. **Needs:** Rust 1.85 or newer and a C compiler, to build once. No model is
called and nothing leaves your machine.

## 1. Install

In a checkout of this repository:

```console
$ cargo install --path .
$ council --version
council 0.5.0
```

That is the whole installation: one program, with its templates, reference pages and model data
compiled in.

## 2. The first-run check

The first command you run prints a short check before its own output. It looks at your machine
(nothing is sent anywhere) and takes about three milliseconds:

```console
$ council plan "Pick the queue technology for the billing pipeline"
council 0.5.0 · first run on this machine (a quick local check: nothing was sent anywhere)
  [ok] seats can start: through the claude CLI
  [ok] lab reachable: Anthropic (a second lab adds an independent review: install codex or gemini, or set OPENROUTER_API_KEY)
  [ok] model data: 1 day(s) old
  [ok] git: found
  [ok] shared memory: plain files in each project's .council/memory
  a run here defaults to mode=fanout:claude profile=efficient (the cheapest that works; rel. cost 0.66)
  see a whole council now, with no model and at no cost:  council demo
  start a real one:                                       council init "<the ask, in the requester's own words>"
  this check again, in full:                              council doctor
GATE tier=lite mode=fanout:claude profile=efficient loops<=1 calls~11 tokens~1.7M rel_cost=0.66 (default: pass --stakes and --reversible to record your judgment)
MEMBERS researchers=2 (papers and primary research; practice in the field; about 10 searches each) mapper=not needed (no source code found in this project) memory=files:.council/memory
```

Yours will differ: it depends on which AI tools are installed. Read the two lines at the end.

- **GATE** is the size of the council and what it costs: the tier (`direct`, `lite` or `full`), the
  mode (which labs sit at the table), the profile (how strong the models are), the number of seat
  calls and tokens, and the relative cost of the models.
- **MEMBERS** is who else sits there: the researchers, the mapper and where the shared memory lives.

`plan` is a dry run: it spends nothing. The check appears once; `council doctor` shows it in full
at any time.

## 3. Watch a council

```console
$ council demo
council demo · a whole Lite council on a made-up question, played by a script.
No model is called, nothing is spent and nothing leaves this machine. Every command is the real one.
project: /tmp/council-demo-8b7b1465 (a toy billing service)

 1  $ council init "Pick the queue technology for the billing pipeline" --tier lite --mode fanout:claude --profile efficient
      > GATE tier=lite mode=fanout:claude profile=efficient loops<=1 calls~12 tokens~1.8M rel_cost=0.66 (explicit)
      (script) the chair fills in the brief: the question restated, a rubric written before anyone has ideas, one lens per expert
 2  $ council ingest
      > brief accepted: rubric frozen for this run
      (script) the researchers bring sourced facts; the mapper writes the code map (MAP, RA, RB)
 3  $ council ingest
      > codebase map written: docs/ARCHITECTURE.md
      > research in: 9 new fact(s) in the shared memory, 1 already held; not found: 1 gap(s) reported
      (script) every expert works alone on the same ask through its own lens; the control arm gives a one-shot answer (L1, L2, L3, CTRL)
 4  $ council ingest
      > ideas in: 18 from 3 experts; 6 cards kept
      (script) the chair reviews the drafted pool (same ideas merged, authors out of sight) and approves it
 5  $ council ingest
      > pool: 6 cards (small enough: no shortlist vote, all go to the red team)
      (script) the red team attacks the blinded shortlist (RT)
 6  $ council ingest
      > red team: 6 findings (S1 3, S2 2, S3 1)
      (script) the chair answers every finding of the red team in writing: fixed, accepted or refuted
 7  $ council ingest
      > 3 candidates fused; every S1/S2 finding has a response; not carried into any candidate: P4, P5, P6
      (script) every voter ranks the blinded cards, each in a different order (L1, L2, L3)
 8  $ council ingest
      > rank vote: 3 ballots, agreement 1.0 (strong); order C1 > C2 > C3
      (script) the elders review the blinded finalists (EA)
 9  $ council ingest
      > 1 elder review(s) in
10  $ council cut --eli5 "one plan won on reliability; its loose end is fixed" --changed "the winner gained an expiry rule for its keys"
      > cut: kept C1; eliminated C2, C3
      (script) the chair writes the decision for people: the answer, the evidence, what was dropped and why
11  $ council ingest
      > final accepted
      (script) the chair writes what was learned (a lesson reaches docs/lessons.md only through a pull request)
12  $ council ingest
      > retro accepted: 1 lesson(s) proposed in <project>/.council/runs/<run>/lessons.proposed.md
      > protocol check: ok; warnings: none

done: 12 commands in 0.1 s. The run went brief -> research -> ideate -> merge -> redteam -> fuse -> rank -> elders -> cut -> final -> retro -> done.
the decision:  Managed queue with idempotency keys
the audit:     audit: ok
```

(A few lines are left out here. `council demo --all` prints every line each command printed.)

What you just saw:

- Lines that start with `$` are **the real commands**, run through the real program in a
  throwaway project.
- Lines that start with `(script)` stand for **what a model or a person does** between commands:
  the chair writing the brief, seats writing their answers. In a real run those are models. Here a
  script wrote plausible answers.
- Lines that start with `>` are **what the program answered**: it checked each answer, filed the
  facts, hid the authors, counted the votes and made the cut.

That is the division of labour in every run. The models think. The program keeps the books.

## 4. Keep one and look around

```console
$ council demo --keep
...
the folder is kept. Look around:  cd /tmp/council-demo-2307e931 && council status && council log
$ cd /tmp/council-demo-2307e931
```

**Where the run stands** (this is the command to use after any interruption):

```console
$ council status
GATE tier=lite mode=fanout:claude profile=efficient loops<=1 calls~12 tokens~1.8M rel_cost=0.66 (explicit)
run 261114-192013-pick-queue-technology · phase done · loop 1/1
ask: Pick the queue technology for the billing pipeline
brief ELI5: Pick the safest way to carry bills from the shop to the ledger.
rubric: R1 no double charge, no lost event (40), R2 cost to run (20), R3 simple to operate (20), R4 time to ship (10), R5 order kept per account (10)
lenses: L1 least that can go wrong, L2 least to build and run, L3 room to grow
loop 1: one plan won on reliability; its loose end is fixed  -> kept C1 Managed queue with idempotency keys
winner: C1 · control arm: the council's answer was preferred by 1 of 1 elder(s)
shared memory: files at <project>/.council/memory · this run: 9 fact(s) added, 5 note(s), 3 question(s) waiting for the researchers
codebase map: docs/ARCHITECTURE.md (written in this run)
calls so far: 12 (estimate 12)
next: nothing, the run is finished. The decision: final.md in the run folder · re-count the votes: council audit
```

**The decision, written for people** (`.council/runs/<run>/final.md`): the answer first, an ELI5,
the evidence with its source, what was dropped and why, the risks, any dissent, how you will know
it worked, and the facts of the run.

```markdown
# Go with: Managed queue with idempotency keys

**Answer:** Put every billing event on a managed queue and give each event an idempotency key made from the account id and the invoice number.

**ELI5:** A mail service that may deliver twice, and a notebook that stops us paying twice.

## What we considered and dropped
| Option | Why it lost |
| --- | --- |
| Outbox table inside the billing database | cut in loop 1: it ranked below the winner in the blind vote |
| Log stream partitioned by account | cut in loop 1: it ranked below the winner in the blind vote |

## How we'll know (falsifiable)
- Success metric at 30 and 90 days: no invoice billed twice in 30 days; 90 days without a manual correction
- Kill criterion: one double charge after launch: stop and fall back to the nightly batch
```

**The loop's record** (`loop-1/ledger.md`): one page that says how the decision was reached.

```markdown
- Ideas: 18 from 3 experts -> 6 kept -> pool of 6 -> 6 to the red team -> 3 fused
- Red team: S1 3, S2 2, S3 1. Responses: fixed 5
- Rank vote: 3 blind ballots, 5 head-to-heads counted, 4 sat out (the voter alone wrote one side); agreement 1.0 (strong)
  - C1 0.73: Managed queue with idempotency keys
  - C2 0.43: Outbox table inside the billing database
  - C3 0.33: Log stream partitioned by account
- Elder EA (Claude Opus 5.5 (xhigh)): prefers C1; C2 revise, C1 ship; confidence 0.8. Fix: write the key and the ledger entry in one transaction, and test it with a forced retry
- Cut: kept C1; eliminated C2, C3
- Control arm: preferred by 0 of 1 elder(s)
```

Three things to notice in that record:

- **"4 sat out".** A voter's opinion of its own card does not count. The program knows who wrote
  what; the voters do not.
- **The elder** judged the finalists without knowing who wrote them or what the council thought.
- **The control arm** is one model answering the same question in one shot, hidden among the
  finalists. Here the elder preferred the council's answer. When the one-shot answer wins, the
  final says so, and you should use a smaller council for that kind of question next time.

**Check it yourself.** The audit counts every vote again from the ballots on disk:

```console
$ council audit
loop 1 rank: ok  recorded C1 > C2 > C3 (3 ballots)
seat outputs: 12 of 12 match what was ingested
control arm: sealed card intact
audit: ok
```

**What happened, and when:**

```console
$ council log -n 4
run 261114-192013-pick-queue-technology · 4 event(s) · <project>/.council/runs/261114-192013-pick-queue-technology/events.jsonl
19:20:13  ingest     elders -> cut          ok     1 ms
19:20:13  cut        cut -> final           ok     1 ms
19:20:13  ingest     final -> retro         ok     1 ms
19:20:13  ingest     retro -> done          ok     9 ms
```

## 5. The files of a run

```text
.council/runs/<run>/
  brief.yaml               the question, the rubric and the lenses, written before anyone has ideas
  loop-1/research/         what the researchers and the mapper were asked, and what they answered
  loop-1/ideate/           each expert's packet (L1.prompt.md) and answer (L1.out.json)
  loop-1/cards.md          the kept ideas, digested for the chair
  loop-1/redteam/          the attack on the shortlist
  loop-1/rank/             the blind ballots
  loop-1/elders/           the elders' reviews
  loop-1/ledger.md         the loop's record
  control/                 the sealed one-shot answer
  final.md  retro.md       the decision, and what was learned
  run.json                 the state: any step can be picked up from here
  events.jsonl             the diary
  unblinded.json           who wrote what, added when the run is over
.council/memory/           the facts the researchers filed, one note each, with its source
docs/ARCHITECTURE.md       the map of the code the mapper wrote
```

Everything is a plain file. Nothing is hidden in a database, and a run can be resumed, audited or
read by a person at any point.

## What next

- A bigger one: `council demo --full` plays a Full council, three loops with seven experts, in
  about a third of a second.
- A real one: [Tutorial 2](02-a-real-council.md).
- With another AI tool: [Tutorial 3](03-other-ai-tools.md).
