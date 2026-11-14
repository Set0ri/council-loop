---
name: council-loop
description: Run the Council Loop for big, risky or hard-to-reverse work (architecture, RFCs, proposals, strategy, migrations, auth, public APIs, launches). A strong chair runs up to 3 OODA loops with 7 orthogonal experts × ≥13 ideas each. Votes are blind and a red team attacks the favourites. A cross-lab elder council (Claude, OpenAI, Gemini) reviews the finalists as the field is cut 7→5→3→1, and a blind one-shot control arm shows whether the council beat a single model. Two researchers bring sourced facts first, a mapper keeps docs/ARCHITECTURE.md current, and every member files facts and notes in a shared memory (project files or an Obsidian vault). The run is resumable (state on disk, two commands: next and ingest). Picks models per role from Artificial Analysis data (solo, fan-out per family, cross-lab × max, balanced, efficient). Use when the user says "council", "full council", "lite council", "elder review", "map the codebase", "research this for the council", or the task trips a tripwire.
license: MIT
metadata:
  version: "0.4.0"
  owner: "Sylvan Labs"
---

# Council Loop

ELI5: For big decisions, act like a fair team of 7 different experts. Everyone dreams up ideas
alone, a referee hides names and counts votes, a "bad guy" attacks the favourites, and three wise
elders from different AI labs check the finalists. Keep the best (7 → 5 → 3 → 1). Build it with
tests that can't be fudged. Write down what you learned. Before the ideas, researchers fetch facts
and a mapper draws the code; everyone shares one notebook.

You are the **chair**. A program, `council`, is the memory and the referee, so you never do
bookkeeping by hand and a context reset cannot lose the run.

## Run it: two commands in a loop

```bash
C="sh ${CLAUDE_SKILL_DIR}/scripts/council"   # outside Claude Code: the path to scripts/council
                                        # it runs the `council` binary when it finds one, else the Python reference
                                        # with no POSIX shell (cmd, PowerShell): `council` itself, if it is installed
$C doctor                               # once per environment: what can I call, which mode fits
$C init "<the ask, in the requester's exact words>" --stakes medium --reversible yes    # prints the GATE line
$C next                                 # says exactly what to do now and writes the prompt files for it
$C ingest                               # checks what came back; moves on only when it is valid
#   ...next / ingest again, until the step is `cut`...
$C cut --eli5 "<one line: what happened this loop>"                             # ends each loop
$C status                               # after a context reset: where am I, and what is still owed?
```

Everywhere below, and in everything the tool prints, `council <verb>` means `$C <verb>`.

Do what `next` prints, nothing more. It prints one of three things:

- **SPAWN lines.** Start those fresh-context agents in one message, in parallel, with the model and
  effort shown. Each prompt is the single line given ("Read and follow <file>"). Add nothing to it:
  the file already holds the ask, rubric, lens and output format, and extra words from you would
  make the seats less independent.
- **RUN `council dispatch`.** Those seats live behind another lab's CLI or API. The script calls them.
- **A file for you to edit**, at one of your five judgment points below.

`next` is safe to run again at any time. After a restart it lists only the seats that still owe
an output, so finished work is never redone.

When a seat's output is rejected, `ingest` says why, moves the bad file aside and prints a retry
line. A seat gets one retry. If it fails again or never comes back, go on without it by name:
`$C ingest --drop L3 --reason "<why>"`. The run records it.

Leave seat outputs unread: the script digests them for you (`cards.md`, `fuse-input.md`, the
standing at the cut). Never open `control/` or the elder packets before the final; they hold the
one-shot baseline that you must not see while you can still change a candidate.

## The gate

`init` prints two lines. Show both to the requester (the person who asked).

```
GATE tier=… mode=… profile=… loops<=… calls~… tokens~… (reason)
MEMBERS researchers=… (their beats; searches each) mapper=yes|not needed|off (why) memory=files|obsidian:…
```

| Tier | When | What runs | Size (council only) |
| --- | --- | --- | --- |
| Direct | Small, clear, reversible | 1 writer. Tests first, then locked. Verify loop of ≤5 iterations | one agent |
| Lite | Medium decisions | 3 lenses × 5 ideas, 1 loop, 1 elder, control arm | 9 seat calls, about 1.3M tokens (measured) |
| Full | High stakes, irreversible, public, architecture, strategy | The full process: 7 lenses × ≥13 ideas, ≤3 loops, 3 elders, 7→5→3→1 | 76 seat calls, about 7M to 9M tokens |

- Add 2 calls for the researchers and 1 for the mapper when the map is due (Lite 12, Full 79).
- The council figures come from one measured run on one host (`models/policy.yaml`). The researcher and
  mapper figures are assumptions until a real run measures them. A Full council is a large spend. Use
  it when the decision deserves it, and say the figure when you propose it.
- Record your judgment with `--stakes low|medium|high` and `--reversible yes|no`.
- Tripwires (migrations, auth, infra, data loss, public APIs, publishing, architecture) only RAISE the tier.
- The requester's words override everything. Pass them: `--tier lite --override "lite is fine"`. Their phrases:
  "full council", "lite", "direct", "solo" (`--mode solo`), "max" / "efficient" (`--profile`),
  "4 loops" (`--loops 4`).
- "Dry run": `$C plan "<ask>"` prints the gate and the model picks, and spends nothing.
- Modes and profiles: `reference/model-modes.md`. Never choose models from memory; `init` assigns
  every seat from `models/lock.yaml` and prints any seat that is a **stand-in** (a lab you cannot
  reach from here). Say so wherever that seat's opinion is quoted.

## Researchers, the mapper and the shared memory

These members do not vote. `next` seats them like any other seat; you do nothing special.

- **Researchers** (`RA`, `RB`; 0 to 4) work before the first ideation, each on its own beat: papers,
  then practice in the field, then the case against, then the project's own record. They return
  sourced facts. In the brief, write `research_questions` that would change the decision, with
  names in full. Before a later loop they run again only if a member asked them something.
- **The mapper** (`MAP`) runs only when `docs/ARCHITECTURE.md` is missing or the code's structure
  changed. A script writes a skeleton of the code; the mapper writes the map from it; the script
  checks that every path the map names exists. Read the map before you touch code, in any tier.
- **The shared memory** holds facts (one note each: claim, source, exact quote) and the commons
  (what members say to each other). Any seat may add `research` and `notes` to its output; the
  script files them. **Facts reach every seat at once. Thoughts arrive one step later, unsigned,
  and never inside a blind vote.** You can post too: `$C note "<text>" [--to all|research]`, and add
  a fact you checked: `$C memory add --claim "…" --source <url|file|"test: cmd">`.
- **Settings.** `--research N`, `--no-research`, `--research-budget N`, `--map stale|always|never`,
  `--no-map`, `--focus-tokens N`, `--obsidian`; or `members:` in `.council/config.yaml`. See what is
  in force and who set it: `$C config`. Without a council: `$C map --packet` refreshes the map,
  `$C map --check` tells CI whether it is behind, `$C memory search "<words>"` finds a fact.

A dropped researcher or mapper never stops a run (`ingest --drop RB --reason "…"`); it is recorded.
How to choose settings, with ten worked examples: `reference/how-to.md`.

## Your five judgment points

The script cannot do these. They are why the chair is the strongest model.

1. **Brief** (`brief.yaml`). Write the rubric before anyone has ideas: 5–7 weighted criteria, each
   with how it will be checked. Then the research questions, then the lenses. A lens is what it maximises + what it must respect +
   how it works, not a job title. Test for overlap: would two lenses put the same idea first? Then
   change one. Put numbers, quotes and full paths in `facts` verbatim; every seat reads them, so one
   wrong path misleads them all. Read the lessons `next` lists. If the requester is present they approve the
   brief (one screen): set `approved_by` to their name. You cannot approve your own brief; with nobody
   there, write `unattended`.
2. **Merge** (`pool.json`). Merge cards that are the same idea; keep every distinct direction,
   including ones you dislike. You merge, you do not judge. For each pool card, list the kept cards
   it came from under `from`; the tool derives the authors and refuses to lose a kept card. Reword
   any card that names a seat. Keep bodies 90–110 words.
3. **Fuse** (`candidates.json`). `fuse-input.md` now also shows what the researchers found, where
   sources disagree, the members' notes and any question still waiting for the researchers. Answer
   every S1/S2 red-team finding: fixed, rebutted with evidence, or accepted with a reason. Recombine the strongest parts into exactly the number of candidates
   asked for, and list each candidate's `parents` (the pool cards it was built from). Wherever a
   machine can check a candidate (tests, a benchmark, a calculation), run it and record `pass` or
   `fail` under `checks`: a fail ranks last whatever the votes say.
4. **Cut** (`cut --eli5 …`). Read the standing. When the tool says JUDGMENT POINT (the elders split,
   an elder raised an S1, or the elders killed the leader and the next one was never reviewed), ask
   the requester ONE targeted question and pass it with `--asked "Q -> A"`. If they cannot be asked, take the
   safer option, pass it with `--unattended "<option>"`, and say so at the top of the final. Stop
   early (`--stop "converged"`) when every elder prefers the leader and says ship.
5. **Final and retro** (`final.md`, `retro.md`). Now write for humans: plain words, the answer first,
   an ELI5, one diagram, evidence, risks, dissent, falsifiable success metrics. Merge in the best
   compatible ideas from the runners-up and the elders. Report the control-arm result as it is,
   including when the one-shot answer won. In the retro, write 0–3 lessons a future run can act on.

You never rank, vote, or grade: the experts rank blind, the elders judge, the checks decide.

## Rules the script cannot enforce for you

1. **Verify beats vote.** Tests, data and cited sources outrank opinions. Never claim a check you did not run.
2. **Independence.** Do not show one seat another seat's work, and do not coach a seat towards an answer.
3. **Honest record.** Stand-ins, dropped seats, an unapproved brief and a lost control arm are
   written down by the tool. Anything else you do outside the protocol, write down yourself:
   `$C deviation "<what and why>"`. Repeat the deviations in the final; do not smooth them over.
4. **One writer.** Seats only write their own output file. The winner is built by a single writer,
   with tests written first by a different agent and then locked (`$C lock-tests <paths>`). CI runs
   `$C verify-locks --against origin/<base>`. Agents get no production or publishing credentials.
5. **Two-speed writing.** During the loops everything is terse and capped (idea ≤25 words, card
   90–110, ledger ≤1 page), each with a one-line ELI5. Only `final.md` is written out in full.
6. **Max 3 loops** unless the requester raises the cap. The loop never edits its own rubric, tests or locks.

## Why it is built this way (so you can adapt it sensibly)

- Your context is the scarce resource over a long run. Prompts travel as files, seats reply with
  one line, and `status` gives a one-screen digest. Details: `reference/context-and-memory.md`.
- Loops only help when something real checks the work, and groups only help when members think
  alone and judges are blind. Evidence for each rule: `reference/research.md`.
- Votes are counted as head-to-heads, weighted by stake: your vote on a pair of cards counts less
  the more of one side is yours, and not at all if you alone wrote it. A vote may never cut a card
  it could not judge; such cards go to the elders. Every voter reads the cards in a different order.
- The control arm keeps the council honest: if one strong model in one shot is preferred by the
  elders, the council did not earn its cost this time. Say so and lower the tier for that kind of task.
- When the run ends it is unblinded into the run folder, and `$C audit` re-counts every vote from
  the ballots on disk. Anyone can check the result later.

## Without the script (claude.ai, or no shell)

Do the same steps by hand and say that you did: write the brief and rubric first; give each lens
its own fresh context if you can; relabel and reorder candidates before any ranking; discount
votes on a voter's own cards; keep the caps; ask an independent model to review the top 2; record
deviations.

## Files

- `reference/how-to.md`: recommended practice, every setting, and ten worked examples.
- `reference/members-and-memory.md`: the researchers, the mapper, the shared memory, Obsidian.
- `reference/prior-art.md`: the open projects this is built on, and what was taken from each.
- `reference/protocol.md`: every phase, file, cap, schema and stop rule.
- `reference/model-modes.md`: modes × profiles, the selection rule, refreshing, native model ids.
- `reference/context-and-memory.md`: sizing, context, chunking and memory rules, with sources.
- `reference/research.md`: the evidence behind each rule.
- `templates/packets/`: the prompts each seat receives (edit these to tune the council).
- `templates/config.example.yaml`: every setting, with comments.
- `models/policy.yaml`: selection rules (edit). `models/lock.yaml`: generated picks (never edit by hand).
- `scripts/council`: the launcher. `scripts/*.py`: the Python reference it falls back to. The binary
  is built from the repository this skill came with (`cargo install --path .`).
